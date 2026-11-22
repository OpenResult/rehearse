#![forbid(unsafe_code)]
//! GitHub Actions reporting through rehearse's existing progress listener.
//!
//! Reports are observations, not execution checkpoints. Explicit finalization
//! exposes reporting failures without changing or hiding operation failures.

use rehearse::{
    Impact, NodeId, PlanGraphDescription, ProgressEvent, ProgressListener, ProgressMode,
    ProgressOutcome, ProgressPlanOutcome,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

/// Presentation and output choices for one observed traversal.
#[derive(Debug, Clone)]
pub struct ReportOptions {
    output_dir: PathBuf,
    summary_path: Option<PathBuf>,
    metadata: BTreeMap<String, String>,
    live_log: bool,
}

impl ReportOptions {
    /// Creates options for a new or empty directory. Existing files are refused.
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            summary_path: None,
            metadata: BTreeMap::new(),
            live_log: true,
        }
    }

    /// Also appends the completed Markdown summary to this file.
    pub fn with_summary_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.summary_path = Some(path.into());
        self
    }

    /// Uses `GITHUB_STEP_SUMMARY` when present. No other environment is read.
    pub fn with_github_summary(mut self) -> Self {
        if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            self.summary_path = Some(path.into());
        }
        self
    }

    /// Adds application-supplied identity such as run ID, commit, or stack.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Enables or disables live GitHub workflow-command lines (enabled by default).
    pub fn with_live_log(mut self, enabled: bool) -> Self {
        self.live_log = enabled;
        self
    }
}

/// Caller-visible result, including application checks after interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutcome {
    Succeeded,
    Incomplete,
    Failed,
}

/// The interpretation selected by the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportMode {
    Describe,
    DryRun,
    Execute,
}

impl From<ProgressMode> for ReportMode {
    fn from(mode: ProgressMode) -> Self {
        match mode {
            ProgressMode::Describe => Self::Describe,
            ProgressMode::DryRun => Self::DryRun,
            ProgressMode::Execute => Self::Execute,
        }
    }
}

/// Observed node status. Unvisited nodes remain explicitly `NotStarted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    NotStarted,
    Running,
    Executed,
    Skipped,
    Denied,
    Blocked,
    Failed,
    Internal,
    Described,
}

impl NodeStatus {
    fn label(self) -> &'static str {
        match self {
            Self::NotStarted => "Not started",
            Self::Running => "Running",
            Self::Executed => "Executed",
            Self::Skipped => "Skipped",
            Self::Denied => "Denied",
            Self::Blocked => "Blocked",
            Self::Failed => "Failed",
            Self::Internal => "Internal error",
            Self::Described => "Described",
        }
    }
}

/// A declaration-ordered row in the completed report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeReport {
    pub id: NodeId,
    pub position: usize,
    pub name: String,
    pub label: String,
    pub impact: Impact,
    pub status: NodeStatus,
    pub duration_ms: Option<u64>,
    pub message: Option<String>,
    pub missing_dependencies: Vec<NodeId>,
}

/// Versioned report without operation bodies, literal inputs, or stored values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub schema_version: u32,
    pub plan_name: String,
    pub plan_revision: Option<String>,
    pub mode: Option<ReportMode>,
    pub outcome: ReportOutcome,
    pub duration_ms: u64,
    pub metadata: BTreeMap<String, String>,
    pub nodes: Vec<NodeReport>,
}

/// An owned event in `events.jsonl`; sequence reflects observation order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportEvent {
    pub schema_version: u32,
    pub sequence: u64,
    pub elapsed_ms: u64,
    pub plan_name: String,
    pub plan_revision: Option<String>,
    pub kind: EventKind,
    pub mode: ReportMode,
    pub node_id: Option<NodeId>,
    pub status: Option<NodeStatus>,
    pub message: Option<String>,
    pub missing_dependencies: Vec<NodeId>,
}

/// The event types emitted by the listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    PlanStarted,
    NodeStarted,
    NodeFinished,
    NodeDescribed,
    PlanFinished,
}

/// Structured failures at the reporting boundary, separate from workflow errors.
#[derive(Debug)]
pub enum ReportingError {
    SummaryInsideOutput {
        summary_path: PathBuf,
        output_dir: PathBuf,
    },
    InvalidDirectory {
        path: PathBuf,
        reason: &'static str,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    UnknownNode {
        node: NodeId,
    },
    InvalidTraversal {
        reason: &'static str,
    },
    Multiple {
        errors: Vec<ReportingError>,
    },
}

impl fmt::Display for ReportingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SummaryInsideOutput {
                summary_path,
                output_dir,
            } => write!(
                f,
                "GitHub summary {} must be outside report directory {}",
                summary_path.display(),
                output_dir.display()
            ),
            Self::InvalidDirectory { path, reason } => {
                write!(f, "invalid report directory {}: {reason}", path.display())
            }
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "{action} {}: {source}", path.display()),
            Self::Json { path, source } => write!(f, "serialize {}: {source}", path.display()),
            Self::UnknownNode { node } => {
                write!(f, "progress references unknown graph node {}", node.index())
            }
            Self::InvalidTraversal { reason } => write!(f, "invalid report traversal: {reason}"),
            Self::Multiple { errors } => {
                write!(f, "{} reporting failures", errors.len())?;
                if let Some(first) = errors.first() {
                    write!(f, "; first: {first}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ReportingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Multiple { errors } => errors.first().map(|error| error as &dyn Error),
            _ => None,
        }
    }
}

/// Observes one traversal and produces GitHub logs plus local report artifacts.
///
/// Supply the graph from the exact plan being observed; plan-local IDs are not
/// identities across separately constructed plans.
///
/// Call [`Self::finish`] on success and failure. A listener cannot return an I/O
/// error, so callback errors are retained and returned during finalization.
pub struct GitHubReporter {
    options: ReportOptions,
    report: RunReport,
    started: Instant,
    starts: HashMap<NodeId, Instant>,
    sequence: u64,
    events: File,
    log: Box<dyn Write + Send>,
    observed_outcome: Option<ReportOutcome>,
    errors: Vec<ReportingError>,
}

impl GitHubReporter {
    /// Writes live workflow commands to stdout.
    pub fn new(
        graph: PlanGraphDescription,
        options: ReportOptions,
    ) -> Result<Self, ReportingError> {
        Self::with_writer(graph, options, io::stdout())
    }

    /// Chooses the live log writer, for example stderr to preserve JSON stdout.
    pub fn with_writer(
        graph: PlanGraphDescription,
        options: ReportOptions,
        writer: impl Write + Send + 'static,
    ) -> Result<Self, ReportingError> {
        if let Some(path) = &options.summary_path {
            if resolved_path(path)?.starts_with(resolved_path(&options.output_dir)?) {
                return Err(ReportingError::SummaryInsideOutput {
                    summary_path: path.clone(),
                    output_dir: options.output_dir.clone(),
                });
            }
        }
        prepare_directory(&options.output_dir)?;
        write_json_new(&options.output_dir.join("graph.json"), &graph)?;
        let events_path = options.output_dir.join("events.jsonl");
        let events = create_new(&events_path)?;
        let report = RunReport {
            schema_version: 1,
            plan_name: graph.plan_name().to_owned(),
            plan_revision: graph.plan_revision().map(str::to_owned),
            mode: None,
            outcome: ReportOutcome::Failed,
            duration_ms: 0,
            metadata: options.metadata.clone(),
            nodes: graph
                .nodes()
                .iter()
                .map(|node| NodeReport {
                    id: node.id(),
                    position: node.position(),
                    name: node.name().to_owned(),
                    label: node
                        .annotations()
                        .get("label")
                        .cloned()
                        .unwrap_or_else(|| node.name().to_owned()),
                    impact: node.impact(),
                    status: NodeStatus::NotStarted,
                    duration_ms: None,
                    message: None,
                    missing_dependencies: Vec::new(),
                })
                .collect(),
        };
        Ok(Self {
            options,
            report,
            started: Instant::now(),
            starts: HashMap::new(),
            sequence: 0,
            events,
            log: Box::new(writer),
            observed_outcome: None,
            errors: Vec::new(),
        })
    }

    /// Completes artifacts and appends the optional job summary.
    ///
    /// `outcome` includes application-level checks such as `require_complete`.
    /// Observed failures cannot be downgraded by passing a successful outcome.
    /// Calling this while a run is interrupted leaves active nodes `Running`;
    /// it never fabricates completed operations.
    pub fn finish(mut self, outcome: ReportOutcome) -> Result<RunReport, ReportingError> {
        self.report.duration_ms = elapsed(self.started);
        self.report.outcome = match (outcome, self.observed_outcome) {
            (ReportOutcome::Failed, _) | (_, Some(ReportOutcome::Failed)) => ReportOutcome::Failed,
            (ReportOutcome::Incomplete, _) | (_, Some(ReportOutcome::Incomplete)) => {
                ReportOutcome::Incomplete
            }
            _ => outcome,
        };
        if self
            .report
            .nodes
            .iter()
            .any(|node| matches!(node.status, NodeStatus::Failed | NodeStatus::Internal))
        {
            self.report.outcome = ReportOutcome::Failed;
        } else if self.report.outcome == ReportOutcome::Succeeded
            && (self.observed_outcome.is_none()
                || self.report.nodes.iter().any(|node| {
                    matches!(node.status, NodeStatus::NotStarted | NodeStatus::Running)
                }))
        {
            self.report.outcome = ReportOutcome::Incomplete;
        }
        if let Err(source) = self.events.flush() {
            self.errors.push(io_error(
                "flush event report",
                self.options.output_dir.join("events.jsonl"),
                source,
            ));
        }
        if self.options.live_log {
            if let Err(source) = self.log.flush() {
                self.errors.push(io_error(
                    "flush live report",
                    PathBuf::from("<live log>"),
                    source,
                ));
            }
        }
        if let Err(error) =
            write_json_new(&self.options.output_dir.join("report.json"), &self.report)
        {
            self.errors.push(error);
        }
        let summary = markdown(&self.report);
        if let Err(error) = write_new(
            &self.options.output_dir.join("summary.md"),
            summary.as_bytes(),
        ) {
            self.errors.push(error);
        }
        if let Some(path) = &self.options.summary_path {
            if let Err(error) = append_summary(path, summary.as_bytes()) {
                self.errors.push(error);
            }
        }
        match self.errors.len() {
            0 => Ok(self.report),
            1 => Err(self.errors.remove(0)),
            _ => Err(ReportingError::Multiple {
                errors: self.errors,
            }),
        }
    }

    fn node_mut(&mut self, node: NodeId) -> Option<&mut NodeReport> {
        if !self.report.nodes.iter().any(|row| row.id == node) {
            self.errors.push(ReportingError::UnknownNode { node });
            return None;
        }
        self.report.nodes.iter_mut().find(|row| row.id == node)
    }

    fn emit(&mut self, event: ReportEvent) {
        let path = self.options.output_dir.join("events.jsonl");
        if let Err(source) = serde_json::to_writer(&mut self.events, &event) {
            self.errors.push(ReportingError::Json {
                path: path.clone(),
                source,
            });
        } else if let Err(source) = self
            .events
            .write_all(b"\n")
            .and_then(|()| self.events.flush())
        {
            self.errors
                .push(io_error("write event report", path, source));
        }
        if self.options.live_log {
            let is_error = matches!(
                event.status,
                Some(NodeStatus::Failed | NodeStatus::Internal)
            );
            let command = if is_error { "error" } else { "notice" };
            let message = if let Some(id) = event.node_id {
                let name = self
                    .report
                    .nodes
                    .iter()
                    .find(|row| row.id == id)
                    .map(|row| row.name.as_str())
                    .unwrap_or("unknown");
                format!(
                    "{} node {} ({name}): {}{}",
                    self.report.plan_name,
                    id.index(),
                    event.status.map(NodeStatus::label).unwrap_or("progress"),
                    event
                        .message
                        .as_ref()
                        .map(|message| format!(" — {message}"))
                        .unwrap_or_default()
                )
            } else {
                format!(
                    "{}: {}",
                    self.report.plan_name,
                    event.message.as_deref().unwrap_or("progress")
                )
            };
            if let Err(source) = writeln!(
                self.log,
                "::{command} title=rehearse::{}",
                escape_command_data(&message)
            )
            .and_then(|()| self.log.flush())
            {
                self.errors.push(io_error(
                    "write live report",
                    PathBuf::from("<live log>"),
                    source,
                ));
            }
        }
    }
}

impl<E: fmt::Display> ProgressListener<E> for GitHubReporter {
    fn on_event(&mut self, event: ProgressEvent<'_, E>) {
        self.sequence += 1;
        let (kind, mode, node_id, status, message, missing_dependencies) = match event {
            ProgressEvent::PlanStarted {
                mode, plan_name, ..
            } => {
                if self.report.mode.is_some() {
                    self.errors.push(ReportingError::InvalidTraversal {
                        reason: "one reporter can observe only one traversal",
                    });
                }
                if plan_name != self.report.plan_name {
                    self.errors.push(ReportingError::InvalidTraversal {
                        reason: "plan name differs from the supplied graph",
                    });
                }
                self.report.mode = Some(mode.into());
                (
                    EventKind::PlanStarted,
                    mode,
                    None,
                    None,
                    Some("Plan started".to_owned()),
                    vec![],
                )
            }
            ProgressEvent::NodeStarted { mode, node } => {
                self.starts.insert(node.node(), Instant::now());
                if let Some(row) = self.node_mut(node.node()) {
                    row.status = NodeStatus::Running;
                }
                (
                    EventKind::NodeStarted,
                    mode,
                    Some(node.node()),
                    Some(NodeStatus::Running),
                    Some("started".to_owned()),
                    vec![],
                )
            }
            ProgressEvent::NodeFinished {
                mode,
                node,
                outcome,
            }
            | ProgressEvent::NodeDescribed {
                mode,
                node,
                outcome,
            } => {
                let described = matches!(outcome, ProgressOutcome::Described { .. });
                let (status, message, missing) = present_outcome(outcome);
                let duration = self.starts.remove(&node.node()).map(elapsed);
                if let Some(row) = self.node_mut(node.node()) {
                    row.status = status;
                    row.duration_ms = duration;
                    row.message = message.clone();
                    row.missing_dependencies = missing.clone();
                }
                (
                    if described {
                        EventKind::NodeDescribed
                    } else {
                        EventKind::NodeFinished
                    },
                    mode,
                    Some(node.node()),
                    Some(status),
                    message,
                    missing,
                )
            }
            ProgressEvent::PlanFinished { mode, outcome, .. } => {
                let outcome = match outcome {
                    ProgressPlanOutcome::Complete => ReportOutcome::Succeeded,
                    ProgressPlanOutcome::Incomplete => ReportOutcome::Incomplete,
                    ProgressPlanOutcome::Failed => ReportOutcome::Failed,
                };
                self.observed_outcome = Some(outcome);
                (
                    EventKind::PlanFinished,
                    mode,
                    None,
                    None,
                    Some(format!("Plan finished: {outcome:?}")),
                    vec![],
                )
            }
        };
        self.emit(ReportEvent {
            schema_version: 1,
            sequence: self.sequence,
            elapsed_ms: elapsed(self.started),
            plan_name: self.report.plan_name.clone(),
            plan_revision: self.report.plan_revision.clone(),
            kind,
            mode: mode.into(),
            node_id,
            status,
            message,
            missing_dependencies,
        });
    }
}

fn present_outcome<E: fmt::Display>(
    outcome: ProgressOutcome<'_, E>,
) -> (NodeStatus, Option<String>, Vec<NodeId>) {
    match outcome {
        ProgressOutcome::Described { dry_run_action } => (
            NodeStatus::Described,
            dry_run_action.map(|action| format!("dry-run action: {action:?}")),
            vec![],
        ),
        ProgressOutcome::Executed => (NodeStatus::Executed, None, vec![]),
        ProgressOutcome::Skipped { reason } => {
            (NodeStatus::Skipped, Some(reason.to_owned()), vec![])
        }
        ProgressOutcome::Denied { reason } => (NodeStatus::Denied, Some(reason.to_owned()), vec![]),
        ProgressOutcome::Blocked {
            missing_dependencies,
        } => (
            NodeStatus::Blocked,
            Some("unavailable value inputs".to_owned()),
            missing_dependencies.to_vec(),
        ),
        ProgressOutcome::UnavailableDependencies {
            missing_dependencies,
        } => (
            NodeStatus::Internal,
            Some("unavailable value inputs during execute".to_owned()),
            missing_dependencies.to_vec(),
        ),
        ProgressOutcome::Failed { error } => (NodeStatus::Failed, Some(error.to_string()), vec![]),
        ProgressOutcome::Internal { error } => {
            (NodeStatus::Internal, Some(error.to_string()), vec![])
        }
    }
}

fn elapsed(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn escape_command_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_markdown(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\n' | '\r' => output.push_str("<br>"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.'
            | '!' | '|' => {
                output.push('\\');
                output.push(ch);
            }
            _ => output.push(ch),
        }
    }
    output
}

fn markdown(report: &RunReport) -> String {
    use fmt::Write as _;
    let mut output = String::from("\n## Rehearse workflow report\n\n");
    writeln!(output, "**Plan:** {}  ", escape_markdown(&report.plan_name)).unwrap();
    writeln!(output, "**Outcome:** {:?}  ", report.outcome).unwrap();
    let mode = match report.mode {
        Some(ReportMode::Describe) => "Describe",
        Some(ReportMode::DryRun) => "Dry-run",
        Some(ReportMode::Execute) => "Execute",
        None => "Not started",
    };
    writeln!(output, "**Mode:** {mode}  ").unwrap();
    writeln!(output, "**Duration:** {} ms\n", report.duration_ms).unwrap();
    if let Some(revision) = &report.plan_revision {
        writeln!(output, "**Plan revision:** {}\n", escape_markdown(revision)).unwrap();
    }
    for (key, value) in &report.metadata {
        writeln!(
            output,
            "**{}:** {}  ",
            escape_markdown(key),
            escape_markdown(value)
        )
        .unwrap();
    }
    output.push_str("\n| Node | Operation | Impact | Outcome | Duration | Details |\n| --- | --- | --- | --- | --- | --- |\n");
    for row in &report.nodes {
        let mut details = row
            .message
            .as_deref()
            .map(escape_markdown)
            .unwrap_or_default();
        if !row.missing_dependencies.is_empty() {
            if !details.is_empty() {
                details.push_str("; ");
            }
            details.push_str("inputs: ");
            details.push_str(
                &row.missing_dependencies
                    .iter()
                    .map(|node| node.index().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        writeln!(
            output,
            "| {} | {} | {} | {} | {} | {} |",
            row.id.index(),
            escape_markdown(&row.label),
            row.impact,
            row.status.label(),
            row.duration_ms
                .map(|ms| format!("{ms} ms"))
                .unwrap_or_else(|| "—".to_owned()),
            details
        )
        .unwrap();
    }
    output.push_str("\nArtifacts: graph.json, events.jsonl, report.json, summary.md.\n");
    output
}

fn io_error(action: &'static str, path: PathBuf, source: io::Error) -> ReportingError {
    ReportingError::Io {
        action,
        path,
        source,
    }
}

fn inspect_components(path: &Path) -> Result<(), ReportingError> {
    let mut current = PathBuf::new();
    for part in path.components() {
        if part == Component::ParentDir {
            return Err(ReportingError::InvalidDirectory {
                path: path.to_owned(),
                reason: "parent traversal is not supported",
            });
        }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ReportingError::InvalidDirectory {
                    path: current,
                    reason: "symlink paths are not supported",
                })
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error("inspect report path", current, source)),
        }
    }
    Ok(())
}

// Canonicalize the nearest existing ancestor without creating the requested
// destination. This compares relative/absolute spellings before report writes.
fn resolved_path(path: &Path) -> Result<PathBuf, ReportingError> {
    inspect_components(path)?;
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| io_error("resolve report path", path.to_owned(), source))?
            .join(path)
    };
    let mut existing = absolute.clone();
    let mut tail = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            return Err(ReportingError::InvalidDirectory {
                path: absolute,
                reason: "path has no existing ancestor",
            });
        };
        tail.push(name.to_owned());
        existing.pop();
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|source| io_error("resolve report path", existing, source))?;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn prepare_directory(path: &Path) -> Result<(), ReportingError> {
    if path.as_os_str().is_empty() {
        return Err(ReportingError::InvalidDirectory {
            path: path.to_owned(),
            reason: "directory path must not be empty",
        });
    }
    inspect_components(path)?;
    if path.exists() {
        if !path.is_dir() {
            return Err(ReportingError::InvalidDirectory {
                path: path.to_owned(),
                reason: "path is not a directory",
            });
        }
        if fs::read_dir(path)
            .map_err(|source| io_error("read report directory", path.to_owned(), source))?
            .next()
            .is_some()
        {
            return Err(ReportingError::InvalidDirectory {
                path: path.to_owned(),
                reason: "directory must be new or empty",
            });
        }
    } else {
        fs::create_dir_all(path)
            .map_err(|source| io_error("create report directory", path.to_owned(), source))?;
    }
    Ok(())
}

fn create_new(path: &Path) -> Result<File, ReportingError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| io_error("create report file", path.to_owned(), source))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ReportingError> {
    create_new(path)?
        .write_all(bytes)
        .map_err(|source| io_error("write report file", path.to_owned(), source))
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), ReportingError> {
    let mut file = create_new(path)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|source| ReportingError::Json {
        path: path.to_owned(),
        source,
    })?;
    file.write_all(b"\n")
        .map_err(|source| io_error("write report file", path.to_owned(), source))
}

fn append_summary(path: &Path, bytes: &[u8]) -> Result<(), ReportingError> {
    inspect_components(path)?;
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|source| io_error("append GitHub step summary", path.to_owned(), source))
}

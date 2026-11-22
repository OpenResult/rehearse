//! In-memory server orchestration around rehearse's opt-in concurrent runner.
//!
//! HTTP connections only observe a run. Dropping a subscriber never drops its
//! execution task, decision sender, or fixture evidence.

use super::model::{
    ApprovalSnapshot, ApprovalStatus, ApprovalSubject, ControlError, CreateRun, Decision,
    DecisionRequest, EventKind, GateError, NodeSnapshot, NodeStatus, OwnedEvent, ProductionPermit,
    PublicError, Replay, RunMode, RunSnapshot, RunStatus,
};
use super::workflow::{self, RunContext, WorkflowError};
use crate::lab;
use crate::model::{LabError, Stack, Summary};
use rehearse::{
    ConcurrentExecuteError, DryRunStatus, ExecuteError, ProgressEvent, ProgressListener,
    ProgressOutcome, ProgressPlanOutcome,
};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

const ROOT_MARKER: &str = ".rehearse-workflow-server";
const ROOT_MARKER_CONTENTS: &str = "rehearse local workflow server v1\n";
const ENVIRONMENTS: [&str; 3] = ["dev", "staging", "production"];

pub struct ServerConfig {
    pub root: PathBuf,
    pub approval_timeout: Duration,
    pub step_delay: Duration,
    pub max_runs: usize,
    pub event_capacity: usize,
}

fn require_directory(path: &Path) -> Result<(), LabError> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(LabError::invalid(
            path,
            "parent traversal is not allowed in a server lab path",
        ));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| LabError::io("inspect directory", path, error))?;
    if !metadata.is_dir() {
        return Err(LabError::invalid(
            path,
            "expected a directory, not a symlink or file",
        ));
    }
    Ok(())
}

fn create_owned_directory(path: &Path) -> Result<(), LabError> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| LabError::io("create directory", path, error))
        }
        Err(error) => Err(LabError::io("inspect directory", path, error)),
    }
}

fn prepare_root(root: &Path) -> Result<(), LabError> {
    let parent = root
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    require_directory(parent)?;
    create_owned_directory(root)?;
    let marker = root.join(ROOT_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if !metadata.is_file()
                || fs::read_to_string(&marker)
                    .map_err(|error| LabError::io("read server marker", &marker, error))?
                    != ROOT_MARKER_CONTENTS
            {
                return Err(LabError::invalid(
                    &marker,
                    "not an owned workflow server directory",
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut entries = fs::read_dir(root)
                .map_err(|error| LabError::io("list server directory", root, error))?;
            if entries.next().is_some() {
                return Err(LabError::invalid(
                    root,
                    "refusing to adopt a nonempty unmarked directory",
                ));
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(|error| LabError::io("create server marker", &marker, error))?;
            file.write_all(ROOT_MARKER_CONTENTS.as_bytes())
                .map_err(|error| LabError::io("write server marker", &marker, error))?;
        }
        Err(error) => return Err(LabError::io("inspect server marker", &marker, error)),
    }
    create_owned_directory(&root.join("runs"))?;
    let preview = root.join("preview");
    create_owned_directory(&preview)?;
    for environment in ENVIRONMENTS {
        let directory = preview.join(environment);
        match fs::symlink_metadata(&directory) {
            Ok(_) => {
                require_directory(&directory)?;
                for stack in [Stack::Rust, Stack::SpringBoot] {
                    let lab = lab::Lab {
                        root: directory.clone(),
                        stack,
                        revision: "seed".to_owned(),
                        fail_at: None,
                    };
                    lab.authenticate()?;
                    lab.read_tested_artifact()?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                lab::initialize(&directory)?
            }
            Err(error) => return Err(LabError::io("inspect preview fixtures", directory, error)),
        }
    }
    Ok(())
}

fn prepare_workspace(root: &Path, id: &str) -> Result<PathBuf, LabError> {
    require_directory(root)?;
    let runs = root.join("runs");
    require_directory(&runs)?;
    let workspace = runs.join(id);
    fs::create_dir(&workspace)
        .map_err(|error| LabError::io("create run workspace", &workspace, error))?;
    for environment in ENVIRONMENTS {
        lab::initialize(&workspace.join(environment))?;
    }
    Ok(workspace)
}

pub struct ServerState {
    config: ServerConfig,
    runs: Mutex<BTreeMap<String, Arc<Run>>>,
}

pub struct Run {
    id: String,
    inner: Mutex<RunInner>,
    changed: watch::Sender<u64>,
    approval_timeout: Duration,
    step_delay: Duration,
    event_capacity: usize,
}

struct RunInner {
    snapshot: RunSnapshot,
    events: VecDeque<OwnedEvent>,
    decision: Option<oneshot::Sender<Decision>>,
    approval_deadline: Option<Instant>,
    cancel_requested: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("local run state mutex poisoned")
}

fn approval_node_id(snapshot: &RunSnapshot) -> Option<usize> {
    snapshot
        .graph
        .nodes()
        .iter()
        .find(|node| {
            node.annotations()
                .get("kind")
                .is_some_and(|kind| kind == "approval")
        })
        .map(|node| node.id().index())
}

impl ServerState {
    pub fn new(config: ServerConfig) -> Result<Arc<Self>, LabError> {
        if config.max_runs == 0 || config.event_capacity == 0 || config.approval_timeout.is_zero() {
            return Err(LabError::invalid(
                &config.root,
                "run/event limits and approval timeout must be positive",
            ));
        }
        prepare_root(&config.root)?;
        Ok(Arc::new(Self {
            config,
            runs: Mutex::new(BTreeMap::new()),
        }))
    }

    pub fn create_run(self: &Arc<Self>, input: CreateRun) -> Result<RunSnapshot, ControlError> {
        static NEXT_RUN: AtomicU64 = AtomicU64::new(1);
        let mut runs = lock(&self.runs);
        if runs.len() >= self.config.max_runs {
            return Err(ControlError::Capacity);
        }
        let serial = NEXT_RUN.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = format!("run-{nanos:x}-{serial:x}");
        let revision = format!("release-{nanos:x}-{serial:x}");
        let workspace = match input.mode {
            RunMode::Execute => prepare_workspace(&self.config.root, &id)?,
            RunMode::DryRun => self.config.root.join("preview"),
        };
        let timestamp = now_ms();
        let graph = workflow::plan_snapshot();
        let plan_revision = graph
            .plan_revision()
            .expect("server plan declares its application revision")
            .to_owned();
        let nodes = graph
            .nodes()
            .iter()
            .map(|node| NodeSnapshot {
                id: node.id().index(),
                status: NodeStatus::Pending,
                message: None,
            })
            .collect();
        let snapshot = RunSnapshot {
            id: id.clone(),
            plan_revision,
            graph,
            stack: input.stack,
            mode: input.mode,
            revision: revision.clone(),
            status: RunStatus::Queued,
            created_at_ms: timestamp,
            updated_at_ms: timestamp,
            last_event_id: 0,
            nodes,
            approval: None,
            error: None,
            summary: None,
            workspace: workspace.display().to_string(),
        };
        let (changed, _) = watch::channel(0);
        let run = Arc::new(Run {
            id: id.clone(),
            inner: Mutex::new(RunInner {
                snapshot,
                events: VecDeque::new(),
                decision: None,
                approval_deadline: None,
                cancel_requested: false,
            }),
            changed,
            approval_timeout: self.config.approval_timeout,
            step_delay: self.config.step_delay,
            event_capacity: self.config.event_capacity,
        });
        {
            let mut inner = lock(&run.inner);
            run.push(
                &mut inner,
                EventKind::RunCreated,
                "Created local fixture run".to_owned(),
            );
        }
        let created = run.snapshot();
        runs.insert(id, Arc::clone(&run));
        // Ownership belongs to this task and the registry, never to an SSE stream.
        tokio::spawn(async move {
            let context = RunContext {
                run: Arc::clone(&run),
                workspace,
                stack: input.stack,
                revision,
                fail_at: input.fail_at,
            };
            let plan = workflow::plan();
            let mut listener = RunProgress {
                run: Arc::clone(&run),
            };
            match input.mode {
                RunMode::Execute => {
                    let result = tokio::select! {
                        biased;
                        () = run.cancelled() => None,
                        result = plan.execute_concurrent_with_listener(&context, &mut listener) => Some(result),
                    };
                    match result {
                        Some(Ok(summary)) => run.finish(RunStatus::Succeeded, Some(summary), None),
                        Some(Err(error)) => run.finish_error(error),
                        None => {
                            run.cancel();
                        }
                    }
                }
                RunMode::DryRun => {
                    let report = tokio::select! {
                        biased;
                        () = run.cancelled() => None,
                        report = plan.dry_run_concurrent_with_policy_and_listener(&context, &workflow::ServerDryRun, &mut listener) => Some(report),
                    };
                    if let Some(report) = report {
                        let status = if report.has_failures() {
                            RunStatus::Failed
                        } else if report.status() == DryRunStatus::Complete {
                            RunStatus::Succeeded
                        } else {
                            RunStatus::Incomplete
                        };
                        let error = report.has_failures().then(|| PublicError {
                            code: "dry_run_failed".to_owned(),
                            message: "An allowed fixture operation failed; independent work still completed.".to_owned(),
                        });
                        let summary = Summary {
                            workflow: "gated-release".to_owned(), stack: input.stack,
                            revision: context.revision.clone(),
                            message: format!("Dry-run: {} executed, {} skipped, {} denied, {} blocked, {} failed. No approval was requested.",
                                report.executed_count(), report.skipped_count(), report.denied_count(),
                                report.blocked_count(), report.failure_count()),
                        };
                        run.finish(status, Some(summary), error);
                    } else {
                        run.cancel();
                    }
                }
            }
        });
        Ok(created)
    }

    pub fn get_run(&self, id: &str) -> Result<Arc<Run>, ControlError> {
        lock(&self.runs)
            .get(id)
            .cloned()
            .ok_or_else(|| ControlError::NotFound("Run not found".to_owned()))
    }

    pub fn list_runs(&self) -> Vec<RunSnapshot> {
        let mut snapshots: Vec<_> = lock(&self.runs)
            .values()
            .map(|run| run.snapshot())
            .collect();
        snapshots.sort_by(|left, right| {
            right
                .created_at_ms
                .cmp(&left.created_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        snapshots
    }

    pub fn cancel_all(&self) {
        for run in lock(&self.runs).values() {
            run.cancel();
        }
    }
}

impl Run {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn snapshot(&self) -> RunSnapshot {
        lock(&self.inner).snapshot.clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    fn push(&self, inner: &mut RunInner, kind: EventKind, message: String) {
        self.push_event(inner, kind, None, message);
    }

    fn push_event(
        &self,
        inner: &mut RunInner,
        kind: EventKind,
        node_id: Option<usize>,
        message: String,
    ) {
        inner.snapshot.last_event_id += 1;
        inner.snapshot.updated_at_ms = now_ms();
        let event = OwnedEvent {
            seq: inner.snapshot.last_event_id,
            run_id: self.id.clone(),
            plan_revision: inner.snapshot.plan_revision.clone(),
            at_ms: inner.snapshot.updated_at_ms,
            kind,
            node_id,
            message,
            snapshot: inner.snapshot.clone(),
        };
        inner.events.push_back(event);
        while inner.events.len() > self.event_capacity {
            inner.events.pop_front();
        }
        self.changed.send_replace(inner.snapshot.last_event_id);
    }

    pub fn events_since(&self, after: u64) -> Replay {
        let inner = lock(&self.inner);
        let earliest = inner
            .events
            .front()
            .map(|event| event.seq)
            .unwrap_or(inner.snapshot.last_event_id + 1);
        if after.saturating_add(1) < earliest {
            Replay {
                events: Vec::new(),
                reset: Some(OwnedEvent {
                    seq: inner.snapshot.last_event_id,
                    run_id: self.id.clone(),
                    plan_revision: inner.snapshot.plan_revision.clone(),
                    at_ms: now_ms(),
                    kind: EventKind::Snapshot,
                    node_id: None,
                    message: "History window passed; restored the current snapshot".to_owned(),
                    snapshot: inner.snapshot.clone(),
                }),
            }
        } else {
            Replay {
                events: inner
                    .events
                    .iter()
                    .filter(|event| event.seq > after)
                    .cloned()
                    .collect(),
                reset: None,
            }
        }
    }

    pub async fn checkpoint(&self) -> Result<(), GateError> {
        tokio::select! {
            biased;
            () = self.cancelled() => Err(GateError::Cancelled),
            () = tokio::time::sleep(self.step_delay) => {
                if lock(&self.inner).cancel_requested { Err(GateError::Cancelled) } else { Ok(()) }
            }
        }
    }

    async fn cancelled(&self) {
        let mut changed = self.subscribe();
        loop {
            if lock(&self.inner).cancel_requested {
                return;
            }
            if changed.changed().await.is_err() {
                return;
            }
        }
    }

    pub async fn request_approval(
        &self,
        subject: ApprovalSubject,
    ) -> Result<ProductionPermit, GateError> {
        let gate_id = "production";
        let deadline = Instant::now() + self.approval_timeout;
        let receiver = {
            let mut inner = lock(&self.inner);
            if inner.cancel_requested {
                return Err(GateError::Cancelled);
            }
            let timestamp = now_ms();
            let expiry = u64::try_from(self.approval_timeout.as_millis()).unwrap_or(u64::MAX);
            inner.snapshot.approval = Some(ApprovalSnapshot {
                id: gate_id.to_owned(),
                subject: subject.clone(),
                question: "Deploy this verified artifact to production?".to_owned(),
                requested_at_ms: timestamp,
                expires_at_ms: timestamp.saturating_add(expiry),
                decision: None,
                status: ApprovalStatus::Pending,
            });
            inner.snapshot.status = RunStatus::AwaitingApproval;
            let approval_node = approval_node_id(&inner.snapshot);
            if let Some(node) = inner
                .snapshot
                .nodes
                .iter_mut()
                .find(|node| Some(node.id) == approval_node)
            {
                node.status = NodeStatus::AwaitingApproval;
                node.message =
                    Some("Waiting for the local operator's production decision".to_owned());
            }
            let (sender, receiver) = oneshot::channel();
            inner.decision = Some(sender);
            inner.approval_deadline = Some(deadline);
            self.push(
                &mut inner,
                EventKind::ApprovalRequested,
                "Production approval requested".to_owned(),
            );
            receiver
        };
        let response = tokio::select! {
            biased;
            () = self.cancelled() => return Err(GateError::Cancelled),
            response = receiver => response.ok(),
            () = tokio::time::sleep_until(deadline) => None,
        };
        // A decision and the deadline can become ready together. The serialized
        // gate state determines which transition actually won, not poll order.
        let mut inner = lock(&self.inner);
        if inner.cancel_requested {
            return Err(GateError::Cancelled);
        }
        let status = inner
            .snapshot
            .approval
            .as_ref()
            .map(|approval| approval.status);
        match (response, status) {
            (_, Some(ApprovalStatus::Approved)) | (Some(Decision::Approve), _) => {
                Ok(ProductionPermit {
                    run_id: self.id.clone(),
                    gate_id: gate_id.to_owned(),
                    subject,
                })
            }
            (_, Some(ApprovalStatus::Rejected)) | (Some(Decision::Reject), _) => {
                Err(GateError::Rejected)
            }
            _ => {
                self.expire_approval(&mut inner);
                Err(GateError::TimedOut)
            }
        }
    }

    fn expire_approval(&self, inner: &mut RunInner) {
        if let Some(approval) = &mut inner.snapshot.approval {
            if approval.status != ApprovalStatus::Pending {
                return;
            }
            approval.status = ApprovalStatus::TimedOut;
        }
        inner.decision.take();
        inner.snapshot.status = RunStatus::Running;
        self.push(
            inner,
            EventKind::ApprovalExpired,
            "Production approval expired".to_owned(),
        );
    }

    pub fn decide(
        &self,
        gate_id: &str,
        request: &DecisionRequest,
    ) -> Result<RunSnapshot, ControlError> {
        let mut inner = lock(&self.inner);
        let approval = inner.snapshot.approval.as_ref().ok_or_else(|| {
            ControlError::Conflict("No approval is pending for this run".to_owned())
        })?;
        if approval.id != gate_id {
            return Err(ControlError::NotFound("Approval gate not found".to_owned()));
        }
        if approval.subject.artifact_digest != request.artifact_digest
            || approval.subject.revision != request.revision
        {
            return Err(ControlError::Conflict(
                "Decision does not match the pending artifact and revision".to_owned(),
            ));
        }
        if let Some(decision) = approval.decision {
            if decision == request.decision {
                return Ok(inner.snapshot.clone());
            }
            return Err(ControlError::Conflict(
                "This approval has already been decided".to_owned(),
            ));
        }
        if approval.status != ApprovalStatus::Pending || inner.snapshot.status.is_terminal() {
            return Err(ControlError::Conflict(
                "This approval is no longer pending".to_owned(),
            ));
        }
        if inner
            .approval_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.expire_approval(&mut inner);
            return Err(ControlError::Conflict(
                "This approval has expired".to_owned(),
            ));
        }
        let approval = inner
            .snapshot
            .approval
            .as_mut()
            .expect("approval inspected above");
        approval.decision = Some(request.decision);
        approval.status = match request.decision {
            Decision::Approve => ApprovalStatus::Approved,
            Decision::Reject => ApprovalStatus::Rejected,
        };
        inner.snapshot.status = RunStatus::Running;
        let approval_node = approval_node_id(&inner.snapshot);
        if let Some(node) = inner
            .snapshot
            .nodes
            .iter_mut()
            .find(|node| Some(node.id) == approval_node)
        {
            node.status = NodeStatus::Running;
            node.message = Some(
                match request.decision {
                    Decision::Approve => "Approval granted",
                    Decision::Reject => "Approval rejected",
                }
                .to_owned(),
            );
        }
        let sender = inner.decision.take();
        self.push(
            &mut inner,
            EventKind::ApprovalDecided,
            match request.decision {
                Decision::Approve => "Production approval granted",
                Decision::Reject => "Production approval rejected",
            }
            .to_owned(),
        );
        if let Some(sender) = sender {
            let _ = sender.send(request.decision);
        }
        Ok(inner.snapshot.clone())
    }

    pub fn cancel(&self) -> RunSnapshot {
        let mut inner = lock(&self.inner);
        if inner.snapshot.status.is_terminal() {
            return inner.snapshot.clone();
        }
        inner.cancel_requested = true;
        inner.snapshot.status = RunStatus::Cancelled;
        inner.snapshot.error = Some(PublicError {
            code: "cancelled".to_owned(),
            message: "Run cancelled; completed fixture writes are retained.".to_owned(),
        });
        if let Some(approval) = &mut inner.snapshot.approval {
            if approval.status == ApprovalStatus::Pending {
                approval.status = ApprovalStatus::Cancelled;
            }
        }
        inner.decision.take();
        for node in &mut inner.snapshot.nodes {
            if node.status == NodeStatus::Pending {
                node.status = NodeStatus::NotStarted;
                node.message = Some("Not started because the run was cancelled".to_owned());
            } else if matches!(
                node.status,
                NodeStatus::Running | NodeStatus::AwaitingApproval
            ) {
                node.status = NodeStatus::Cancelled;
                node.message = Some("Cancelled".to_owned());
            }
        }
        self.push(
            &mut inner,
            EventKind::RunFinished,
            "Run cancelled".to_owned(),
        );
        inner.snapshot.clone()
    }

    fn finish_error(&self, error: ConcurrentExecuteError<WorkflowError>) {
        {
            let mut inner = lock(&self.inner);
            for id in error.not_started_nodes() {
                if let Some(node) = inner
                    .snapshot
                    .nodes
                    .iter_mut()
                    .find(|node| node.id == id.index())
                {
                    node.status = NodeStatus::NotStarted;
                    node.message = Some("Not started after execution failed".to_owned());
                }
            }
        }
        let (status, code) = match error.primary_error() {
            ExecuteError::Operation {
                source: WorkflowError::Gate(GateError::Rejected),
                ..
            } => (RunStatus::Rejected, "approval_rejected"),
            ExecuteError::Operation {
                source: WorkflowError::Gate(GateError::TimedOut),
                ..
            } => (RunStatus::TimedOut, "approval_timed_out"),
            ExecuteError::Operation {
                source: WorkflowError::Gate(GateError::Cancelled),
                ..
            } => (RunStatus::Cancelled, "cancelled"),
            ExecuteError::Operation { .. } => (RunStatus::Failed, "operation_failed"),
            ExecuteError::Internal(_) => (RunStatus::Failed, "invariant_error"),
        };
        self.finish(
            status,
            None,
            Some(PublicError {
                code: code.to_owned(),
                message: error.to_string(),
            }),
        );
    }

    fn finish(&self, status: RunStatus, summary: Option<Summary>, error: Option<PublicError>) {
        let mut inner = lock(&self.inner);
        if inner.snapshot.status.is_terminal() {
            return;
        }
        inner.snapshot.status = status;
        inner.snapshot.summary = summary;
        inner.snapshot.error = error;
        inner.decision.take();
        self.push(
            &mut inner,
            EventKind::RunFinished,
            format!(
                "Run finished: {}",
                match status {
                    RunStatus::Succeeded => "succeeded",
                    RunStatus::Incomplete => "dry-run incomplete",
                    RunStatus::Rejected => "rejected",
                    RunStatus::TimedOut => "approval expired",
                    RunStatus::Cancelled => "cancelled",
                    _ => "failed",
                }
            ),
        );
    }
}

struct RunProgress {
    run: Arc<Run>,
}

impl ProgressListener<WorkflowError> for RunProgress {
    fn on_event(&mut self, event: ProgressEvent<'_, WorkflowError>) {
        let mut inner = lock(&self.run.inner);
        if inner.cancel_requested || inner.snapshot.status.is_terminal() {
            return;
        }
        let node_id = match &event {
            ProgressEvent::NodeStarted { node, .. } | ProgressEvent::NodeFinished { node, .. } => {
                Some(node.node().index())
            }
            _ => None,
        };
        let (kind, message) = match event {
            ProgressEvent::PlanStarted { .. } => {
                inner.snapshot.status = RunStatus::Running;
                (
                    EventKind::PlanStarted,
                    "Fixture plan started with declared parallel branches".to_owned(),
                )
            }
            ProgressEvent::NodeStarted { node, .. } => {
                if let Some(snapshot) = inner.snapshot.nodes.get_mut(node.node().index()) {
                    snapshot.status = NodeStatus::Running;
                    snapshot.message = None;
                }
                (
                    EventKind::NodeStarted,
                    if inner.snapshot.mode == RunMode::DryRun {
                        format!("{}: checking policy and inputs", node.name())
                    } else {
                        format!("{} started", node.name())
                    },
                )
            }
            ProgressEvent::NodeFinished { node, outcome, .. } => {
                let (status, detail) = match outcome {
                    ProgressOutcome::Executed => (NodeStatus::Executed, None),
                    ProgressOutcome::Skipped { reason } => {
                        (NodeStatus::Skipped, Some(reason.to_owned()))
                    }
                    ProgressOutcome::Denied { reason } => {
                        (NodeStatus::Denied, Some(reason.to_owned()))
                    }
                    ProgressOutcome::Blocked {
                        missing_dependencies,
                    }
                    | ProgressOutcome::UnavailableDependencies {
                        missing_dependencies,
                    } => (
                        NodeStatus::Blocked,
                        Some(format!(
                            "Unavailable input node(s): {}",
                            missing_dependencies
                                .iter()
                                .map(|id| id.index().to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    ),
                    ProgressOutcome::Failed { error } => {
                        (NodeStatus::Failed, Some(error.to_string()))
                    }
                    ProgressOutcome::Internal { error } => {
                        (NodeStatus::Failed, Some(error.to_string()))
                    }
                    ProgressOutcome::Described { .. } => return,
                };
                if let Some(snapshot) = inner.snapshot.nodes.get_mut(node.node().index()) {
                    snapshot.status = status;
                    snapshot.message = detail;
                }
                let outcome = match status {
                    NodeStatus::Executed => "executed",
                    NodeStatus::Skipped => "skipped",
                    NodeStatus::Denied => "denied",
                    NodeStatus::Blocked => "blocked",
                    NodeStatus::Failed => "failed",
                    _ => "finished",
                };
                (
                    EventKind::NodeFinished,
                    format!("{} {outcome}", node.name()),
                )
            }
            ProgressEvent::PlanFinished { outcome, .. } => (
                EventKind::PlanFinished,
                match outcome {
                    ProgressPlanOutcome::Complete => "Plan completed",
                    ProgressPlanOutcome::Incomplete => "Plan has expected dry-run omissions",
                    ProgressPlanOutcome::Failed => "Plan stopped with a failure",
                }
                .to_owned(),
            ),
            ProgressEvent::NodeDescribed { .. } => return,
        };
        // Borrowed errors/names never escape the callback; only owned DTOs do.
        self.run.push_event(&mut inner, kind, node_id, message);
    }
}

use rehearse::{Impact, Input, Operation, OperationMetadata, Plan, PlanBuilder};
use rehearse_github::{
    GitHubReporter, NodeStatus, ReportEvent, ReportMode, ReportOptions, ReportOutcome,
    ReportingError, RunReport,
};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Barrier, Notify};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "rehearse-github-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn reports(&self) -> PathBuf {
        self.0.join("reports")
    }

    fn report(&self) -> RunReport {
        serde_json::from_slice(&fs::read(self.reports().join("report.json")).unwrap()).unwrap()
    }

    fn events(&self) -> Vec<ReportEvent> {
        fs::read_to_string(self.reports().join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct Failure(&'static str);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

fn pure(name: &str) -> Operation<(), u32, Failure> {
    Operation::sync(OperationMetadata::new(name, Impact::Pure), (), |_, ()| {
        Ok(7)
    })
}

#[tokio::test]
async fn errors_are_escaped_for_logs_and_markdown_and_unstarted_nodes_are_explicit() {
    let directory = TestDirectory::new();
    let mut builder = PlanBuilder::<(), Failure>::new("plan%\r\n::error::injected")
        .with_revision("build<1>|[revision]");
    builder.add(pure("first"));
    builder.add(Operation::sync(
        OperationMetadata::new("fail%\n::notice::fake", Impact::Write)
            .with_annotation("label", "<script>|[label](https://example.test)"),
        (),
        |_, ()| Err::<u32, _>(Failure("bad%\r\n::error::new command")),
    ));
    let final_value = builder.add(pure("never"));
    let plan = builder.finish(final_value);
    let log = Capture::default();
    let summary_path = directory.0.join("job-summary.md");
    fs::write(&summary_path, "Existing summary\n").unwrap();
    let mut reporter = GitHubReporter::with_writer(
        plan.describe_graph(),
        ReportOptions::new(directory.reports())
            .with_summary_path(&summary_path)
            .with_metadata("commit", "<b>|untrusted\nvalue"),
        log.clone(),
    )
    .unwrap();
    assert!(plan
        .execute_with_listener(&(), &mut reporter)
        .await
        .is_err());
    // An accidental successful caller result cannot hide a real node failure.
    let report = reporter.finish(ReportOutcome::Succeeded).unwrap();
    assert_eq!(report.outcome, ReportOutcome::Failed);
    assert_eq!(report.mode, Some(ReportMode::Execute));
    assert_eq!(
        report
            .nodes
            .iter()
            .map(|node| node.status)
            .collect::<Vec<_>>(),
        [
            NodeStatus::Executed,
            NodeStatus::Failed,
            NodeStatus::NotStarted
        ]
    );
    assert!(report.nodes[0].duration_ms.is_some());
    assert!(report.nodes[2].duration_ms.is_none());
    assert_eq!(directory.report(), report);
    let events = directory.events();
    assert!(events
        .iter()
        .enumerate()
        .all(|(index, event)| event.schema_version == 1 && event.sequence == index as u64 + 1));
    assert_eq!(events.last().unwrap().node_id, None);
    let log = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert_eq!(log.lines().count(), events.len());
    assert!(log.contains("::error title=rehearse::"));
    assert!(log.contains("%25%0D%0A::error::new command"));
    assert!(!log.contains("\r"));
    let summary = fs::read_to_string(directory.reports().join("summary.md")).unwrap();
    assert!(summary.contains("Not started"));
    assert!(summary.contains("**Mode:** Execute"));
    assert!(summary.contains("&lt;script&gt;\\|\\[label\\]"));
    assert!(!summary.contains("<script>"));
    assert!(fs::read_to_string(summary_path)
        .unwrap()
        .starts_with("Existing summary\n"));
    let graph: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.reports().join("graph.json")).unwrap()).unwrap();
    assert_eq!(graph, serde_json::to_value(plan.describe_graph()).unwrap());
}

#[tokio::test]
async fn concurrent_events_keep_observation_order_and_final_rows_keep_declaration_order() {
    struct Context {
        barrier: Barrier,
        release: Notify,
    }
    let directory = TestDirectory::new();
    let mut builder = PlanBuilder::<Context, Failure>::new("parallel");
    let output = builder.parallel("two lanes", NonZeroUsize::new(2).unwrap(), |group| {
        group.branch("failure", |branch| {
            branch.add(Operation::new(
                OperationMetadata::new("fail", Impact::Read),
                (),
                |context: &Context, ()| {
                    Box::pin(async move {
                        context.barrier.wait().await;
                        context.release.notify_one();
                        Err::<(), _>(Failure("first failure"))
                    })
                },
            ));
            branch.add(Operation::sync(
                OperationMetadata::new("unstarted in lane", Impact::Write),
                (),
                |_, ()| Ok(()),
            ))
        });
        group.branch("drained", |branch| {
            branch.add(Operation::new(
                OperationMetadata::new("finish active peer", Impact::Write),
                (),
                |context: &Context, ()| {
                    Box::pin(async move {
                        context.barrier.wait().await;
                        context.release.notified().await;
                        Ok(())
                    })
                },
            ))
        });
        group.branch("queued", |branch| {
            branch.add(Operation::sync(
                OperationMetadata::new("never launched", Impact::Write),
                (),
                |_, ()| Ok(()),
            ))
        })
    });
    let plan = builder.finish(output);
    let context = Context {
        barrier: Barrier::new(2),
        release: Notify::new(),
    };
    let mut reporter = GitHubReporter::new(
        plan.describe_graph(),
        ReportOptions::new(directory.reports()).with_live_log(false),
    )
    .unwrap();
    assert!(plan
        .execute_concurrent_with_listener(&context, &mut reporter)
        .await
        .is_err());
    let report = reporter.finish(ReportOutcome::Failed).unwrap();
    assert_eq!(
        report
            .nodes
            .iter()
            .map(|node| node.status)
            .collect::<Vec<_>>(),
        [
            NodeStatus::Failed,
            NodeStatus::NotStarted,
            NodeStatus::Executed,
            NodeStatus::NotStarted
        ]
    );
    let events = directory.events();
    let starts = events
        .iter()
        .filter(|event| event.status == Some(NodeStatus::Running))
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0].node_id.unwrap().index(), 0);
    assert_eq!(starts[1].node_id.unwrap().index(), 2);
    assert_eq!(events[events.len() - 2].status, Some(NodeStatus::Executed));
    assert!(
        events[events.len() - 2].sequence
            > events
                .iter()
                .find(|event| event.status == Some(NodeStatus::Failed))
                .unwrap()
                .sequence
    );
    assert_eq!(
        events.last().unwrap().kind,
        rehearse_github::EventKind::PlanFinished
    );
}

fn rehearsal_plan() -> Plan<(), u32, Failure> {
    let mut builder = PlanBuilder::<(), Failure>::new("rehearsal");
    let written = builder.add(Operation::sync(
        OperationMetadata::new("write", Impact::Write),
        (),
        |_, ()| panic!("dry-run must skip this body"),
    ));
    builder.add(Operation::sync(
        OperationMetadata::new("dependent read", Impact::Read),
        Input::<u32>::value(written),
        |_, value| Ok(value),
    ));
    builder.add(Operation::sync(
        OperationMetadata::new("opaque", Impact::Opaque),
        (),
        |_, ()| Err::<(), _>(Failure("opaque body must not run")),
    ));
    let result = builder.add(pure("independent read"));
    builder.finish(result)
}

#[tokio::test]
async fn dry_run_reports_policy_outcomes_and_caller_strict_failure_separately() {
    for strict in [false, true] {
        let directory = TestDirectory::new();
        let plan = rehearsal_plan();
        let mut reporter = GitHubReporter::new(
            plan.describe_graph(),
            ReportOptions::new(directory.reports()).with_live_log(false),
        )
        .unwrap();
        let rehearsal = plan.dry_run_with_listener(&(), &mut reporter).await;
        assert!(rehearsal.require_complete().is_err());
        let outcome = if strict {
            ReportOutcome::Failed
        } else {
            ReportOutcome::Incomplete
        };
        let report = reporter.finish(outcome).unwrap();
        assert_eq!(report.outcome, outcome);
        assert_eq!(report.mode, Some(ReportMode::DryRun));
        assert_eq!(
            report
                .nodes
                .iter()
                .map(|node| node.status)
                .collect::<Vec<_>>(),
            [
                NodeStatus::Skipped,
                NodeStatus::Blocked,
                NodeStatus::Denied,
                NodeStatus::Executed
            ]
        );
        assert_eq!(report.nodes[1].missing_dependencies[0].index(), 0);
        assert_eq!(
            report.nodes[1].message.as_deref(),
            Some("unavailable value inputs")
        );
    }
}

struct BrokenLog;
impl Write for BrokenLog {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("log unavailable"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn reporting_io_errors_do_not_change_execution_and_artifacts_are_still_attempted() {
    let directory = TestDirectory::new();
    let mut builder = PlanBuilder::<(), Failure>::new("logging failed");
    let result = builder.add(pure("real operation"));
    let plan = builder.finish(result);
    let mut reporter = GitHubReporter::with_writer(
        plan.describe_graph(),
        ReportOptions::new(directory.reports()).with_summary_path(&directory.0),
        BrokenLog,
    )
    .unwrap();
    assert_eq!(
        plan.execute_with_listener(&(), &mut reporter)
            .await
            .unwrap(),
        7
    );
    let error = reporter.finish(ReportOutcome::Succeeded).unwrap_err();
    assert!(matches!(error, ReportingError::Multiple { .. }));
    assert!(directory.reports().join("summary.md").is_file());
    assert_eq!(directory.report().outcome, ReportOutcome::Succeeded);
    assert_eq!(directory.report().nodes[0].status, NodeStatus::Executed);
}

#[test]
fn report_output_refuses_existing_content_and_does_not_overwrite_late_files() {
    let directory = TestDirectory::new();
    let plan = rehearsal_plan();
    assert!(matches!(
        GitHubReporter::new(plan.describe_graph(), ReportOptions::new("")),
        Err(ReportingError::InvalidDirectory { .. })
    ));
    fs::create_dir(directory.reports()).unwrap();
    let owned = directory.reports().join("keep.txt");
    fs::write(&owned, "preserve").unwrap();
    assert!(matches!(
        GitHubReporter::new(
            plan.describe_graph(),
            ReportOptions::new(directory.reports())
        ),
        Err(ReportingError::InvalidDirectory { .. })
    ));
    assert_eq!(fs::read_to_string(&owned).unwrap(), "preserve");
    let output = directory.0.join("fresh");
    let reporter = GitHubReporter::new(
        plan.describe_graph(),
        ReportOptions::new(&output).with_live_log(false),
    )
    .unwrap();
    fs::write(output.join("report.json"), "late existing content").unwrap();
    assert!(reporter.finish(ReportOutcome::Failed).is_err());
    assert_eq!(
        fs::read_to_string(output.join("report.json")).unwrap(),
        "late existing content"
    );
}

#[test]
fn summary_cannot_overlap_a_report_artifact_and_rejection_precedes_output_creation() {
    for file in [
        "graph.json",
        "events.jsonl",
        "report.json",
        "summary.md",
        "nested/job.md",
    ] {
        let directory = TestDirectory::new();
        let output = directory.reports();
        let reporter = GitHubReporter::new(
            rehearsal_plan().describe_graph(),
            ReportOptions::new(&output).with_summary_path(output.join(file)),
        );
        assert!(matches!(
            reporter,
            Err(ReportingError::SummaryInsideOutput { .. })
        ));
        assert!(
            !output.exists(),
            "invalid configuration must not create output"
        );
    }
    // Differently spelled paths are compared as locations, not as raw strings.
    let directory = TestDirectory::new();
    let output = directory.reports();
    assert!(matches!(
        GitHubReporter::new(
            rehearsal_plan().describe_graph(),
            ReportOptions::new(&output).with_summary_path(output.join(".").join("report.json"))
        ),
        Err(ReportingError::SummaryInsideOutput { .. })
    ));
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn symlink_output_and_summary_paths_are_rejected_without_touching_their_targets() {
    use std::os::unix::fs::symlink;
    let directory = TestDirectory::new();
    let external = directory.0.join("external");
    fs::create_dir(&external).unwrap();
    let linked = directory.0.join("linked");
    symlink(&external, &linked).unwrap();
    assert!(GitHubReporter::new(
        rehearsal_plan().describe_graph(),
        ReportOptions::new(linked.join("child"))
    )
    .is_err());
    assert_eq!(fs::read_dir(&external).unwrap().count(), 0);
    let target = directory.0.join("target.md");
    fs::write(&target, "preserve").unwrap();
    let summary_link = directory.0.join("summary-link.md");
    symlink(&target, &summary_link).unwrap();
    let reporter = GitHubReporter::new(
        rehearsal_plan().describe_graph(),
        ReportOptions::new(directory.reports())
            .with_summary_path(summary_link)
            .with_live_log(false),
    );
    assert!(matches!(
        reporter,
        Err(ReportingError::InvalidDirectory { .. })
    ));
    assert!(!directory.reports().exists());
    assert_eq!(fs::read_to_string(target).unwrap(), "preserve");
}

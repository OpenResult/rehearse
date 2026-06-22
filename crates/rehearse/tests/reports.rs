mod common;

use common::{fail0, op0, TestContext, TestError};
use rehearse::{DryRunStatus, Impact, NodeOutcome, PlanBuilder};

#[tokio::test]
async fn every_node_has_one_outcome_and_retains_metadata() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("metadata");

    builder.add(op0("login", Impact::Session, ()));
    let read = builder.add(op0("read", Impact::Read, 1_u32));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;
    let nodes = report.iter().collect::<Vec<_>>();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].node().index(), 0);
    assert_eq!(nodes[0].name(), "login");
    assert_eq!(nodes[0].impact(), Impact::Session);
    assert_eq!(nodes[0].outcome(), &NodeOutcome::Executed);
    assert_eq!(nodes[1].node().index(), 1);
    assert_eq!(nodes[1].name(), "read");
    assert_eq!(nodes[1].impact(), Impact::Read);
    assert_eq!(nodes[1].outcome(), &NodeOutcome::Executed);
}

#[tokio::test]
async fn counters_predicates_and_status_are_correct_for_incomplete_report() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("counts");

    let read = builder.add(op0("read", Impact::Read, ()));
    builder.add(op0("write", Impact::Write, ()));
    builder.add(op0("opaque", Impact::Opaque, ()));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;

    assert_eq!(report.len(), 3);
    assert_eq!(report.executed_count(), 1);
    assert_eq!(report.skipped_count(), 1);
    assert_eq!(report.denied_count(), 1);
    assert_eq!(report.blocked_count(), 0);
    assert_eq!(report.failure_count(), 0);
    assert!(!report.has_failures());
    assert!(!report.has_blocked());
    assert!(report.has_denied());
    assert_eq!(report.status(), DryRunStatus::Incomplete);
    assert!(report.require_no_failures().is_ok());
}

#[tokio::test]
async fn require_no_failures_rejects_executed_operation_failures() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("failures");

    let failing = builder.add(fail0::<u32>("read", Impact::Read, "read failed"));
    let plan = builder.finish(failing);

    let report = plan.dry_run(&context).await;
    let error = report
        .require_no_failures()
        .expect_err("failures are rejected");

    assert_eq!(report.status(), DryRunStatus::Failed);
    assert_eq!(error.failure_count(), 1);
}

#[tokio::test]
async fn display_output_is_deterministic() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("display");

    let read = builder.add(op0("read", Impact::Read, ()));
    builder.add(op0("write", Impact::Write, ()));
    builder.add(fail0::<u32>(
        "failed_read",
        Impact::Read,
        "network unavailable",
    ));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;

    assert_eq!(
        report.to_string(),
        "\
[ok] read executed
[skip] write skipped: write operation
[fail] failed_read failed: network unavailable

Dry-run failed: 1 executed, 1 skipped, 0 denied, 0 blocked, 1 failed."
    );
}

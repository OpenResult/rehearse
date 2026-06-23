mod common;

use common::{fail0, op0, panic1, TestContext, TestError};
use rehearse::{
    DryRunStatus, Impact, Input, NodeOutcome, Operation, OperationMetadata, PlanBuilder,
};

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

    let incomplete = report
        .require_complete()
        .expect_err("skips and denials are incomplete");
    assert_eq!(incomplete.status(), DryRunStatus::Incomplete);
    assert_eq!(incomplete.executed_count(), 1);
    assert_eq!(incomplete.skipped_count(), 1);
    assert_eq!(incomplete.denied_count(), 1);
    assert_eq!(incomplete.blocked_count(), 0);
    assert_eq!(incomplete.failure_count(), 0);
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
    assert_eq!(
        report
            .require_complete()
            .expect_err("failures are incomplete")
            .status(),
        DryRunStatus::Failed
    );
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

#[tokio::test]
async fn display_names_missing_dependencies_when_available() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("blocked-display");

    let write = builder.add(op0("write_config", Impact::Write, 1_u32));
    let read = builder.add(panic1::<u32, ()>(
        "verify_config",
        Impact::Read,
        Input::value(write),
    ));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;

    assert_eq!(
        report.to_string(),
        "\
[skip] write_config skipped: write operation
[block] verify_config blocked: missing #0 (write_config)

Dry-run incomplete: 0 executed, 1 skipped, 0 denied, 1 blocked, 0 failed."
    );
}

#[cfg(feature = "serde")]
#[tokio::test]
async fn reports_serialize_to_json() {
    let mut builder = PlanBuilder::<(), String>::new("json-report");
    let read = builder.add(Operation::sync(
        OperationMetadata::new("read", Impact::Read),
        (),
        |_, ()| Ok(1_u32),
    ));
    let plan = builder.finish(read);

    let report = plan.dry_run(&()).await;
    let json = serde_json::to_string(&report).expect("report serializes");

    assert!(json.contains("\"plan_name\":\"json-report\""));
    assert!(json.contains("\"outcome\":\"executed\""));
}

mod common;

use common::{fail0, op0, op1, op2, panic0, panic1, TestContext, TestError};
use rehearse::{DryRunStatus, Impact, Input, NodeOutcome, PlanBuilder};

#[tokio::test]
async fn safe_dry_run_runs_pure_session_and_read_operations() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("safe-runs");

    builder.add(op0("pure", Impact::Pure, ()));
    builder.add(op0("session", Impact::Session, ()));
    let read = builder.add(op0("read", Impact::Read, "value"));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), vec!["pure", "session", "read"]);
    assert_eq!(report.executed_count(), 3);
    assert_eq!(report.status(), DryRunStatus::Complete);
}

#[tokio::test]
async fn safe_dry_run_skips_write_and_delete_without_invoking_bodies() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("safe-skips");

    let read = builder.add(op0("read", Impact::Read, ()));
    builder.add(panic0::<()>("write", Impact::Write));
    builder.add(panic0::<()>("delete", Impact::Delete));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), vec!["read"]);
    assert_eq!(report.skipped_count(), 2);
    assert_eq!(report.status(), DryRunStatus::Incomplete);
    assert!(matches!(
        report.iter().nth(1).expect("write report").outcome(),
        NodeOutcome::Skipped { reason } if reason == "write operation"
    ));
    assert!(matches!(
        report.iter().nth(2).expect("delete report").outcome(),
        NodeOutcome::Skipped { reason } if reason == "delete operation"
    ));
}

#[tokio::test]
async fn safe_dry_run_denies_opaque_without_invoking_body() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("safe-deny");

    let read = builder.add(op0("read", Impact::Read, ()));
    builder.add(panic0::<()>("opaque", Impact::Opaque));
    let plan = builder.finish(read);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), vec!["read"]);
    assert_eq!(report.denied_count(), 1);
    assert!(report.has_denied());
    assert!(matches!(
        report.iter().nth(1).expect("opaque report").outcome(),
        NodeOutcome::Denied { reason } if reason == "opaque operation"
    ));
}

#[tokio::test]
async fn read_depending_on_skipped_write_is_blocked_but_independent_read_runs() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("read-after-write");

    let login = builder.add(op0("login", Impact::Session, ()));
    let resource = builder.add(op1(
        "apply_changes",
        Impact::Write,
        Input::value(login),
        |_| 42_u32,
    ));
    let quota = builder.add(op0("read_account_quota", Impact::Read, 100_u32));
    builder.add(panic1::<u32, ()>(
        "verify_deployment",
        Impact::Read,
        Input::value(resource),
    ));
    let plan = builder.finish(quota);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), vec!["login", "read_account_quota"]);
    assert!(matches!(
        report
            .iter()
            .find(|node| node.name() == "apply_changes")
            .expect("apply report")
            .outcome(),
        NodeOutcome::Skipped { reason } if reason == "write operation"
    ));
    assert!(matches!(
        report
            .iter()
            .find(|node| node.name() == "verify_deployment")
            .expect("verify report")
            .outcome(),
        NodeOutcome::Blocked {
            missing_dependencies
        } if missing_dependencies == &[resource.node()]
    ));
}

#[tokio::test]
async fn failed_read_blocks_dependents_but_not_independent_later_work() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("failed-read");

    let failed_read = builder.add(fail0::<u32>("read_current", Impact::Read, "read failed"));
    builder.add(panic1::<u32, ()>(
        "calculate_changes",
        Impact::Pure,
        Input::value(failed_read),
    ));
    let independent = builder.add(op0("read_account_quota", Impact::Read, 10_u32));
    let plan = builder.finish(independent);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), vec!["read_current", "read_account_quota"]);
    assert_eq!(report.failure_count(), 1);
    assert_eq!(report.blocked_count(), 1);
    assert_eq!(report.status(), DryRunStatus::Failed);
    assert!(matches!(
        report
            .iter()
            .find(|node| node.name() == "calculate_changes")
            .expect("calculate report")
            .outcome(),
        NodeOutcome::Blocked {
            missing_dependencies
        } if missing_dependencies == &[failed_read.node()]
    ));
}

#[tokio::test]
async fn blocked_node_reports_all_missing_value_dependencies() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("multiple-missing");

    let first = builder.add(panic0::<u32>("write_one", Impact::Write));
    let second = builder.add(panic0::<u32>("write_two", Impact::Write));
    let combined = builder.add(op2(
        "inspect_both",
        Impact::Read,
        (Input::value(first), Input::value(second)),
        |a, b| a + b,
    ));
    let plan = builder.finish(combined);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), Vec::<&str>::new());
    assert!(matches!(
        report
            .iter()
            .find(|node| node.name() == "inspect_both")
            .expect("inspect report")
            .outcome(),
        NodeOutcome::Blocked {
            missing_dependencies
        } if missing_dependencies == &[first.node(), second.node()]
    ));
}

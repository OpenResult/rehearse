mod common;

use common::{op0, panic0, TestContext, TestError};
use rehearse::{DryRunAction, DryRunPolicy, Impact, OperationMetadata, PlanBuilder};

#[test]
fn describe_does_not_invoke_operation_bodies() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("describe_inert");

    builder.add(panic0::<()>("write", Impact::Write));
    let read = builder.add(panic0::<u32>("read", Impact::Read));
    let plan = builder.finish(read);

    let description = plan.describe();

    assert_eq!(context.calls(), Vec::<&str>::new());
    assert_eq!(description.len(), 2);
}

#[test]
fn describe_rows_match_plan_order_and_expose_metadata() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("ordered");

    let login = builder.add(op0("login", Impact::Session, ()));
    let write = builder.add(op0("apply_changes", Impact::Write, ()));
    let opaque = builder.add(op0("shell_command", Impact::Opaque, ()));
    let plan = builder.finish(opaque);

    let rows = plan.describe().iter().cloned().collect::<Vec<_>>();

    assert_eq!(rows[0].node(), login.node());
    assert_eq!(rows[0].position(), 1);
    assert_eq!(rows[0].name(), "login");
    assert_eq!(rows[0].impact(), Impact::Session);
    assert_eq!(rows[0].dry_run_action(), DryRunAction::Run);

    assert_eq!(rows[1].node(), write.node());
    assert_eq!(rows[1].position(), 2);
    assert_eq!(rows[1].name(), "apply_changes");
    assert_eq!(rows[1].impact(), Impact::Write);
    assert_eq!(rows[1].dry_run_action(), DryRunAction::Skip);

    assert_eq!(rows[2].node(), opaque.node());
    assert_eq!(rows[2].position(), 3);
    assert_eq!(rows[2].name(), "shell_command");
    assert_eq!(rows[2].impact(), Impact::Opaque);
    assert_eq!(rows[2].dry_run_action(), DryRunAction::Deny);
}

#[test]
fn describe_with_policy_reflects_custom_policy() {
    struct RunWrites;

    impl DryRunPolicy for RunWrites {
        fn action(&self, metadata: &OperationMetadata) -> DryRunAction {
            match metadata.impact() {
                Impact::Write => DryRunAction::Run,
                Impact::Delete => DryRunAction::Deny,
                _ => DryRunAction::Skip,
            }
        }
    }

    let mut builder = PlanBuilder::<TestContext, TestError>::new("custom");

    builder.add(op0("read", Impact::Read, ()));
    builder.add(op0("write", Impact::Write, ()));
    let delete = builder.add(op0("delete", Impact::Delete, ()));
    let plan = builder.finish(delete);

    let actions = plan
        .describe_with_policy(&RunWrites)
        .iter()
        .map(|row| row.dry_run_action())
        .collect::<Vec<_>>();

    assert_eq!(
        actions,
        vec![DryRunAction::Skip, DryRunAction::Run, DryRunAction::Deny]
    );
}

#[test]
fn display_output_is_deterministic() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("deploy");

    builder.add(op0("login", Impact::Session, ()));
    builder.add(op0("read_current", Impact::Read, ()));
    builder.add(op0("calculate_changes", Impact::Pure, ()));
    builder.add(op0("apply_changes", Impact::Write, ()));
    let delete = builder.add(op0("delete_old_releases", Impact::Delete, ()));
    let plan = builder.finish(delete);

    assert_eq!(
        plan.describe().to_string(),
        "\
deploy

  1  login                session  run
  2  read_current         read     run
  3  calculate_changes    pure     run
  4  apply_changes        write    skip
  5  delete_old_releases  delete   skip
"
    );
}

mod common;

use common::{op0, panic0, TestContext, TestError};
use rehearse::{
    DryRunAction, DryRunPolicy, Impact, OperationMetadata, PlanBuilder, ProgressEvent,
    ProgressListener, ProgressOutcome,
};

#[derive(Default)]
struct RecordingProgress {
    events: Vec<String>,
}

impl ProgressListener<TestError> for RecordingProgress {
    fn on_event(&mut self, event: ProgressEvent<'_, TestError>) {
        match event {
            ProgressEvent::PlanStarted {
                mode,
                plan_name,
                total_nodes,
            } => self
                .events
                .push(format!("start {mode:?} {plan_name} {total_nodes}")),
            ProgressEvent::NodeDescribed {
                mode,
                node,
                outcome: ProgressOutcome::Described { dry_run_action },
            } => self.events.push(format!(
                "describe {mode:?} {} {} {:?}",
                node.position(),
                node.name(),
                dry_run_action
            )),
            ProgressEvent::PlanFinished {
                mode,
                plan_name,
                total_nodes,
                outcome,
            } => self.events.push(format!(
                "finish {mode:?} {plan_name} {total_nodes} {outcome:?}"
            )),
            _ => self.events.push("unexpected event".to_owned()),
        }
    }
}

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
fn describe_execution_does_not_invoke_operation_bodies() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("execute_inert");

    builder.add(panic0::<()>("write", Impact::Write));
    let read = builder.add(panic0::<u32>("read", Impact::Read));
    let plan = builder.finish(read);

    let description = plan.describe_execution();

    assert_eq!(context.calls(), Vec::<&str>::new());
    assert_eq!(description.len(), 2);
}

#[test]
fn describe_with_listener_reports_static_rows_without_invoking_bodies() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("describe_progress");

    builder.add(panic0::<()>("write", Impact::Write));
    let read = builder.add(panic0::<u32>("read", Impact::Read));
    let plan = builder.finish(read);
    let mut progress = RecordingProgress::default();

    let description = plan.describe_with_listener(&mut progress);

    assert_eq!(context.calls(), Vec::<&str>::new());
    assert_eq!(description.len(), 2);
    assert_eq!(
        progress.events,
        vec![
            "start Describe describe_progress 2",
            "describe Describe 1 write Some(Skip)",
            "describe Describe 2 read Some(Run)",
            "finish Describe describe_progress 2 Complete",
        ]
    );
}

#[test]
fn describe_execution_with_listener_omits_dry_run_actions() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("execute_progress");

    builder.add(op0("write", Impact::Write, ()));
    let read = builder.add(op0("read", Impact::Read, 1_u32));
    let plan = builder.finish(read);
    let mut progress = RecordingProgress::default();

    let description = plan.describe_execution_with_listener(&mut progress);

    assert_eq!(description.len(), 2);
    assert_eq!(
        progress.events,
        vec![
            "start Describe execute_progress 2",
            "describe Describe 1 write None",
            "describe Describe 2 read None",
            "finish Describe execute_progress 2 Complete",
        ]
    );
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
fn describe_execution_rows_match_plan_order_and_expose_metadata() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("ordered_execute");

    let login = builder.add(op0("login", Impact::Session, ()));
    let write = builder.add(op0("apply_changes", Impact::Write, ()));
    let opaque = builder.add(op0("shell_command", Impact::Opaque, ()));
    let plan = builder.finish(opaque);

    let rows = plan
        .describe_execution()
        .iter()
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(rows[0].node(), login.node());
    assert_eq!(rows[0].position(), 1);
    assert_eq!(rows[0].name(), "login");
    assert_eq!(rows[0].impact(), Impact::Session);

    assert_eq!(rows[1].node(), write.node());
    assert_eq!(rows[1].position(), 2);
    assert_eq!(rows[1].name(), "apply_changes");
    assert_eq!(rows[1].impact(), Impact::Write);

    assert_eq!(rows[2].node(), opaque.node());
    assert_eq!(rows[2].position(), 3);
    assert_eq!(rows[2].name(), "shell_command");
    assert_eq!(rows[2].impact(), Impact::Opaque);
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

#[test]
fn execution_display_omits_dry_run_action_column() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("deploy");

    builder.add(op0("login", Impact::Session, ()));
    builder.add(op0("read_current", Impact::Read, ()));
    builder.add(op0("calculate_changes", Impact::Pure, ()));
    builder.add(op0("apply_changes", Impact::Write, ()));
    let delete = builder.add(op0("delete_old_releases", Impact::Delete, ()));
    let plan = builder.finish(delete);

    assert_eq!(
        plan.describe_execution().to_string(),
        "\
deploy

  1  login                session
  2  read_current         read
  3  calculate_changes    pure
  4  apply_changes        write
  5  delete_old_releases  delete
"
    );
}

#[cfg(feature = "serde")]
#[test]
fn descriptions_serialize_to_json() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("json-description");

    let read = builder.add(op0("read", Impact::Read, 1_u32));
    let plan = builder.finish(read);

    let json = serde_json::to_string(&plan.describe()).expect("description serializes");

    assert!(json.contains("\"plan_name\":\"json-description\""));
    assert!(json.contains("\"dry_run_action\":\"run\""));
}

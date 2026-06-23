mod common;

use common::{fail0, op0, op1, op2, panic0, TestContext, TestError};
use rehearse::{
    ExecuteError, Impact, Input, PlanBuilder, ProgressEvent, ProgressListener, ProgressOutcome,
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
            ProgressEvent::NodeStarted { mode, node } => self.events.push(format!(
                "start-node {mode:?} {} {}",
                node.position(),
                node.name()
            )),
            ProgressEvent::NodeFinished {
                mode,
                node,
                outcome,
            } => self.events.push(format!(
                "finish-node {mode:?} {} {} {}",
                node.position(),
                node.name(),
                outcome_label(outcome)
            )),
            ProgressEvent::PlanFinished {
                mode,
                plan_name,
                total_nodes,
                outcome,
            } => self.events.push(format!(
                "finish {mode:?} {plan_name} {total_nodes} {outcome:?}"
            )),
            ProgressEvent::NodeDescribed { .. } => {
                self.events.push("unexpected describe event".to_owned());
            }
        }
    }
}

fn outcome_label(outcome: ProgressOutcome<'_, TestError>) -> String {
    match outcome {
        ProgressOutcome::Executed => "executed".to_owned(),
        ProgressOutcome::Failed { error } => format!("failed:{error}"),
        ProgressOutcome::Internal { error } => format!("internal:{error}"),
        ProgressOutcome::UnavailableDependencies {
            missing_dependencies,
        } => format!("unavailable:{missing_dependencies:?}"),
        ProgressOutcome::Skipped { reason } => format!("skipped:{reason}"),
        ProgressOutcome::Denied { reason } => format!("denied:{reason}"),
        ProgressOutcome::Blocked {
            missing_dependencies,
        } => format!("blocked:{missing_dependencies:?}"),
        ProgressOutcome::Described { .. } => "described".to_owned(),
    }
}

#[tokio::test]
async fn execute_runs_all_impacts_in_source_order() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("all-impacts");

    builder.add(op0("pure", Impact::Pure, ()));
    builder.add(op0("session", Impact::Session, ()));
    builder.add(op0("read", Impact::Read, ()));
    builder.add(op0("write", Impact::Write, ()));
    let delete = builder.add(op0("delete", Impact::Delete, "done"));
    let plan = builder.finish(delete);

    let output = plan.execute(&context).await.expect("execute succeeds");

    assert_eq!(output, "done");
    assert_eq!(
        context.calls(),
        vec!["pure", "session", "read", "write", "delete"]
    );
}

#[tokio::test]
async fn execute_flows_outputs_to_dependent_operations() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("data-flow");

    let seed = builder.add(op0("seed", Impact::Read, 2_u32));
    let doubled = builder.add(op1("double", Impact::Pure, Input::value(seed), |value| {
        value * 2
    }));
    let with_literal = builder.add(op2(
        "add-literal",
        Impact::Pure,
        (Input::value(doubled), Input::literal(3_u32)),
        |value, literal| value + literal,
    ));
    let plan = builder.finish(with_literal);

    let output = plan.execute(&context).await.expect("execute succeeds");

    assert_eq!(output, 7);
    assert_eq!(context.calls(), vec!["seed", "double", "add-literal"]);
}

#[tokio::test]
async fn execute_stops_at_the_first_operation_failure() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("fail-fast");

    builder.add(op0("login", Impact::Session, ()));
    let failing = builder.add(fail0::<u32>("read", Impact::Read, "read failed"));
    let after_failure = builder.add(panic0::<u32>("write", Impact::Write));
    let plan = builder.finish(after_failure);

    let error = plan.execute(&context).await.expect_err("execute fails");

    assert_eq!(context.calls(), vec!["login", "read"]);
    assert_eq!(
        error,
        ExecuteError::Operation {
            node: failing.node(),
            name: "read".to_owned(),
            source: TestError::Boom("read failed"),
        }
    );
}

#[tokio::test]
async fn execute_returns_the_final_output_when_all_nodes_succeed() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("final-output");

    let first = builder.add(op0("first", Impact::Pure, "hello".to_owned()));
    let final_value = builder.add(op1("second", Impact::Pure, Input::value(first), |value| {
        format!("{value} world")
    }));
    let plan = builder.finish(final_value);

    let output = plan.execute(&context).await.expect("execute succeeds");

    assert_eq!(output, "hello world");
}

#[tokio::test]
async fn execute_with_listener_reports_successful_nodes_and_completion() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("execute-progress");

    let first = builder.add(op0("first", Impact::Pure, 1_u32));
    let second = builder.add(op1("second", Impact::Pure, Input::value(first), |value| {
        value + 1
    }));
    let plan = builder.finish(second);
    let mut progress = RecordingProgress::default();

    let output = plan
        .execute_with_listener(&context, &mut progress)
        .await
        .expect("execute succeeds");

    assert_eq!(output, 2);
    assert_eq!(
        progress.events,
        vec![
            "start Execute execute-progress 2".to_owned(),
            "start-node Execute 1 first".to_owned(),
            "finish-node Execute 1 first executed".to_owned(),
            "start-node Execute 2 second".to_owned(),
            "finish-node Execute 2 second executed".to_owned(),
            "finish Execute execute-progress 2 Complete".to_owned(),
        ]
    );
}

#[tokio::test]
async fn execute_with_listener_reports_failure_before_returning() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("execute-fail");

    builder.add(op0("login", Impact::Session, ()));
    let failing = builder.add(fail0::<u32>("read", Impact::Read, "read failed"));
    let after_failure = builder.add(panic0::<u32>("write", Impact::Write));
    let plan = builder.finish(after_failure);
    let mut progress = RecordingProgress::default();

    let error = plan
        .execute_with_listener(&context, &mut progress)
        .await
        .expect_err("execute fails");

    assert_eq!(context.calls(), vec!["login", "read"]);
    assert_eq!(
        error,
        ExecuteError::Operation {
            node: failing.node(),
            name: "read".to_owned(),
            source: TestError::Boom("read failed"),
        }
    );
    assert_eq!(
        progress.events,
        vec![
            "start Execute execute-fail 3".to_owned(),
            "start-node Execute 1 login".to_owned(),
            "finish-node Execute 1 login executed".to_owned(),
            "start-node Execute 2 read".to_owned(),
            "finish-node Execute 2 read failed:read failed".to_owned(),
            "finish Execute execute-fail 3 Failed".to_owned(),
        ]
    );
}

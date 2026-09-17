mod common;

use common::{metadata, op0, op1, op2, panic1, TestContext, TestError};
use rehearse::{Impact, Input, NodeOutcome, Operation, PlanBuilder};

#[tokio::test]
async fn building_a_plan_invokes_no_operation_bodies() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("inert");

    let first = builder.add(op0("first", Impact::Pure, 1_u32));
    let second = builder.add(op1("second", Impact::Read, Input::value(first), |value| {
        value + 1
    }));
    let plan = builder.finish(second);

    assert_eq!(context.calls(), Vec::<&str>::new());

    let output = plan.execute(&context).await.expect("execute succeeds");
    assert_eq!(output, 2);
    assert_eq!(context.calls(), vec!["first", "second"]);
}

#[tokio::test]
async fn sync_operation_runs_through_the_async_runner() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("sync");

    let first = builder.add(Operation::sync(
        metadata("first", Impact::Pure),
        (),
        |context: &TestContext, ()| {
            context.record("first");
            Ok(20_u32)
        },
    ));
    let second = builder.add(Operation::sync(
        metadata("second", Impact::Pure),
        Input::value(first),
        |context: &TestContext, value| {
            context.record("second");
            Ok(value + 22)
        },
    ));
    let plan = builder.finish(second);

    let output = plan.execute(&context).await.expect("execute succeeds");

    assert_eq!(output, 42);
    assert_eq!(context.calls(), vec!["first", "second"]);
}

#[test]
fn node_order_matches_insertion_order() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("order");

    let first = builder.add(op0("first", Impact::Pure, 1_u32));
    let second = builder.add(op0("second", Impact::Session, 2_u32));
    let third = builder.add(op0("third", Impact::Read, 3_u32));
    let plan = builder.finish(third);

    let nodes = plan.nodes().collect::<Vec<_>>();
    assert_eq!(nodes[0].id(), first.node());
    assert_eq!(nodes[0].name(), "first");
    assert_eq!(nodes[0].impact(), Impact::Pure);
    assert_eq!(nodes[1].id(), second.node());
    assert_eq!(nodes[1].name(), "second");
    assert_eq!(nodes[2].id(), third.node());
    assert_eq!(nodes[2].name(), "third");
}

#[test]
fn value_points_to_the_correct_producer() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("values");

    let first = builder.add(op0("first", Impact::Pure, 1_u32));
    let second = builder.add(op1("second", Impact::Read, Input::value(first), |value| {
        value + 1
    }));
    let plan = builder.finish(second);

    assert_eq!(first.node().index(), 0);
    assert_eq!(second.node().index(), 1);

    let second_node = plan.nodes().nth(1).expect("second node");
    assert_eq!(second_node.dependencies(), &[first.node()]);
}

#[test]
fn mermaid_output_uses_static_dependencies() {
    let mut builder = PlanBuilder::<TestContext, TestError>::new("graph");

    let read = builder.add(op0("read \"current\"", Impact::Read, 1_u32));
    let write = builder.add(op1(
        "apply\\changes",
        Impact::Write,
        Input::value(read),
        |value| value + 1,
    ));
    let plan = builder.finish(write);

    assert_eq!(
        plan.to_mermaid(),
        "\
flowchart TD
  %% plan: graph
  n0[\"1. read \\\"current\\\"\\nread\"]
  n1[\"2. apply\\\\changes\\nwrite\"]
  n0 --> n1
"
    );
}

#[tokio::test]
async fn reusing_one_value_in_multiple_later_operations_works() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("reuse");

    let base = builder.add(op0("base", Impact::Read, 10_u32));
    let left = builder.add(op1("left", Impact::Pure, Input::value(base), |value| {
        value + 1
    }));
    let right = builder.add(op1("right", Impact::Pure, Input::value(base), |value| {
        value + 2
    }));
    let sum = builder.add(op2(
        "sum",
        Impact::Pure,
        (Input::value(left), Input::value(right)),
        |left, right| left + right,
    ));
    let plan = builder.finish(sum);

    let output = plan.execute(&context).await.expect("execute succeeds");

    assert_eq!(output, 23);
    assert_eq!(context.calls(), vec!["base", "left", "right", "sum"]);
}

#[tokio::test]
async fn running_the_same_plan_twice_uses_independent_stores() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("fresh-store");

    let login = builder.add(op0("login", Impact::Session, ()));
    let write = builder.add(op1("write", Impact::Write, Input::value(login), |_| 41_u32));
    let inspect = builder.add(op1("inspect", Impact::Read, Input::value(write), |value| {
        value + 1
    }));
    let plan = builder.finish(inspect);

    let output = plan.execute(&context).await.expect("execute succeeds");
    assert_eq!(output, 42);

    let report = plan.dry_run(&context).await;
    let inspect_report = report
        .iter()
        .find(|node| node.name() == "inspect")
        .expect("inspect report");

    assert!(matches!(
        inspect_report.outcome(),
        NodeOutcome::Blocked {
            missing_dependencies
        } if missing_dependencies == &[write.node()]
    ));
}

#[tokio::test]
async fn blocked_nodes_do_not_receive_fabricated_values() {
    let context = TestContext::default();
    let mut builder = PlanBuilder::<TestContext, TestError>::new("no-fakes");

    let write = builder.add(op0("write", Impact::Write, 10_u32));
    let dependent = builder.add(panic1::<u32, ()>(
        "dependent",
        Impact::Read,
        Input::value(write),
    ));
    let plan = builder.finish(dependent);

    let report = plan.dry_run(&context).await;

    assert_eq!(context.calls(), Vec::<&str>::new());
    assert!(matches!(
        report.iter().nth(1).expect("dependent report").outcome(),
        NodeOutcome::Blocked {
            missing_dependencies
        } if missing_dependencies == &[write.node()]
    ));
}

#[test]
fn foreign_inputs_are_rejected_before_any_body_runs() {
    let mut a = PlanBuilder::<TestContext, TestError>::new("a");
    let foreign = a.add(op0("foreign", Impact::Pure, 100_u32));
    let mut b = PlanBuilder::<TestContext, TestError>::new("b");
    let local = b.add(common::panic0::<u32>("local", Impact::Write));
    assert_eq!(foreign.node(), local.node());
    assert_ne!(foreign, local);
    let consumer = b.add(panic1::<u32, u32>(
        "consumer",
        Impact::Write,
        Input::value(foreign),
    ));
    let error = b
        .try_finish(consumer)
        .err()
        .expect("foreign input rejected");
    assert_eq!(
        error,
        rehearse::PlanBuildError::ForeignValue {
            node: foreign.node(),
            consumer: Some(consumer.node()),
        }
    );
}

#[test]
fn foreign_final_outputs_are_rejected_even_for_empty_plans() {
    let mut a = PlanBuilder::<TestContext, TestError>::new("a");
    let foreign = a.add(op0("foreign", Impact::Pure, 100_u32));
    for populated in [false, true] {
        let mut b = PlanBuilder::<TestContext, TestError>::new("b");
        if populated {
            b.add(common::panic0::<u32>("local", Impact::Pure));
        }
        assert_eq!(
            b.try_finish(foreign).err(),
            Some(rehearse::PlanBuildError::ForeignValue {
                node: foreign.node(),
                consumer: None,
            })
        );
    }
}

#[test]
#[should_panic(expected = "value belongs to another plan")]
fn finish_panics_for_foreign_output() {
    let mut a = PlanBuilder::<TestContext, TestError>::new("a");
    let foreign = a.add(op0("foreign", Impact::Pure, 100_u32));
    PlanBuilder::<TestContext, TestError>::new("b").finish(foreign);
}

#[test]
fn value_is_copy_without_a_clone_or_copy_output_bound() {
    struct NotClone;
    fn assert_copy<T: Copy>() {}
    assert_copy::<rehearse::Value<NotClone>>();
}

#[tokio::test]
async fn concurrent_runs_of_one_plan_have_separate_stores() {
    let mut builder = PlanBuilder::<u32, ()>::new("concurrent");
    let value = builder.add(Operation::new(
        metadata("context", Impact::Read),
        (),
        |context, ()| {
            Box::pin(async move {
                tokio::task::yield_now().await;
                Ok(*context)
            })
        },
    ));
    let output = builder.add(Operation::sync(
        metadata("double", Impact::Pure),
        Input::value(value),
        |_, v| Ok(v * 2),
    ));
    let plan = builder.try_finish(output).expect("valid plan");
    let (left, right) = tokio::join!(plan.execute(&10), plan.execute(&20));
    assert_eq!(left.unwrap(), 20);
    assert_eq!(right.unwrap(), 40);
}

#[test]
fn validation_and_describe_do_not_clone_literal_inputs() {
    struct NoCloneDuringConstruction;
    impl Clone for NoCloneDuringConstruction {
        fn clone(&self) -> Self {
            panic!("literal must not be resolved during construction or describe")
        }
    }
    let mut builder = PlanBuilder::<(), ()>::new("static");
    let output = builder.add(Operation::sync(
        metadata("input", Impact::Read),
        Input::literal(NoCloneDuringConstruction),
        |_, _| Ok(()),
    ));
    let plan = builder
        .try_finish(output)
        .expect("metadata-only validation");
    assert_eq!(plan.describe().len(), 1);
    assert_eq!(plan.describe_execution().len(), 1);
    assert!(plan.to_mermaid().contains("input"));
}

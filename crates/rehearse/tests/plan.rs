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

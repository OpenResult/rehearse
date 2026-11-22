use rehearse::{
    Impact, Input, NodeOutcome, Operation, OperationMetadata, PlanBuilder, ProgressEvent,
};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{Barrier, Notify};

// Concurrency must not require cloning or formatting operation errors.
#[derive(Debug)]
struct MarkerError(&'static str);

struct Context {
    events: Mutex<Vec<String>>,
    active: AtomicUsize,
    peak: AtomicUsize,
    barrier: Barrier,
    failed: Notify,
}
impl Context {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            barrier: Barrier::new(2),
            failed: Notify::new(),
        }
    }
    fn record(&self, name: impl Into<String>) {
        self.events.lock().unwrap().push(name.into());
    }
    fn start(&self, name: &str) {
        self.record(format!("start:{name}"));
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
    }
    fn finish(&self, name: &str) {
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.record(format!("finish:{name}"));
    }
}

fn yielding(name: &'static str, output: u32) -> Operation<Context, u32, MarkerError> {
    Operation::new(
        OperationMetadata::new(name, Impact::Read),
        (),
        move |ctx: &Context, ()| {
            Box::pin(async move {
                ctx.start(name);
                tokio::task::yield_now().await;
                ctx.finish(name);
                Ok(output)
            })
        },
    )
}
fn increment(
    name: &'static str,
    value: rehearse::Value<u32>,
) -> Operation<Context, u32, MarkerError> {
    Operation::new(
        OperationMetadata::new(name, Impact::Read),
        Input::value(value),
        move |ctx: &Context, value| {
            Box::pin(async move {
                ctx.start(name);
                tokio::task::yield_now().await;
                ctx.finish(name);
                Ok(value + 1)
            })
        },
    )
}
fn sum(
    left: rehearse::Value<u32>,
    right: rehearse::Value<u32>,
) -> Operation<Context, u32, MarkerError> {
    Operation::sync(
        OperationMetadata::new("after", Impact::Pure),
        (Input::value(left), Input::value(right)),
        |ctx: &Context, (left, right)| {
            assert_eq!(
                ctx.active.load(Ordering::SeqCst),
                0,
                "group barrier must settle all operations"
            );
            ctx.record("after");
            Ok(left + right)
        },
    )
}
fn limit(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

#[tokio::test]
async fn declared_branches_overlap_preserve_lane_order_and_join_before_following_work() {
    let ctx = Context::new();
    let mut builder = PlanBuilder::<Context, MarkerError>::new("lanes");
    let (left, right) = builder.parallel("non-production", limit(2), |group| {
        let left = group.branch("dev", |lane| {
            let value = lane.add(yielding("deploy-dev", 10));
            lane.add(increment("verify-dev", value))
        });
        let right = group.branch("staging", |lane| {
            let value = lane.add(yielding("deploy-staging", 20));
            lane.add(increment("verify-staging", value))
        });
        (left, right)
    });
    let output = builder.add(sum(left, right));
    let plan = builder.try_finish(output).unwrap();
    let mut events = Vec::new();
    let mut listener = |event: ProgressEvent<'_, MarkerError>| match event {
        ProgressEvent::NodeStarted { node, .. } => events.push(format!("start:{}", node.name())),
        ProgressEvent::NodeFinished { node, .. } => events.push(format!("finish:{}", node.name())),
        _ => {}
    };
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        plan.execute_concurrent_with_listener(&ctx, &mut listener),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(output, 32);
    assert_eq!(ctx.peak.load(Ordering::SeqCst), 2);
    let body_events = ctx.events.lock().unwrap();
    for (deploy, verify) in [
        ("deploy-dev", "verify-dev"),
        ("deploy-staging", "verify-staging"),
    ] {
        let finish = body_events
            .iter()
            .position(|value| value == &format!("finish:{deploy}"))
            .unwrap();
        let start = body_events
            .iter()
            .position(|value| value == &format!("start:{verify}"))
            .unwrap();
        assert!(finish < start);
    }
    assert_eq!(body_events.last().unwrap(), "after");
    let second_start = events
        .iter()
        .position(|value| value == "start:deploy-staging")
        .unwrap();
    let first_finish = events
        .iter()
        .position(|value| value.starts_with("finish:"))
        .unwrap();
    assert!(
        second_start < first_finish,
        "progress must expose multiple running nodes"
    );
}

#[tokio::test]
async fn existing_execute_remains_sequential_even_for_declared_parallel_groups() {
    let ctx = Context::new();
    let mut builder = PlanBuilder::<Context, MarkerError>::new("serial-fallback");
    let (left, right) = builder.parallel("group", limit(3), |group| {
        let left = group.branch("left", |lane| {
            let first = lane.add(yielding("a", 1));
            lane.add(increment("a-next", first))
        });
        let right = group.branch("right", |lane| lane.add(yielding("b", 2)));
        (left, right)
    });
    let output = builder.add(sum(left, right));
    let output = builder.finish(output).execute(&ctx).await.unwrap();
    assert_eq!(output, 4);
    assert_eq!(ctx.peak.load(Ordering::SeqCst), 1);
    assert_eq!(
        *ctx.events.lock().unwrap(),
        [
            "start:a",
            "finish:a",
            "start:a-next",
            "finish:a-next",
            "start:b",
            "finish:b",
            "after"
        ]
    );
}

#[tokio::test]
async fn group_limits_bound_real_inflight_work_and_no_unmarked_nodes_are_reordered() {
    for maximum in [1, 2, 3] {
        let ctx = Context::new();
        let mut builder = PlanBuilder::<Context, MarkerError>::new("limit");
        builder.add(yielding("before", 0));
        let output = builder.parallel("bounded", limit(maximum), |group| {
            group.branch("a", |lane| lane.add(yielding("a", 1)));
            group.branch("b", |lane| lane.add(yielding("b", 2)));
            group.branch("c", |lane| lane.add(yielding("c", 3)))
        });
        let plan = builder.finish(output);
        assert_eq!(plan.execute_concurrent(&ctx).await.unwrap(), 3);
        assert_eq!(ctx.peak.load(Ordering::SeqCst), maximum);
        let events = ctx.events.lock().unwrap();
        assert_eq!(&events[..2], ["start:before", "finish:before"]);
    }
    let ctx = Context::new();
    let mut builder = PlanBuilder::<Context, MarkerError>::new("ordinary");
    builder.add(yielding("a", 1));
    let output = builder.add(yielding("b", 2));
    builder
        .finish(output)
        .execute_concurrent(&ctx)
        .await
        .unwrap();
    assert_eq!(ctx.peak.load(Ordering::SeqCst), 1);
}

#[test]
fn sibling_branch_dependencies_are_rejected_at_construction() {
    let mut builder = PlanBuilder::<Context, MarkerError>::new("invalid-sibling");
    let output = builder.parallel("siblings", limit(2), |group| {
        let left = group.branch("left", |lane| lane.add(yielding("left", 1)));
        group.branch("right", |lane| lane.add(increment("right", left)))
    });
    assert!(builder.try_finish(output).is_err());
}

fn failing_after_peer_starts() -> Operation<Context, u32, MarkerError> {
    Operation::new(
        OperationMetadata::new("fail", Impact::Write),
        (),
        |ctx: &Context, ()| {
            Box::pin(async move {
                ctx.record("fail-start");
                ctx.barrier.wait().await;
                ctx.failed.notify_one();
                Err(MarkerError("first failure"))
            })
        },
    )
}
fn draining_peer() -> Operation<Context, u32, MarkerError> {
    Operation::new(
        OperationMetadata::new("peer", Impact::Write),
        (),
        |ctx: &Context, ()| {
            Box::pin(async move {
                ctx.record("peer-start");
                ctx.barrier.wait().await;
                ctx.failed.notified().await;
                tokio::task::yield_now().await;
                ctx.record("peer-settled");
                Ok(2)
            })
        },
    )
}

#[tokio::test]
async fn first_failure_stops_new_work_but_drains_started_mutations_before_returning() {
    let ctx = Context::new();
    let mut builder = PlanBuilder::<Context, MarkerError>::new("drain");
    let (left, right) = builder.parallel("writes", limit(2), |group| {
        let left = group.branch("failing", |lane| {
            let failed = lane.add(failing_after_peer_starts());
            lane.add(increment("never-after-failure", failed))
        });
        let right = group.branch("inflight", |lane| {
            let peer = lane.add(draining_peer());
            lane.add(increment("never-after-peer", peer))
        });
        group.branch("queued", |lane| lane.add(yielding("never-queued", 9)));
        (left, right)
    });
    let output = builder.add(sum(left, right));
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        builder.finish(output).execute_concurrent(&ctx),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.errors().len(), 1);
    match error.primary_error() {
        rehearse::ExecuteError::Operation { source, .. } => assert_eq!(source.0, "first failure"),
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(
        error
            .completed_nodes()
            .iter()
            .map(|node| node.index())
            .collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(
        error
            .not_started_nodes()
            .iter()
            .map(|node| node.index())
            .collect::<Vec<_>>(),
        [1, 3, 4, 5]
    );
    let events = ctx.events.lock().unwrap();
    assert!(events.iter().any(|event| event == "peer-settled"));
    assert!(!events.iter().any(|event| event.contains("never")));
    assert!(!events.iter().any(|event| event == "after"));
}

#[tokio::test]
async fn every_already_running_failure_is_retained_without_clone_or_display_bounds() {
    let ctx = Context::new();
    let mut builder = PlanBuilder::<Context, MarkerError>::new("multiple-errors");
    let output = builder.parallel("group", limit(2), |group| {
        let mut result = None;
        for name in ["a", "b"] {
            result = Some(group.branch(name, |lane| {
                lane.add(Operation::new(
                    OperationMetadata::new(name, Impact::Write),
                    (),
                    move |ctx: &Context, ()| {
                        Box::pin(async move {
                            ctx.barrier.wait().await;
                            Err::<u32, _>(MarkerError(name))
                        })
                    },
                ))
            }));
        }
        result.unwrap()
    });
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        builder.finish(output).execute_concurrent(&ctx),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.errors().len(), 2);
    let mut names = error
        .errors()
        .iter()
        .map(|error| match error {
            rehearse::ExecuteError::Operation { source, .. } => source.0,
            _ => panic!("unexpected invariant failure"),
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["a", "b"]);
    assert!(error.not_started_nodes().is_empty());
}

#[tokio::test]
async fn concurrent_dry_run_keeps_policy_first_and_continues_independent_checks_in_ordered_reports()
{
    let ctx = Context::new();
    let mut builder = PlanBuilder::<Context, MarkerError>::new("rehearsal");
    builder.parallel("checks", limit(2), |group| {
        group.branch("skipped-write", |lane| {
            let write = lane.add(Operation::sync(
                OperationMetadata::new("write", Impact::Write),
                (),
                |_: &Context, ()| -> Result<u32, MarkerError> { panic!("write body must not run") },
            ));
            let blocked = lane.add(Operation::sync(
                OperationMetadata::new("dependent-read", Impact::Read),
                Input::value(write),
                |_: &Context, _: u32| -> Result<u32, MarkerError> {
                    panic!("missing input must block")
                },
            ));
            lane.add(Operation::sync(
                OperationMetadata::new("dependent-write", Impact::Write),
                Input::value(blocked),
                |_: &Context, _: u32| -> Result<u32, MarkerError> {
                    panic!("policy is checked first")
                },
            ));
            lane.add(yielding("independent-a", 1))
        });
        group.branch("failed-read", |lane| {
            lane.add(Operation::sync(
                OperationMetadata::new("failure", Impact::Read),
                (),
                |_: &Context, ()| Err::<u32, _>(MarkerError("read failed")),
            ));
            lane.add(yielding("independent-b", 2))
        })
    });
    let output = builder.add(yielding("independent-after-group", 3));
    let report = builder.finish(output).dry_run_concurrent(&ctx).await;
    assert_eq!(report.len(), 7);
    assert_eq!(
        report
            .iter()
            .map(|row| row.node().index())
            .collect::<Vec<_>>(),
        (0..7).collect::<Vec<_>>()
    );
    let rows = report.iter().collect::<Vec<_>>();
    assert!(matches!(rows[0].outcome(), NodeOutcome::Skipped { .. }));
    assert!(matches!(rows[1].outcome(), NodeOutcome::Blocked { .. }));
    assert!(matches!(rows[2].outcome(), NodeOutcome::Skipped { .. }));
    assert!(matches!(rows[4].outcome(), NodeOutcome::Failed { .. }));
    for index in [3, 5, 6] {
        assert!(rows[index].outcome().is_executed());
    }
    assert_eq!(report.failure_count(), 1);
}

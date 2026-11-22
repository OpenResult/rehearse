use rehearse::{Impact, Operation, OperationMetadata, PlanBuildError, PlanBuilder};
use std::num::NonZeroUsize;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn number() -> Operation<(), u32, ()> {
    Operation::sync(
        OperationMetadata::new("number", Impact::Pure),
        (),
        |_, ()| panic!("construction validation must not invoke operation bodies"),
    )
}

fn limit() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}

#[test]
fn rejects_empty_parallel_groups_with_their_declared_name() {
    let mut builder = PlanBuilder::<(), ()>::new("empty-group");
    let output = builder.add(number());
    builder.parallel("non-production", limit(), |_| ());

    assert!(matches!(
        builder.try_finish(output),
        Err(PlanBuildError::EmptyParallelGroup { group }) if group == "non-production"
    ));
}

#[test]
fn rejects_empty_parallel_branches_with_their_group_and_branch_names() {
    let mut builder = PlanBuilder::<(), ()>::new("empty-branch");
    let output = builder.parallel("non-production", limit(), |group| {
        group.branch("dev", |_| ());
        group.branch("staging", |branch| branch.add(number()))
    });

    assert!(matches!(
        builder.try_finish(output),
        Err(PlanBuildError::EmptyParallelBranch { group, branch })
            if group == "non-production" && branch == "dev"
    ));
}

#[test]
fn catching_a_parallel_callback_panic_cannot_silently_omit_added_nodes() {
    let mut builder = PlanBuilder::<(), ()>::new("caught-group-panic");
    builder.add(number());
    let panic = catch_unwind(AssertUnwindSafe(|| {
        builder.parallel("interrupted", limit(), |group| {
            group.branch("complete-branch", |branch| branch.add(number()));
            panic!("interrupted group construction");
        });
    }));
    assert!(panic.is_err());
    let output = builder.add(number());

    assert!(matches!(
        builder.try_finish(output),
        Err(PlanBuildError::InvalidExecutionStructure { node }) if node.index() == 1
    ));
}

#[test]
fn catching_a_branch_callback_panic_cannot_silently_omit_added_nodes() {
    let mut builder = PlanBuilder::<(), ()>::new("caught-branch-panic");
    builder.add(number());
    let output = builder.parallel("non-production", limit(), |group| {
        let panic = catch_unwind(AssertUnwindSafe(|| {
            group.branch("interrupted", |branch| {
                branch.add(number());
                panic!("interrupted branch construction");
            });
        }));
        assert!(panic.is_err());
        group.branch("staging", |branch| branch.add(number()))
    });

    assert!(matches!(
        builder.try_finish(output),
        Err(PlanBuildError::InvalidExecutionStructure { node }) if node.index() == 1
    ));
}

#[cfg(feature = "serde")]
#[test]
fn concurrent_error_json_requires_a_real_failure_and_retains_structured_outcomes() {
    use rehearse::{ConcurrentExecuteError, ExecuteError};

    let invalid = r#"{"errors":[],"completed":[],"not_started":[]}"#;
    assert!(serde_json::from_str::<ConcurrentExecuteError<String>>(invalid).is_err());

    let valid = r#"{
        "errors":[{"operation":{"node":0,"name":"deploy-dev","source":"failed"}}],
        "completed":[1],
        "not_started":[2]
    }"#;
    let error: ConcurrentExecuteError<String> = serde_json::from_str(valid).unwrap();
    assert!(matches!(
        error.primary_error(),
        ExecuteError::Operation { node, name, source }
            if node.index() == 0 && name == "deploy-dev" && source == "failed"
    ));
    assert_eq!(error.completed_nodes()[0].index(), 1);
    assert_eq!(error.not_started_nodes()[0].index(), 2);
    let json = serde_json::to_string(&error).unwrap();
    assert_eq!(
        error,
        serde_json::from_str::<ConcurrentExecuteError<String>>(&json).unwrap()
    );
}

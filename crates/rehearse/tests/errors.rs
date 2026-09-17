use rehearse::{
    ExecuteError, Impact, Operation, OperationMetadata, PlanBuilder, ProgressEvent, ProgressOutcome,
};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct NonCloneError;

#[tokio::test]
async fn execution_reports_and_listeners_accept_non_clone_non_display_errors() {
    let mut builder = PlanBuilder::<(), NonCloneError>::new("error");
    let output = builder.add(Operation::sync(
        OperationMetadata::new("fail", Impact::Read),
        (),
        |_, ()| Err::<(), _>(NonCloneError),
    ));
    let plan = builder.finish(output);
    let mut observed = 0;
    let mut listener = |event: ProgressEvent<'_, NonCloneError>| {
        if let ProgressEvent::NodeFinished {
            outcome: ProgressOutcome::Failed {
                error: NonCloneError,
            },
            ..
        } = event
        {
            observed += 1;
        }
    };
    let report = plan.dry_run_with_listener(&(), &mut listener).await;
    assert_eq!(report.failure_count(), 1);
    assert!(matches!(
        plan.execute_with_listener(&(), &mut listener).await,
        Err(ExecuteError::Operation {
            source: NonCloneError,
            ..
        })
    ));
    assert_eq!(observed, 2);
    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_string(&report).unwrap();
        let restored: rehearse::DryRunReport<NonCloneError> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.failure_count(), 1);
    }
}

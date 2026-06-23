use crate::operation::NodeRunError;
use crate::plan::store::ValueStore;
use crate::{
    DryRunAction, DryRunPolicy, DryRunReport, NodeOutcome, NodeReport, NoopProgress,
    OperationMetadata, Plan, ProgressEvent, ProgressListener, ProgressMode, ProgressNode,
    ProgressOutcome, ProgressPlanOutcome,
};

pub async fn dry_run<C, T, E, P>(plan: &Plan<C, T, E>, context: &C, policy: &P) -> DryRunReport<E>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
    P: DryRunPolicy,
{
    let mut progress = NoopProgress;
    dry_run_with_listener(plan, context, policy, &mut progress).await
}

pub async fn dry_run_with_listener<C, T, E, P, L>(
    plan: &Plan<C, T, E>,
    context: &C,
    policy: &P,
    listener: &mut L,
) -> DryRunReport<E>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
    P: DryRunPolicy,
    L: ProgressListener<E> + ?Sized,
{
    let mut store = ValueStore::new();
    let mut report = DryRunReport::new(plan.name());
    let total_nodes = plan.nodes.len();

    listener.on_event(ProgressEvent::PlanStarted {
        mode: ProgressMode::DryRun,
        plan_name: plan.name(),
        total_nodes,
    });

    for (index, node) in plan.nodes.iter().enumerate() {
        let metadata = node.metadata();
        let progress_node = ProgressNode::new(
            node.id(),
            index + 1,
            total_nodes,
            metadata.name(),
            metadata.impact(),
        );
        listener.on_event(ProgressEvent::NodeStarted {
            mode: ProgressMode::DryRun,
            node: progress_node,
        });

        let outcome = match policy.action(node.metadata()) {
            DryRunAction::Skip => NodeOutcome::Skipped {
                reason: impact_reason(metadata),
            },
            DryRunAction::Deny => NodeOutcome::Denied {
                reason: impact_reason(metadata),
            },
            DryRunAction::Run => {
                let missing = node
                    .dependencies()
                    .iter()
                    .copied()
                    .filter(|dependency| !store.contains(*dependency))
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    NodeOutcome::Blocked {
                        missing_dependencies: missing,
                    }
                } else {
                    match node.run(context, &store).await {
                        Ok(output) => {
                            store.insert_erased(node.id(), output);
                            NodeOutcome::Executed
                        }
                        Err(NodeRunError::Operation(error)) => NodeOutcome::Failed { error },
                        Err(NodeRunError::Internal(error)) => NodeOutcome::Internal { error },
                    }
                }
            }
        };

        listener.on_event(ProgressEvent::NodeFinished {
            mode: ProgressMode::DryRun,
            node: progress_node,
            outcome: progress_outcome(&outcome),
        });
        report.push(NodeReport::new(node.id(), metadata, outcome));
    }

    listener.on_event(ProgressEvent::PlanFinished {
        mode: ProgressMode::DryRun,
        plan_name: plan.name(),
        total_nodes,
        outcome: ProgressPlanOutcome::from(report.status()),
    });

    report
}

fn impact_reason(metadata: &OperationMetadata) -> String {
    format!("{} operation", metadata.impact())
}

fn progress_outcome<E>(outcome: &NodeOutcome<E>) -> ProgressOutcome<'_, E> {
    match outcome {
        NodeOutcome::Executed => ProgressOutcome::Executed,
        NodeOutcome::Skipped { reason } => ProgressOutcome::Skipped { reason },
        NodeOutcome::Denied { reason } => ProgressOutcome::Denied { reason },
        NodeOutcome::Blocked {
            missing_dependencies,
        } => ProgressOutcome::Blocked {
            missing_dependencies,
        },
        NodeOutcome::Failed { error } => ProgressOutcome::Failed { error },
        NodeOutcome::Internal { error } => ProgressOutcome::Internal { error },
    }
}

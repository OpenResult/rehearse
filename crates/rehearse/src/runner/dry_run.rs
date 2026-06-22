use crate::operation::NodeRunError;
use crate::plan::store::ValueStore;
use crate::{
    DryRunAction, DryRunPolicy, DryRunReport, NodeOutcome, NodeReport, OperationMetadata, Plan,
};

pub async fn dry_run<C, T, E, P>(plan: &Plan<C, T, E>, context: &C, policy: &P) -> DryRunReport<E>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
    P: DryRunPolicy,
{
    let mut store = ValueStore::new();
    let mut report = DryRunReport::new(plan.name());

    for node in &plan.nodes {
        let outcome = match policy.action(node.metadata()) {
            DryRunAction::Skip => NodeOutcome::Skipped {
                reason: impact_reason(node.metadata()),
            },
            DryRunAction::Deny => NodeOutcome::Denied {
                reason: impact_reason(node.metadata()),
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

        report.push(NodeReport::new(node.id(), node.metadata(), outcome));
    }

    report
}

fn impact_reason(metadata: &OperationMetadata) -> String {
    format!("{} operation", metadata.impact())
}

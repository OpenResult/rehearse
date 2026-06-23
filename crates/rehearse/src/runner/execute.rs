use crate::operation::NodeRunError;
use crate::plan::store::{ResolveInputError, ValueStore};
use crate::{
    ExecuteError, NoopProgress, Plan, ProgressEvent, ProgressListener, ProgressMode, ProgressNode,
    ProgressOutcome, ProgressPlanOutcome,
};

pub async fn execute<C, T, E>(plan: &Plan<C, T, E>, context: &C) -> Result<T, ExecuteError<E>>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
{
    let mut progress = NoopProgress;
    execute_with_listener(plan, context, &mut progress).await
}

pub async fn execute_with_listener<C, T, E, L>(
    plan: &Plan<C, T, E>,
    context: &C,
    listener: &mut L,
) -> Result<T, ExecuteError<E>>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
    L: ProgressListener<E> + ?Sized,
{
    let mut store = ValueStore::new();
    let total_nodes = plan.nodes.len();

    listener.on_event(ProgressEvent::PlanStarted {
        mode: ProgressMode::Execute,
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
            mode: ProgressMode::Execute,
            node: progress_node,
        });

        let missing = node
            .dependencies()
            .iter()
            .copied()
            .filter(|dependency| !store.contains(*dependency))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            listener.on_event(ProgressEvent::NodeFinished {
                mode: ProgressMode::Execute,
                node: progress_node,
                outcome: ProgressOutcome::UnavailableDependencies {
                    missing_dependencies: &missing,
                },
            });
            listener.on_event(ProgressEvent::PlanFinished {
                mode: ProgressMode::Execute,
                plan_name: plan.name(),
                total_nodes,
                outcome: ProgressPlanOutcome::Failed,
            });
            return Err(ExecuteError::Internal(format!(
                "node {} ('{}') has unavailable dependencies: {}",
                node.id(),
                metadata.name(),
                format_node_list(&missing)
            )));
        }

        match node.run(context, &store).await {
            Ok(output) => {
                store.insert_erased(node.id(), output);
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: progress_node,
                    outcome: ProgressOutcome::Executed,
                });
            }
            Err(NodeRunError::Operation(source)) => {
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: progress_node,
                    outcome: ProgressOutcome::Failed { error: &source },
                });
                listener.on_event(ProgressEvent::PlanFinished {
                    mode: ProgressMode::Execute,
                    plan_name: plan.name(),
                    total_nodes,
                    outcome: ProgressPlanOutcome::Failed,
                });
                return Err(ExecuteError::Operation {
                    node: node.id(),
                    name: metadata.name().to_owned(),
                    source,
                });
            }
            Err(NodeRunError::Internal(message)) => {
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: progress_node,
                    outcome: ProgressOutcome::Internal { error: &message },
                });
                listener.on_event(ProgressEvent::PlanFinished {
                    mode: ProgressMode::Execute,
                    plan_name: plan.name(),
                    total_nodes,
                    outcome: ProgressPlanOutcome::Failed,
                });
                return Err(ExecuteError::Internal(message));
            }
        }
    }

    match store.require(plan.output) {
        Ok(output) => {
            listener.on_event(ProgressEvent::PlanFinished {
                mode: ProgressMode::Execute,
                plan_name: plan.name(),
                total_nodes,
                outcome: ProgressPlanOutcome::Complete,
            });
            Ok(output)
        }
        Err(error) => {
            listener.on_event(ProgressEvent::PlanFinished {
                mode: ProgressMode::Execute,
                plan_name: plan.name(),
                total_nodes,
                outcome: ProgressPlanOutcome::Failed,
            });
            Err(ExecuteError::Internal(final_output_error(error)))
        }
    }
}

fn format_node_list(nodes: &[crate::NodeId]) -> String {
    nodes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn final_output_error(error: ResolveInputError) -> String {
    format!("final output could not be resolved: {error}")
}

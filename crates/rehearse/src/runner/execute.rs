use crate::operation::NodeRunError;
use crate::plan::store::{ResolveInputError, ValueStore};
use crate::{ExecuteError, Plan};

pub async fn execute<C, T, E>(plan: &Plan<C, T, E>, context: &C) -> Result<T, ExecuteError<E>>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
{
    let mut store = ValueStore::new();

    for node in &plan.nodes {
        let missing = node
            .dependencies()
            .iter()
            .copied()
            .filter(|dependency| !store.contains(*dependency))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(ExecuteError::Internal(format!(
                "node {} ('{}') has unavailable dependencies: {}",
                node.id(),
                node.metadata().name(),
                format_node_list(&missing)
            )));
        }

        match node.run(context, &store).await {
            Ok(output) => store.insert_erased(node.id(), output),
            Err(NodeRunError::Operation(source)) => {
                return Err(ExecuteError::Operation {
                    node: node.id(),
                    name: node.metadata().name().to_owned(),
                    source,
                });
            }
            Err(NodeRunError::Internal(message)) => return Err(ExecuteError::Internal(message)),
        }
    }

    store
        .require(plan.output)
        .map_err(|error| ExecuteError::Internal(final_output_error(error)))
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

use crate::operation::NodeRunError;
use crate::plan::node::ErasedNode;
use crate::plan::store::{StoredValue, ValueStore};
use crate::{
    BoxFuture, ConcurrentExecuteError, DryRunAction, DryRunPolicy, DryRunReport, ExecuteError,
    ExecutionStep, InvariantError, NodeId, NodeOutcome, NodeReport, ParallelBranch, Plan,
    ProgressEvent, ProgressListener, ProgressMode, ProgressNode, ProgressOutcome,
    ProgressPlanOutcome,
};
use futures_util::stream::{FuturesUnordered, StreamExt};

type Pending<'a, E> = BoxFuture<'a, (usize, NodeId, Result<StoredValue, NodeRunError<E>>)>;

fn info<C, E>(nodes: &[Box<dyn ErasedNode<C, E>>], id: NodeId) -> ProgressNode<'_> {
    let node = &nodes[id.index()];
    ProgressNode::new(
        id,
        id.index() + 1,
        nodes.len(),
        node.metadata().name(),
        node.metadata().impact(),
    )
}

fn start<C, E, L: ProgressListener<E> + ?Sized>(
    nodes: &[Box<dyn ErasedNode<C, E>>],
    id: NodeId,
    mode: ProgressMode,
    listener: &mut L,
) {
    listener.on_event(ProgressEvent::NodeStarted {
        mode,
        node: info(nodes, id),
    });
}

fn missing<C, E>(
    nodes: &[Box<dyn ErasedNode<C, E>>],
    id: NodeId,
    store: &ValueStore,
) -> Vec<NodeId> {
    nodes[id.index()]
        .dependencies()
        .iter()
        .copied()
        .filter(|id| !store.contains(*id))
        .collect()
}

fn next_branch(branches: &[ParallelBranch], cursors: &[usize], active: &[bool]) -> Option<usize> {
    branches
        .iter()
        .enumerate()
        .position(|(index, branch)| !active[index] && cursors[index] < branch.nodes.len())
}

struct ExecutionState<E> {
    store: ValueStore,
    started: Vec<bool>,
    completed: Vec<NodeId>,
    errors: Vec<ExecuteError<E>>,
}

pub async fn execute<C, T, E, L>(
    plan: &Plan<C, T, E>,
    context: &C,
    listener: &mut L,
) -> Result<T, ConcurrentExecuteError<E>>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
    L: ProgressListener<E> + ?Sized,
{
    let mut state = ExecutionState {
        store: ValueStore::new(plan.owner),
        started: vec![false; plan.nodes.len()],
        completed: Vec::new(),
        errors: Vec::new(),
    };
    listener.on_event(ProgressEvent::PlanStarted {
        mode: ProgressMode::Execute,
        plan_name: plan.name(),
        total_nodes: plan.nodes.len(),
    });
    for step in &plan.execution {
        match step {
            ExecutionStep::Node { node } => {
                let branch = ParallelBranch {
                    name: String::new(),
                    nodes: vec![*node],
                };
                execute_region(&plan.nodes, context, &[branch], 1, &mut state, listener).await;
            }
            ExecutionStep::Parallel { group } => {
                execute_region(
                    &plan.nodes,
                    context,
                    &group.branches,
                    group.max_concurrency,
                    &mut state,
                    listener,
                )
                .await;
            }
        }
        if !state.errors.is_empty() {
            break;
        }
    }
    if state.errors.is_empty() {
        match state.store.require(plan.output) {
            Ok(value) => {
                listener.on_event(ProgressEvent::PlanFinished {
                    mode: ProgressMode::Execute,
                    plan_name: plan.name(),
                    total_nodes: plan.nodes.len(),
                    outcome: ProgressPlanOutcome::Complete,
                });
                return Ok(value);
            }
            Err(source) => state
                .errors
                .push(ExecuteError::Internal(InvariantError::FinalOutput {
                    source,
                })),
        }
    }
    listener.on_event(ProgressEvent::PlanFinished {
        mode: ProgressMode::Execute,
        plan_name: plan.name(),
        total_nodes: plan.nodes.len(),
        outcome: ProgressPlanOutcome::Failed,
    });
    let not_started = state
        .started
        .iter()
        .enumerate()
        .filter(|(_, started)| !**started)
        .map(|(index, _)| NodeId::new(index))
        .collect();
    Err(ConcurrentExecuteError::new(
        state.errors,
        state.completed,
        not_started,
    ))
}

async fn execute_region<'a, C, E, L>(
    nodes: &'a [Box<dyn ErasedNode<C, E>>],
    context: &'a C,
    branches: &[ParallelBranch],
    limit: usize,
    state: &mut ExecutionState<E>,
    listener: &mut L,
) where
    C: Sync + 'static,
    E: Send + 'static,
    L: ProgressListener<E> + ?Sized,
{
    let mut cursors = vec![0; branches.len()];
    let mut active = vec![false; branches.len()];
    let mut pending: FuturesUnordered<Pending<'a, E>> = FuturesUnordered::new();
    loop {
        while state.errors.is_empty() && pending.len() < limit {
            let Some(branch) = next_branch(branches, &cursors, &active) else {
                break;
            };
            let id = branches[branch].nodes[cursors[branch]];
            state.started[id.index()] = true;
            start(nodes, id, ProgressMode::Execute, listener);
            let unavailable = missing(nodes, id, &state.store);
            if !unavailable.is_empty() {
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: info(nodes, id),
                    outcome: ProgressOutcome::UnavailableDependencies {
                        missing_dependencies: &unavailable,
                    },
                });
                state.errors.push(ExecuteError::Internal(
                    InvariantError::UnavailableDependencies {
                        node: id,
                        missing_dependencies: unavailable,
                    },
                ));
                break;
            }
            let future = nodes[id.index()].run(context, &state.store);
            pending.push(Box::pin(async move { (branch, id, future.await) }));
            active[branch] = true;
        }
        let Some((branch, id, result)) = pending.next().await else {
            break;
        };
        active[branch] = false;
        cursors[branch] += 1;
        match result {
            Ok(value) => {
                state.store.insert_erased(id, value);
                state.completed.push(id);
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: info(nodes, id),
                    outcome: ProgressOutcome::Executed,
                });
            }
            Err(NodeRunError::Operation(source)) => {
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: info(nodes, id),
                    outcome: ProgressOutcome::Failed { error: &source },
                });
                state.errors.push(ExecuteError::Operation {
                    node: id,
                    name: nodes[id.index()].metadata().name().to_owned(),
                    source,
                });
            }
            Err(NodeRunError::Internal(source)) => {
                let error = InvariantError::Input { node: id, source };
                listener.on_event(ProgressEvent::NodeFinished {
                    mode: ProgressMode::Execute,
                    node: info(nodes, id),
                    outcome: ProgressOutcome::Internal { error: &error },
                });
                state.errors.push(ExecuteError::Internal(error));
            }
        }
    }
}

pub async fn dry_run<C, T, E, P, L>(
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
    let mut store = ValueStore::new(plan.owner);
    let mut rows: Vec<Option<NodeReport<E>>> = (0..plan.nodes.len()).map(|_| None).collect();
    listener.on_event(ProgressEvent::PlanStarted {
        mode: ProgressMode::DryRun,
        plan_name: plan.name(),
        total_nodes: plan.nodes.len(),
    });
    for step in &plan.execution {
        match step {
            ExecutionStep::Node { node } => {
                let branch = ParallelBranch {
                    name: String::new(),
                    nodes: vec![*node],
                };
                dry_region(
                    &plan.nodes,
                    context,
                    &[branch],
                    1,
                    &mut store,
                    &mut rows,
                    policy,
                    listener,
                )
                .await;
            }
            ExecutionStep::Parallel { group } => {
                dry_region(
                    &plan.nodes,
                    context,
                    &group.branches,
                    group.max_concurrency,
                    &mut store,
                    &mut rows,
                    policy,
                    listener,
                )
                .await;
            }
        }
    }
    let mut report = DryRunReport::new(plan.name());
    for row in rows {
        report.push(row.expect("concurrent rehearsal visits every node"));
    }
    listener.on_event(ProgressEvent::PlanFinished {
        mode: ProgressMode::DryRun,
        plan_name: plan.name(),
        total_nodes: plan.nodes.len(),
        outcome: ProgressPlanOutcome::from(report.status()),
    });
    report
}

// The coordinator is the sole owner of the store and listener. Running futures
// own resolved inputs, so committing another completion cannot invalidate them.
#[allow(clippy::too_many_arguments)]
async fn dry_region<'a, C, E, P, L>(
    nodes: &'a [Box<dyn ErasedNode<C, E>>],
    context: &'a C,
    branches: &[ParallelBranch],
    limit: usize,
    store: &mut ValueStore,
    rows: &mut [Option<NodeReport<E>>],
    policy: &P,
    listener: &mut L,
) where
    C: Sync + 'static,
    E: Send + 'static,
    P: DryRunPolicy,
    L: ProgressListener<E> + ?Sized,
{
    let mut cursors = vec![0; branches.len()];
    let mut active = vec![false; branches.len()];
    let mut pending: FuturesUnordered<Pending<'a, E>> = FuturesUnordered::new();
    loop {
        while pending.len() < limit {
            let Some(branch) = next_branch(branches, &cursors, &active) else {
                break;
            };
            let id = branches[branch].nodes[cursors[branch]];
            start(nodes, id, ProgressMode::DryRun, listener);
            // Policy is intentionally checked before dependency availability.
            let outcome = match policy.action(nodes[id.index()].metadata()) {
                DryRunAction::Skip => Some(NodeOutcome::Skipped {
                    reason: format!("{} operation", nodes[id.index()].metadata().impact()),
                }),
                DryRunAction::Deny => Some(NodeOutcome::Denied {
                    reason: format!("{} operation", nodes[id.index()].metadata().impact()),
                }),
                DryRunAction::Run => {
                    let unavailable = missing(nodes, id, store);
                    if unavailable.is_empty() {
                        None
                    } else {
                        Some(NodeOutcome::Blocked {
                            missing_dependencies: unavailable,
                        })
                    }
                }
            };
            if let Some(outcome) = outcome {
                record(nodes, id, outcome, rows, listener);
                cursors[branch] += 1;
            } else {
                let future = nodes[id.index()].run(context, store);
                pending.push(Box::pin(async move { (branch, id, future.await) }));
                active[branch] = true;
            }
        }
        let Some((branch, id, result)) = pending.next().await else {
            break;
        };
        active[branch] = false;
        cursors[branch] += 1;
        let outcome = match result {
            Ok(value) => {
                store.insert_erased(id, value);
                NodeOutcome::Executed
            }
            Err(NodeRunError::Operation(error)) => NodeOutcome::Failed { error },
            Err(NodeRunError::Internal(source)) => NodeOutcome::Internal {
                error: InvariantError::Input { node: id, source },
            },
        };
        record(nodes, id, outcome, rows, listener);
    }
}

fn record<C, E, L: ProgressListener<E> + ?Sized>(
    nodes: &[Box<dyn ErasedNode<C, E>>],
    id: NodeId,
    outcome: NodeOutcome<E>,
    rows: &mut [Option<NodeReport<E>>],
    listener: &mut L,
) {
    let progress = match &outcome {
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
    };
    listener.on_event(ProgressEvent::NodeFinished {
        mode: ProgressMode::DryRun,
        node: info(nodes, id),
        outcome: progress,
    });
    rows[id.index()] = Some(NodeReport::new(id, nodes[id.index()].metadata(), outcome));
}

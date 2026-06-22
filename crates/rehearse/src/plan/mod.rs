mod builder;
pub(crate) mod node;
pub mod store;
mod value;

pub use builder::PlanBuilder;
pub use value::{Input, IntoInput, NodeId, OperationInputs, Value};

use crate::runner;
use crate::{DryRunPolicy, DryRunReport, ExecuteError, PlanDescription, SafeDryRun};
use node::ErasedNode;
use std::marker::PhantomData;

pub struct Plan<C, T, E> {
    pub(crate) name: String,
    pub(crate) nodes: Vec<Box<dyn ErasedNode<C, E>>>,
    pub(crate) output: Value<T>,
    pub(crate) _marker: PhantomData<fn() -> (C, E)>,
}

impl<C, T, E> Plan<C, T, E>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
{
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn output(&self) -> Value<T> {
        self.output
    }

    pub fn nodes(&self) -> impl Iterator<Item = PlanNode<'_>> {
        self.nodes.iter().map(|node| PlanNode {
            id: node.id(),
            name: node.metadata().name(),
            impact: node.metadata().impact(),
            dependencies: node.dependencies(),
        })
    }

    pub fn describe(&self) -> PlanDescription {
        self.describe_with_policy(&SafeDryRun)
    }

    pub fn describe_with_policy<P>(&self, policy: &P) -> PlanDescription
    where
        P: DryRunPolicy,
    {
        crate::describe::describe_plan(&self.name, &self.nodes, policy)
    }

    pub async fn execute(&self, context: &C) -> Result<T, ExecuteError<E>> {
        runner::execute::execute(self, context).await
    }

    pub async fn dry_run(&self, context: &C) -> DryRunReport<E> {
        self.dry_run_with_policy(context, &SafeDryRun).await
    }

    pub async fn dry_run_with_policy<P>(&self, context: &C, policy: &P) -> DryRunReport<E>
    where
        P: DryRunPolicy,
    {
        runner::dry_run::dry_run(self, context, policy).await
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlanNode<'a> {
    id: NodeId,
    name: &'a str,
    impact: crate::Impact,
    dependencies: &'a [NodeId],
}

impl<'a> PlanNode<'a> {
    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn impact(&self) -> crate::Impact {
        self.impact
    }

    pub fn dependencies(&self) -> &'a [NodeId] {
        self.dependencies
    }
}

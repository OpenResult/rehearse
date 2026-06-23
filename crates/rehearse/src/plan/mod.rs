mod builder;
pub(crate) mod node;
#[doc(hidden)]
pub mod store;
mod value;

pub use builder::PlanBuilder;
pub use value::{Input, IntoInput, NodeId, OperationInputs, Value};

use crate::runner;
use crate::{
    DryRunPolicy, DryRunReport, ExecuteError, PlanDescription, PlanExecutionDescription,
    ProgressListener, SafeDryRun,
};
use node::ErasedNode;
use std::marker::PhantomData;

/// A reusable ordered operation plan.
///
/// `C` is the shared context type, `T` is the final output type, and `E` is the
/// common operation error type for the plan.
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
    /// Returns the plan name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the number of nodes in the plan.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true when the plan contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the typed final output handle.
    pub fn output(&self) -> Value<T> {
        self.output
    }

    /// Iterates static node metadata in plan order.
    pub fn nodes(&self) -> impl Iterator<Item = PlanNode<'_>> {
        self.nodes.iter().map(|node| PlanNode {
            id: node.id(),
            name: node.metadata().name(),
            impact: node.metadata().impact(),
            dependencies: node.dependencies(),
        })
    }

    /// Renders a Mermaid flowchart from static plan metadata.
    ///
    /// Plan order is shown in node labels. Edges are derived only from explicit
    /// value dependencies.
    pub fn to_mermaid(&self) -> String {
        let mut output = String::new();
        output.push_str("flowchart TD\n");
        output.push_str(&format!(
            "  %% plan: {}\n",
            escape_mermaid_comment(&self.name)
        ));

        for (index, node) in self.nodes.iter().enumerate() {
            let metadata = node.metadata();
            output.push_str(&format!(
                "  n{}[\"{}. {}\\n{}\"]\n",
                node.id().index(),
                index + 1,
                escape_mermaid_label(metadata.name()),
                metadata.impact()
            ));
        }

        for node in &self.nodes {
            for dependency in node.dependencies() {
                output.push_str(&format!(
                    "  n{} --> n{}\n",
                    dependency.index(),
                    node.id().index()
                ));
            }
        }

        output
    }

    /// Describes the plan using [`SafeDryRun`].
    pub fn describe(&self) -> PlanDescription {
        self.describe_with_policy(&SafeDryRun)
    }

    /// Describes the plan using [`SafeDryRun`] and reports progress events.
    pub fn describe_with_listener<L>(&self, listener: &mut L) -> PlanDescription
    where
        L: ProgressListener<E> + ?Sized,
    {
        self.describe_with_policy_and_listener(&SafeDryRun, listener)
    }

    /// Describes the plan using a caller-supplied dry-run policy.
    pub fn describe_with_policy<P>(&self, policy: &P) -> PlanDescription
    where
        P: DryRunPolicy,
    {
        crate::describe::describe_plan(&self.name, &self.nodes, policy)
    }

    /// Describes the plan using a caller-supplied dry-run policy and reports
    /// progress events.
    pub fn describe_with_policy_and_listener<P, L>(
        &self,
        policy: &P,
        listener: &mut L,
    ) -> PlanDescription
    where
        P: DryRunPolicy,
        L: ProgressListener<E> + ?Sized,
    {
        crate::describe::describe_plan_with_listener(&self.name, &self.nodes, policy, listener)
    }

    /// Describes execute-mode plan order without dry-run policy actions.
    pub fn describe_execution(&self) -> PlanExecutionDescription {
        crate::describe::describe_execution_plan(&self.name, &self.nodes)
    }

    /// Describes execute-mode plan order and reports progress events.
    pub fn describe_execution_with_listener<L>(&self, listener: &mut L) -> PlanExecutionDescription
    where
        L: ProgressListener<E> + ?Sized,
    {
        crate::describe::describe_execution_plan_with_listener(&self.name, &self.nodes, listener)
    }

    /// Executes every operation in plan order and stops at the first failure.
    pub async fn execute(&self, context: &C) -> Result<T, ExecuteError<E>> {
        runner::execute::execute(self, context).await
    }

    /// Executes every operation in plan order, reports progress events, and
    /// stops at the first failure.
    pub async fn execute_with_listener<L>(
        &self,
        context: &C,
        listener: &mut L,
    ) -> Result<T, ExecuteError<E>>
    where
        L: ProgressListener<E> + ?Sized,
    {
        runner::execute::execute_with_listener(self, context, listener).await
    }

    /// Runs the plan with [`SafeDryRun`].
    pub async fn dry_run(&self, context: &C) -> DryRunReport<E> {
        self.dry_run_with_policy(context, &SafeDryRun).await
    }

    /// Runs the plan with [`SafeDryRun`] and reports progress events.
    pub async fn dry_run_with_listener<L>(&self, context: &C, listener: &mut L) -> DryRunReport<E>
    where
        L: ProgressListener<E> + ?Sized,
    {
        self.dry_run_with_policy_and_listener(context, &SafeDryRun, listener)
            .await
    }

    /// Runs the plan with a caller-supplied dry-run policy.
    pub async fn dry_run_with_policy<P>(&self, context: &C, policy: &P) -> DryRunReport<E>
    where
        P: DryRunPolicy,
    {
        runner::dry_run::dry_run(self, context, policy).await
    }

    /// Runs the plan with a caller-supplied dry-run policy and reports progress
    /// events.
    pub async fn dry_run_with_policy_and_listener<P, L>(
        &self,
        context: &C,
        policy: &P,
        listener: &mut L,
    ) -> DryRunReport<E>
    where
        P: DryRunPolicy,
        L: ProgressListener<E> + ?Sized,
    {
        runner::dry_run::dry_run_with_listener(self, context, policy, listener).await
    }
}

fn escape_mermaid_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_mermaid_comment(value: &str) -> String {
    value.replace('\n', " ")
}

/// Static metadata view for one plan node.
#[derive(Debug, Clone, Copy)]
pub struct PlanNode<'a> {
    id: NodeId,
    name: &'a str,
    impact: crate::Impact,
    dependencies: &'a [NodeId],
}

impl<'a> PlanNode<'a> {
    /// Returns the node id.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Returns the operation name.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the operation impact.
    pub fn impact(&self) -> crate::Impact {
        self.impact
    }

    /// Returns value dependencies by producer node id.
    pub fn dependencies(&self) -> &'a [NodeId] {
        self.dependencies
    }
}

use crate::plan::store::ValueStore;
use crate::{Impact, NodeId, OperationInputs};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Runtime-independent boxed future used by operation executors.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Static metadata attached to an operation node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OperationMetadata {
    name: String,
    impact: Impact,
}

impl OperationMetadata {
    /// Creates operation metadata from a display name and declared impact.
    pub fn new(name: impl Into<String>, impact: Impact) -> Self {
        Self {
            name: name.into(),
            impact,
        }
    }

    /// Returns the operation name used in descriptions, reports, and errors.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the operation's declared impact.
    pub fn impact(&self) -> Impact {
        self.impact
    }
}

/// A delayed operation descriptor.
///
/// Constructing an `Operation` records metadata, inputs, and the executor
/// closure. The executor body runs only through
/// [`Plan::execute`](crate::Plan::execute) or dry-run when policy allows it.
pub struct Operation<C, T, E> {
    metadata: OperationMetadata,
    dependencies: Vec<NodeId>,
    runner: Box<ErasedRunner<C, T, E>>,
}

type ErasedRunner<C, T, E> = dyn for<'a> Fn(&'a C, &'a ValueStore) -> BoxFuture<'a, Result<T, NodeRunError<E>>>
    + Send
    + Sync;

impl<C, T, E> Operation<C, T, E>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
{
    /// Creates a new operation descriptor.
    ///
    /// `inputs` can be `()`, one [`Input`](crate::Input), or tuples up to eight
    /// inputs. Value inputs are resolved from the per-run store before the
    /// executor is invoked.
    pub fn new<I, F>(metadata: OperationMetadata, inputs: I, executor: F) -> Self
    where
        I: OperationInputs,
        F: for<'a> Fn(&'a C, I::Resolved) -> BoxFuture<'a, Result<T, E>> + Send + Sync + 'static,
    {
        let dependencies = inputs.dependencies();
        let executor = Arc::new(executor);
        let runner: Box<ErasedRunner<C, T, E>> =
            Box::new(move |context: &C, store: &ValueStore| {
                let resolved = inputs.resolve(store);
                let executor = Arc::clone(&executor);
                Box::pin(async move {
                    let resolved =
                        resolved.map_err(|error| NodeRunError::Internal(error.to_string()))?;
                    executor(context, resolved)
                        .await
                        .map_err(NodeRunError::Operation)
                })
            });

        Self {
            metadata,
            dependencies,
            runner,
        }
    }

    /// Creates a new operation descriptor from synchronous work.
    ///
    /// This is a convenience wrapper over [`Operation::new`] for operations
    /// whose implementation does not need to await internally. The operation is
    /// still executed through the same async plan runners.
    pub fn sync<I, F>(metadata: OperationMetadata, inputs: I, executor: F) -> Self
    where
        I: OperationInputs,
        F: Fn(&C, I::Resolved) -> Result<T, E> + Send + Sync + 'static,
    {
        let executor = Arc::new(executor);
        Self::new(metadata, inputs, move |context: &C, resolved| {
            let executor = Arc::clone(&executor);
            Box::pin(async move { executor(context, resolved) })
        })
    }

    /// Returns this operation's static metadata.
    pub fn metadata(&self) -> &OperationMetadata {
        &self.metadata
    }

    /// Returns node ids for value inputs consumed by this operation.
    pub fn dependencies(&self) -> &[NodeId] {
        &self.dependencies
    }

    pub(crate) fn run<'a>(
        &'a self,
        context: &'a C,
        store: &'a ValueStore,
    ) -> BoxFuture<'a, Result<T, NodeRunError<E>>> {
        (self.runner)(context, store)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeRunError<E> {
    Operation(E),
    Internal(String),
}

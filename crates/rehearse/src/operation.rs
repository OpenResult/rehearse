use crate::plan::store::ValueStore;
use crate::{Impact, NodeId, OperationInputs};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationMetadata {
    name: String,
    impact: Impact,
}

impl OperationMetadata {
    pub fn new(name: impl Into<String>, impact: Impact) -> Self {
        Self {
            name: name.into(),
            impact,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn impact(&self) -> Impact {
        self.impact
    }
}

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

    pub fn metadata(&self) -> &OperationMetadata {
        &self.metadata
    }

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

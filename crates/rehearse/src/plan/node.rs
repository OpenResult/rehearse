use super::store::{StoredValue, ValueStore};
use super::NodeId;
use crate::operation::NodeRunError;
use crate::{BoxFuture, Operation, OperationMetadata};
use std::marker::PhantomData;

pub(crate) trait ErasedNode<C, E>: Send + Sync {
    fn id(&self) -> NodeId;
    fn metadata(&self) -> &OperationMetadata;
    fn dependencies(&self) -> &[NodeId];
    fn run<'a>(
        &'a self,
        context: &'a C,
        store: &'a ValueStore,
    ) -> BoxFuture<'a, Result<StoredValue, NodeRunError<E>>>;
}

pub(crate) struct TypedNode<C, T, E> {
    id: NodeId,
    operation: Operation<C, T, E>,
    _marker: PhantomData<fn() -> (C, E)>,
}

impl<C, T, E> TypedNode<C, T, E> {
    pub(crate) fn new(id: NodeId, operation: Operation<C, T, E>) -> Self {
        Self {
            id,
            operation,
            _marker: PhantomData,
        }
    }
}

impl<C, T, E> ErasedNode<C, E> for TypedNode<C, T, E>
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
{
    fn id(&self) -> NodeId {
        self.id
    }

    fn metadata(&self) -> &OperationMetadata {
        self.operation.metadata()
    }

    fn dependencies(&self) -> &[NodeId] {
        self.operation.dependencies()
    }

    fn run<'a>(
        &'a self,
        context: &'a C,
        store: &'a ValueStore,
    ) -> BoxFuture<'a, Result<StoredValue, NodeRunError<E>>> {
        Box::pin(async move {
            let output = self.operation.run(context, store).await?;
            Ok(ValueStore::erase(output))
        })
    }
}

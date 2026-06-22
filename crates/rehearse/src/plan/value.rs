use super::store::{ResolveInputError, ValueStore};
use std::fmt;
use std::marker::PhantomData;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(usize);

impl NodeId {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Value<T> {
    node: NodeId,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Copy for Value<T> {}

impl<T> Clone for Value<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Value<T> {
    pub(crate) fn new(node: NodeId) -> Self {
        Self {
            node,
            _marker: PhantomData,
        }
    }

    pub fn node(self) -> NodeId {
        self.node
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input<T> {
    Literal(T),
    Value(Value<T>),
}

impl<T> Input<T> {
    pub fn literal(value: T) -> Self {
        Self::Literal(value)
    }

    pub fn value(value: Value<T>) -> Self {
        Self::Value(value)
    }
}

pub trait IntoInput<T> {
    fn into_input(self) -> Input<T>;
}

impl<T> IntoInput<T> for Input<T> {
    fn into_input(self) -> Input<T> {
        self
    }
}

impl<T> IntoInput<T> for Value<T> {
    fn into_input(self) -> Input<T> {
        Input::Value(self)
    }
}

impl<T> IntoInput<T> for T {
    fn into_input(self) -> Input<T> {
        Input::Literal(self)
    }
}

pub trait OperationInputs: Send + Sync + 'static {
    type Resolved: Send + 'static;

    #[doc(hidden)]
    fn dependencies(&self) -> Vec<NodeId>;

    #[doc(hidden)]
    fn missing_dependencies(&self, store: &ValueStore) -> Vec<NodeId> {
        self.dependencies()
            .into_iter()
            .filter(|dependency| !store.contains(*dependency))
            .collect()
    }

    #[doc(hidden)]
    fn resolve(&self, store: &ValueStore) -> Result<Self::Resolved, ResolveInputError>;
}

impl OperationInputs for () {
    type Resolved = ();

    fn dependencies(&self) -> Vec<NodeId> {
        Vec::new()
    }

    fn resolve(&self, _store: &ValueStore) -> Result<Self::Resolved, ResolveInputError> {
        Ok(())
    }
}

impl<T> OperationInputs for Input<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Resolved = T;

    fn dependencies(&self) -> Vec<NodeId> {
        match self {
            Self::Literal(_) => Vec::new(),
            Self::Value(value) => vec![value.node()],
        }
    }

    fn resolve(&self, store: &ValueStore) -> Result<Self::Resolved, ResolveInputError> {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::Value(value) => store.require(*value),
        }
    }
}

impl<A, B> OperationInputs for (Input<A>, Input<B>)
where
    A: Clone + Send + Sync + 'static,
    B: Clone + Send + Sync + 'static,
{
    type Resolved = (A, B);

    fn dependencies(&self) -> Vec<NodeId> {
        let mut dependencies = self.0.dependencies();
        dependencies.extend(self.1.dependencies());
        dependencies
    }

    fn resolve(&self, store: &ValueStore) -> Result<Self::Resolved, ResolveInputError> {
        Ok((self.0.resolve(store)?, self.1.resolve(store)?))
    }
}

impl<A, B, C> OperationInputs for (Input<A>, Input<B>, Input<C>)
where
    A: Clone + Send + Sync + 'static,
    B: Clone + Send + Sync + 'static,
    C: Clone + Send + Sync + 'static,
{
    type Resolved = (A, B, C);

    fn dependencies(&self) -> Vec<NodeId> {
        let mut dependencies = self.0.dependencies();
        dependencies.extend(self.1.dependencies());
        dependencies.extend(self.2.dependencies());
        dependencies
    }

    fn resolve(&self, store: &ValueStore) -> Result<Self::Resolved, ResolveInputError> {
        Ok((
            self.0.resolve(store)?,
            self.1.resolve(store)?,
            self.2.resolve(store)?,
        ))
    }
}

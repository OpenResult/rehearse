use super::store::{ResolveInputError, ValueStore};
use std::fmt;
use std::marker::PhantomData;

/// Stable identifier for a node in one plan.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeId(usize);

impl NodeId {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    /// Returns the zero-based node index.
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

/// Typed handle to a value produced by a plan node.
///
/// `Value<T>` is copyable regardless of `T`; it stores only the producing node
/// id and a type marker.
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

    /// Returns the producing node id.
    pub fn node(self) -> NodeId {
        self.node
    }
}

/// Operation input: either a literal plan-time value or a value from a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input<T> {
    /// Literal value cloned into the executor when the operation runs.
    Literal(T),
    /// Handle to a value produced by an earlier node.
    Value(Value<T>),
}

impl<T> Input<T> {
    /// Creates a literal input.
    pub fn literal(value: T) -> Self {
        Self::Literal(value)
    }

    /// Creates a value dependency input.
    pub fn value(value: Value<T>) -> Self {
        Self::Value(value)
    }
}

/// Converts values accepted by generated operation constructors into [`Input`].
pub trait IntoInput<T> {
    /// Converts this value into an operation input.
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

/// Internal input tuple abstraction used by [`Operation`](crate::Operation).
///
/// Implemented for `()`, one [`Input`], and tuples of up to eight inputs.
pub trait OperationInputs: Send + Sync + 'static {
    /// Resolved value shape passed to an operation executor.
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

macro_rules! impl_operation_inputs_tuple {
    ($($name:ident $index:tt),+ $(,)?) => {
        impl<$($name),+> OperationInputs for ($(Input<$name>,)+)
        where
            $($name: Clone + Send + Sync + 'static,)+
        {
            type Resolved = ($($name,)+);

            fn dependencies(&self) -> Vec<NodeId> {
                let mut dependencies = Vec::new();
                $(
                    dependencies.extend(self.$index.dependencies());
                )+
                dependencies
            }

            fn resolve(&self, store: &ValueStore) -> Result<Self::Resolved, ResolveInputError> {
                Ok((
                    $(
                        self.$index.resolve(store)?,
                    )+
                ))
            }
        }
    };
}

impl_operation_inputs_tuple!(A 0, B 1);
impl_operation_inputs_tuple!(A 0, B 1, C 2);
impl_operation_inputs_tuple!(A 0, B 1, C 2, D 3);
impl_operation_inputs_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_operation_inputs_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_operation_inputs_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_operation_inputs_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

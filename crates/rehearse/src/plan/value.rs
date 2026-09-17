use super::store::ValueStore;
use crate::ValueError;
use std::any::{type_name, TypeId};
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

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

// Identities are never recycled, including after a builder is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PlanId(usize);

impl PlanId {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        Self(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("plan identity space exhausted"),
        )
    }
}

// Public only inside this private module, for the sealed input trait.
#[derive(Debug, Clone, Copy)]
pub struct Dependency {
    pub(crate) owner: PlanId,
    pub(crate) node: NodeId,
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
}

/// Typed handle to a value produced by a plan node.
///
/// `Value<T>` is copyable regardless of `T`; it stores only the producing node
/// id, private plan identity, and a type marker.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Value<T> {
    pub(crate) owner: PlanId,
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
    pub(crate) fn new(owner: PlanId, node: NodeId) -> Self {
        Self {
            owner,
            node,
            _marker: PhantomData,
        }
    }

    /// Returns the producing node id.
    pub fn node(self) -> NodeId {
        self.node
    }
}

impl<T: 'static> Value<T> {
    pub(crate) fn dependency(self) -> Dependency {
        Dependency {
            owner: self.owner,
            node: self.node,
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
        }
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

/// Supported input shapes for [`Operation`](crate::Operation).
///
/// This trait is sealed. Implemented for `()`, one [`Input`], and tuples of
/// two to eight inputs. Custom implementations are not supported.
pub trait OperationInputs: sealed::Resolve<Self::Resolved> + Send + Sync + 'static {
    /// Resolved value shape passed to an operation executor.
    type Resolved: Send + 'static;
}

pub(crate) mod sealed {
    use super::{Dependency, ValueError, ValueStore};

    pub trait Resolve<R> {
        fn references(&self) -> Vec<Dependency>;
        fn resolve(&self, store: &ValueStore) -> Result<R, ValueError>;
    }
}

use sealed::Resolve;

impl OperationInputs for () {
    type Resolved = ();
}

impl Resolve<()> for () {
    fn references(&self) -> Vec<Dependency> {
        Vec::new()
    }
    fn resolve(&self, _store: &ValueStore) -> Result<(), ValueError> {
        Ok(())
    }
}

impl<T: Clone + Send + Sync + 'static> OperationInputs for Input<T> {
    type Resolved = T;
}

impl<T: Clone + Send + Sync + 'static> Resolve<T> for Input<T> {
    fn references(&self) -> Vec<Dependency> {
        match self {
            Self::Literal(_) => Vec::new(),
            Self::Value(value) => vec![value.dependency()],
        }
    }

    fn resolve(&self, store: &ValueStore) -> Result<T, ValueError> {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::Value(value) => store.require(*value),
        }
    }
}

macro_rules! impl_operation_inputs_tuple {
    ($($name:ident $index:tt),+ $(,)?) => {
        impl<$($name: Clone + Send + Sync + 'static),+> OperationInputs for ($(Input<$name>,)+) {
            type Resolved = ($($name,)+);
        }

        impl<$($name: Clone + Send + Sync + 'static),+> Resolve<($($name,)+)> for ($(Input<$name>,)+) {
            fn references(&self) -> Vec<Dependency> {
                let mut dependencies = Vec::new();
                $(dependencies.extend(self.$index.references());)+
                dependencies
            }

            fn resolve(&self, store: &ValueStore) -> Result<($($name,)+), ValueError> {
                Ok(($(self.$index.resolve(store)?,)+))
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

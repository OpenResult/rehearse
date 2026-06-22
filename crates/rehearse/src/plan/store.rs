use super::{NodeId, Value};
use std::any::{type_name, Any};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

pub type StoredValue = Arc<dyn Any + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveInputError {
    Missing(NodeId),
    TypeMismatch {
        node: NodeId,
        expected: &'static str,
    },
}

impl fmt::Display for ResolveInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(node) => write!(f, "required value from node {node} is unavailable"),
            Self::TypeMismatch { node, expected } => write!(
                f,
                "stored value for node {node} could not be downcast to {expected}"
            ),
        }
    }
}

impl std::error::Error for ResolveInputError {}

#[derive(Debug, Default)]
pub struct ValueStore {
    values: HashMap<NodeId, StoredValue>,
}

impl ValueStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, node: NodeId) -> bool {
        self.values.contains_key(&node)
    }

    pub fn insert<T>(&mut self, node: NodeId, value: T)
    where
        T: Clone + Send + Sync + 'static,
    {
        self.values.insert(node, Arc::new(value));
    }

    pub fn insert_erased(&mut self, node: NodeId, value: StoredValue) {
        self.values.insert(node, value);
    }

    pub fn erase<T>(value: T) -> StoredValue
    where
        T: Clone + Send + Sync + 'static,
    {
        Arc::new(value)
    }

    pub fn get<T>(&self, value: Value<T>) -> Result<Option<T>, ResolveInputError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let Some(stored) = self.values.get(&value.node()) else {
            return Ok(None);
        };
        let typed =
            Arc::clone(stored)
                .downcast::<T>()
                .map_err(|_| ResolveInputError::TypeMismatch {
                    node: value.node(),
                    expected: type_name::<T>(),
                })?;
        Ok(Some((*typed).clone()))
    }

    pub fn require<T>(&self, value: Value<T>) -> Result<T, ResolveInputError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get(value)?
            .ok_or_else(|| ResolveInputError::Missing(value.node()))
    }
}

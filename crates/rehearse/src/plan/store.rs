use super::value::PlanId;
use super::{NodeId, Value};
use crate::ValueError;
use std::any::{type_name, Any};
use std::collections::HashMap;
use std::sync::Arc;

pub type StoredValue = Arc<dyn Any + Send + Sync>;

#[derive(Debug)]
pub struct ValueStore {
    owner: PlanId,
    values: HashMap<NodeId, StoredValue>,
}

impl ValueStore {
    pub(crate) fn new(owner: PlanId) -> Self {
        Self {
            owner,
            values: HashMap::new(),
        }
    }

    pub fn contains(&self, node: NodeId) -> bool {
        self.values.contains_key(&node)
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

    pub fn get<T>(&self, value: Value<T>) -> Result<Option<T>, ValueError>
    where
        T: Clone + Send + Sync + 'static,
    {
        if value.owner != self.owner {
            return Err(ValueError::ForeignPlan { node: value.node() });
        }
        let Some(stored) = self.values.get(&value.node()) else {
            return Ok(None);
        };
        let typed = Arc::clone(stored)
            .downcast::<T>()
            .map_err(|_| ValueError::TypeMismatch {
                node: value.node(),
                expected: type_name::<T>().to_owned(),
            })?;
        Ok(Some((*typed).clone()))
    }

    pub fn require<T>(&self, value: Value<T>) -> Result<T, ValueError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get(value)?
            .ok_or_else(|| ValueError::Missing { node: value.node() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_checks_owner_availability_and_type() {
        let owner = PlanId::new();
        let mut store = ValueStore::new(owner);
        let node = NodeId::new(0);
        let value = Value::<u32>::new(owner, node);
        assert_eq!(store.require(value), Err(ValueError::Missing { node }));
        store.insert_erased(node, ValueStore::erase("wrong type"));
        assert!(matches!(
            store.require(value),
            Err(ValueError::TypeMismatch { .. })
        ));
        store.insert_erased(node, ValueStore::erase(42_u32));
        assert_eq!(store.require(value), Ok(42));
        assert_eq!(
            store.require(Value::<u32>::new(PlanId::new(), node)),
            Err(ValueError::ForeignPlan { node })
        );
    }
}

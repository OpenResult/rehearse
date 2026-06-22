use super::node::{ErasedNode, TypedNode};
use super::{NodeId, Plan, Value};
use crate::Operation;
use std::marker::PhantomData;

/// Manual builder for ordered plans.
pub struct PlanBuilder<C, E> {
    name: String,
    nodes: Vec<Box<dyn ErasedNode<C, E>>>,
    _marker: PhantomData<fn() -> (C, E)>,
}

impl<C, E> PlanBuilder<C, E> {
    /// Creates an empty plan builder with the supplied plan name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            nodes: Vec::new(),
            _marker: PhantomData,
        }
    }
}

impl<C, E> PlanBuilder<C, E>
where
    C: Sync + 'static,
    E: Send + 'static,
{
    /// Adds an operation as the next plan node.
    ///
    /// The returned [`Value<T>`](Value) is a typed handle to the operation's
    /// eventual output. Adding an operation does not invoke its body.
    pub fn add<T>(&mut self, operation: Operation<C, T, E>) -> Value<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let id = NodeId::new(self.nodes.len());
        let value = Value::new(id);
        self.nodes.push(Box::new(TypedNode::new(id, operation)));
        value
    }

    /// Finishes the builder and selects the final plan output.
    pub fn finish<T>(self, output: Value<T>) -> Plan<C, T, E>
    where
        T: Clone + Send + Sync + 'static,
    {
        Plan {
            name: self.name,
            nodes: self.nodes,
            output,
            _marker: PhantomData,
        }
    }
}

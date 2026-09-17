use super::node::{ErasedNode, TypedNode};
use super::value::{Dependency, PlanId};
use super::{NodeId, Plan, Value};
use crate::{Operation, PlanBuildError};
use std::marker::PhantomData;

/// Manual builder for ordered plans.
pub struct PlanBuilder<C, E> {
    owner: PlanId,
    name: String,
    nodes: Vec<Box<dyn ErasedNode<C, E>>>,
    _marker: PhantomData<fn() -> (C, E)>,
}

impl<C, E> PlanBuilder<C, E> {
    /// Creates an empty plan builder with the supplied plan name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            owner: PlanId::new(),
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
        let value = Value::new(self.owner, id);
        self.nodes.push(Box::new(TypedNode::new(id, operation)));
        value
    }

    /// Validates the plan and selects its final output.
    ///
    /// # Panics
    /// Panics if a dependency or output does not belong to this builder or is
    /// otherwise invalid. Use [`Self::try_finish`] for recoverable errors.
    #[track_caller]
    pub fn finish<T>(self, output: Value<T>) -> Plan<C, T, E>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.try_finish(output)
            .unwrap_or_else(|error| panic!("invalid plan: {error}"))
    }

    /// Validates ownership, producer order, and types without invoking bodies
    /// or resolving inputs, then selects the final output.
    pub fn try_finish<T>(self, output: Value<T>) -> Result<Plan<C, T, E>, PlanBuildError>
    where
        T: Clone + Send + Sync + 'static,
    {
        for node in &self.nodes {
            for dependency in node.references() {
                self.validate(*dependency, Some(node.id()))?;
            }
        }
        self.validate(output.dependency(), None)?;
        Ok(Plan {
            owner: self.owner,
            name: self.name,
            nodes: self.nodes,
            output,
            _marker: PhantomData,
        })
    }

    fn validate(
        &self,
        reference: Dependency,
        consumer: Option<NodeId>,
    ) -> Result<(), PlanBuildError> {
        let node = reference.node;
        if reference.owner != self.owner {
            return Err(PlanBuildError::ForeignValue { node, consumer });
        }
        let producer = self
            .nodes
            .get(node.index())
            .ok_or(PlanBuildError::UnknownValue { node, consumer })?;
        if let Some(consumer) = consumer {
            if node.index() >= consumer.index() {
                return Err(PlanBuildError::InvalidOrder { node, consumer });
            }
        }
        let (actual_id, actual) = producer.output_type();
        if actual_id != reference.type_id {
            return Err(PlanBuildError::TypeMismatch {
                node,
                consumer,
                expected: reference.type_name.to_owned(),
                actual: actual.to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Impact, OperationMetadata};

    fn number() -> Operation<(), u32, ()> {
        Operation::sync(
            OperationMetadata::new("number", Impact::Pure),
            (),
            |_, ()| panic!("validation must not execute"),
        )
    }

    #[test]
    fn rejects_missing_and_mistyped_outputs() {
        let builder = PlanBuilder::<(), ()>::new("missing");
        let output = Value::<u32>::new(builder.owner, NodeId::new(0));
        assert!(matches!(
            builder.try_finish(output),
            Err(PlanBuildError::UnknownValue { consumer: None, .. })
        ));
        let mut builder = PlanBuilder::<(), ()>::new("type");
        let value = builder.add(number());
        let wrong = Value::<String>::new(builder.owner, value.node());
        assert!(matches!(
            builder.try_finish(wrong),
            Err(PlanBuildError::TypeMismatch { consumer: None, .. })
        ));
    }

    #[test]
    fn rejects_forward_and_mistyped_dependencies() {
        for forward in [true, false] {
            let mut builder = PlanBuilder::<(), ()>::new("invalid");
            let reference =
                Value::<String>::new(builder.owner, NodeId::new(if forward { 1 } else { 0 }));
            if !forward {
                builder.add(number());
            }
            let output = builder.add(Operation::sync(
                OperationMetadata::new("consumer", Impact::Pure),
                crate::Input::value(reference),
                |_, _| Ok::<_, ()>(()),
            ));
            if forward {
                builder.add(number());
            }
            let error = builder
                .try_finish(output)
                .err()
                .expect("invalid dependency");
            if forward {
                assert!(matches!(error, PlanBuildError::InvalidOrder { .. }));
            } else {
                assert!(matches!(
                    error,
                    PlanBuildError::TypeMismatch {
                        consumer: Some(_),
                        ..
                    }
                ));
            }
        }
    }
}

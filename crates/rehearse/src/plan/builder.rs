use super::node::{ErasedNode, TypedNode};
use super::value::{Dependency, PlanId};
use super::{ExecutionStep, NodeId, ParallelBranch, ParallelGroup, Plan, Value};
use crate::{Operation, PlanBuildError};
use std::marker::PhantomData;
use std::num::NonZeroUsize;

/// Manual builder for ordered plans.
pub struct PlanBuilder<C, E> {
    owner: PlanId,
    name: String,
    nodes: Vec<Box<dyn ErasedNode<C, E>>>,
    execution: Vec<ExecutionStep>,
    revision: Option<String>,
    _marker: PhantomData<fn() -> (C, E)>,
}

impl<C, E> PlanBuilder<C, E> {
    /// Creates an empty plan builder with the supplied plan name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            owner: PlanId::new(),
            name: name.into(),
            nodes: Vec::new(),
            execution: Vec::new(),
            revision: None,
            _marker: PhantomData,
        }
    }

    /// Declares the application code/configuration revision used by graph consumers.
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
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
        self.execution.push(ExecutionStep::Node { node: id });
        value
    }

    /// Declares independent ordered branches and a barrier after all branches.
    ///
    /// This records structure only. `execute()` remains sequential; use
    /// `execute_concurrent()` to honor the declared concurrency bound.
    pub fn parallel<R>(
        &mut self,
        name: impl Into<String>,
        limit: NonZeroUsize,
        build: impl FnOnce(&mut ParallelBuilder<'_, C, E>) -> R,
    ) -> R {
        let mut group = ParallelBuilder {
            owner: self.owner,
            nodes: &mut self.nodes,
            branches: Vec::new(),
        };
        let result = build(&mut group);
        let branches = group.branches;
        self.execution.push(ExecutionStep::Parallel {
            group: ParallelGroup {
                id: self.execution.len(),
                name: name.into(),
                max_concurrency: limit.get(),
                branches,
            },
        });
        result
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
        self.validate_execution_coverage()?;
        self.validate_regions()?;
        Ok(Plan {
            owner: self.owner,
            name: self.name,
            nodes: self.nodes,
            execution: self.execution,
            revision: self.revision,
            output,
            _marker: PhantomData,
        })
    }

    // A callback can unwind after adding nodes. If its caller catches that
    // panic, no partial branch/region may silently disappear from execution.
    fn validate_execution_coverage(&self) -> Result<(), PlanBuildError> {
        let mut expected = 0;
        let mut visit = |node: NodeId| {
            if node.index() != expected {
                return Err(PlanBuildError::InvalidExecutionStructure {
                    node: NodeId::new(expected),
                });
            }
            expected += 1;
            Ok(())
        };
        for step in &self.execution {
            match step {
                ExecutionStep::Node { node } => visit(*node)?,
                ExecutionStep::Parallel { group } => {
                    for branch in &group.branches {
                        for &node in &branch.nodes {
                            visit(node)?;
                        }
                    }
                }
            }
        }
        if expected != self.nodes.len() {
            return Err(PlanBuildError::InvalidExecutionStructure {
                node: NodeId::new(expected),
            });
        }
        Ok(())
    }

    fn validate_regions(&self) -> Result<(), PlanBuildError> {
        for step in &self.execution {
            let ExecutionStep::Parallel { group } = step else {
                continue;
            };
            if group.branches.is_empty() {
                return Err(PlanBuildError::EmptyParallelGroup {
                    group: group.name.clone(),
                });
            }
            for (branch_index, branch) in group.branches.iter().enumerate() {
                if branch.nodes.is_empty() {
                    return Err(PlanBuildError::EmptyParallelBranch {
                        group: group.name.clone(),
                        branch: branch.name.clone(),
                    });
                }
                for &consumer in &branch.nodes {
                    for dependency in self.nodes[consumer.index()].dependencies() {
                        if group.branches.iter().enumerate().any(|(index, sibling)| {
                            index != branch_index && sibling.nodes.contains(dependency)
                        }) {
                            return Err(PlanBuildError::CrossBranchDependency {
                                node: *dependency,
                                consumer,
                                group: group.name.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
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

/// Builder for one explicit parallel region. Branches share their parent owner.
pub struct ParallelBuilder<'a, C, E> {
    owner: PlanId,
    nodes: &'a mut Vec<Box<dyn ErasedNode<C, E>>>,
    branches: Vec<ParallelBranch>,
}

impl<C: Sync + 'static, E: Send + 'static> ParallelBuilder<'_, C, E> {
    /// Declares an ordered branch. Branches may use preceding outer values,
    /// but sibling branch outputs cannot be consumed inside this region.
    pub fn branch<R>(
        &mut self,
        name: impl Into<String>,
        build: impl FnOnce(&mut BranchBuilder<'_, C, E>) -> R,
    ) -> R {
        let mut branch = BranchBuilder {
            owner: self.owner,
            nodes: self.nodes,
            ids: Vec::new(),
        };
        let result = build(&mut branch);
        self.branches.push(ParallelBranch {
            name: name.into(),
            nodes: branch.ids,
        });
        result
    }
}

/// Operation builder within an ordered parallel branch. Regions cannot nest.
pub struct BranchBuilder<'a, C, E> {
    owner: PlanId,
    nodes: &'a mut Vec<Box<dyn ErasedNode<C, E>>>,
    ids: Vec<NodeId>,
}
impl<C: Sync + 'static, E: Send + 'static> BranchBuilder<'_, C, E> {
    /// Adds a delayed operation to this branch.
    pub fn add<T: Clone + Send + Sync + 'static>(
        &mut self,
        operation: Operation<C, T, E>,
    ) -> Value<T> {
        let id = NodeId::new(self.nodes.len());
        self.nodes.push(Box::new(TypedNode::new(id, operation)));
        self.ids.push(id);
        Value::new(self.owner, id)
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

use crate::NodeId;
use std::error::Error;
use std::fmt;

/// Invalid value reference found while constructing a plan.
/// `consumer: None` identifies the selected final output.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum PlanBuildError {
    /// The value belongs to another builder.
    ForeignValue {
        node: NodeId,
        consumer: Option<NodeId>,
    },
    /// The producer does not exist in this builder.
    UnknownValue {
        node: NodeId,
        consumer: Option<NodeId>,
    },
    /// A producer must precede its consumer.
    InvalidOrder { node: NodeId, consumer: NodeId },
    /// The handle's type differs from the producer's output type.
    TypeMismatch {
        node: NodeId,
        consumer: Option<NodeId>,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for PlanBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (node, consumer) = match self {
            Self::ForeignValue { node, consumer }
            | Self::UnknownValue { node, consumer }
            | Self::TypeMismatch { node, consumer, .. } => (*node, *consumer),
            Self::InvalidOrder { node, consumer } => (*node, Some(*consumer)),
        };
        match consumer {
            Some(consumer) => write!(f, "input {node} of node {consumer}: ")?,
            None => write!(f, "final output {node}: ")?,
        }
        match self {
            Self::ForeignValue { .. } => f.write_str("value belongs to another plan"),
            Self::UnknownValue { .. } => f.write_str("producer does not exist"),
            Self::InvalidOrder { .. } => f.write_str("producer must precede consumer"),
            Self::TypeMismatch {
                expected, actual, ..
            } => write!(f, "expected {expected}, producer returns {actual}"),
        }
    }
}
impl Error for PlanBuildError {}

/// A checked failure to resolve a value from a run's store.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum ValueError {
    /// The handle belongs to a different plan.
    ForeignPlan { node: NodeId },
    /// The producer has no available output.
    Missing { node: NodeId },
    /// A stored output did not have the expected type.
    TypeMismatch { node: NodeId, expected: String },
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignPlan { node } => {
                write!(f, "value from node {node} belongs to another plan")
            }
            Self::Missing { node } => write!(f, "required value from node {node} is unavailable"),
            Self::TypeMismatch { node, expected } => write!(
                f,
                "stored value for node {node} could not be downcast to {expected}"
            ),
        }
    }
}
impl Error for ValueError {}

/// An invariant failure in a validated plan or its per-run value store.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum InvariantError {
    /// Execute encountered inputs that should have been produced already.
    UnavailableDependencies {
        node: NodeId,
        missing_dependencies: Vec<NodeId>,
    },
    /// An input could not be resolved for this node.
    Input { node: NodeId, source: ValueError },
    /// The selected final output could not be resolved.
    FinalOutput { source: ValueError },
}

impl fmt::Display for InvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnavailableDependencies {
                node,
                missing_dependencies,
            } => {
                write!(f, "node {node} has unavailable dependencies: ")?;
                for (index, dependency) in missing_dependencies.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{dependency}")?;
                }
                Ok(())
            }
            Self::Input { node, source } => {
                write!(f, "node {node} input could not be resolved: {source}")
            }
            Self::FinalOutput { source } => {
                write!(f, "final output could not be resolved: {source}")
            }
        }
    }
}

impl Error for InvariantError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input { source, .. } | Self::FinalOutput { source } => Some(source),
            Self::UnavailableDependencies { .. } => None,
        }
    }
}

/// Error returned by execute mode.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ExecuteError<E> {
    /// An operation body returned an error.
    Operation {
        /// Node id of the failed operation.
        node: NodeId,
        /// Operation name copied from metadata.
        name: String,
        /// Original operation error.
        source: E,
    },
    /// An internal plan or store invariant failed.
    Internal(InvariantError),
}

impl<E: fmt::Display> fmt::Display for ExecuteError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation { node, name, source } => {
                write!(
                    f,
                    "operation '{name}' at node {} failed: {source}",
                    node.index()
                )
            }
            Self::Internal(message) => write!(f, "internal execution error: {message}"),
        }
    }
}

impl<E> Error for ExecuteError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Operation { source, .. } => Some(source),
            Self::Internal(source) => Some(source),
        }
    }
}

/// Error returned by [`DryRunReport::require_no_failures`](crate::DryRunReport::require_no_failures).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DryRunFailure {
    failure_count: usize,
}

impl DryRunFailure {
    pub(crate) fn new(failure_count: usize) -> Self {
        Self { failure_count }
    }

    /// Number of failed dry-run nodes.
    pub fn failure_count(&self) -> usize {
        self.failure_count
    }
}

impl fmt::Display for DryRunFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dry-run completed with {} failed operation{}",
            self.failure_count,
            if self.failure_count == 1 { "" } else { "s" }
        )
    }
}

impl Error for DryRunFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_errors_keep_their_source_chain() {
        let error: ExecuteError<std::io::Error> =
            ExecuteError::Internal(InvariantError::FinalOutput {
                source: ValueError::Missing {
                    node: NodeId::new(0),
                },
            });
        assert!(error.source().unwrap().is::<InvariantError>());
        assert!(error.source().unwrap().source().unwrap().is::<ValueError>());
        let operation = ExecuteError::Operation {
            node: NodeId::new(0),
            name: "read".into(),
            source: std::io::Error::other("original"),
        };
        assert!(operation.source().unwrap().is::<std::io::Error>());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn structured_errors_and_all_outcomes_round_trip() {
        fn round_trip<
            T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + fmt::Debug,
        >(
            value: T,
        ) {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(value, serde_json::from_str::<T>(&json).unwrap());
        }
        let node = NodeId::new(0);
        let consumer = Some(NodeId::new(1));
        for error in [
            PlanBuildError::ForeignValue { node, consumer },
            PlanBuildError::UnknownValue {
                node,
                consumer: None,
            },
            PlanBuildError::InvalidOrder {
                node,
                consumer: NodeId::new(1),
            },
            PlanBuildError::TypeMismatch {
                node,
                consumer,
                expected: "u32".into(),
                actual: "String".into(),
            },
        ] {
            round_trip(error);
        }
        let mut internal = vec![InvariantError::UnavailableDependencies {
            node,
            missing_dependencies: vec![node],
        }];
        for source in [
            ValueError::ForeignPlan { node },
            ValueError::Missing { node },
            ValueError::TypeMismatch {
                node,
                expected: "u32".into(),
            },
        ] {
            round_trip(source.clone());
            internal.push(InvariantError::Input {
                node,
                source: source.clone(),
            });
            internal.push(InvariantError::FinalOutput { source });
        }
        for error in internal {
            round_trip(error.clone());
            round_trip(ExecuteError::<String>::Internal(error.clone()));
            round_trip(crate::NodeOutcome::<String>::Internal { error });
        }
        round_trip(ExecuteError::Operation {
            node,
            name: "read".into(),
            source: "failed".to_owned(),
        });
        for outcome in [
            crate::NodeOutcome::Executed,
            crate::NodeOutcome::Skipped {
                reason: "policy".into(),
            },
            crate::NodeOutcome::Denied {
                reason: "policy".into(),
            },
            crate::NodeOutcome::Blocked {
                missing_dependencies: vec![node],
            },
            crate::NodeOutcome::Failed {
                error: "operation failed".to_owned(),
            },
        ] {
            round_trip(outcome);
        }
    }
}

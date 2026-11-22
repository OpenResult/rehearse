use crate::{ExecuteError, NodeId};
use std::error::Error;
use std::fmt;

/// Outcomes retained after concurrent execution stops scheduling and drains
/// operations already in flight. No completed write is rolled back.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConcurrentExecuteError<E> {
    #[cfg_attr(
        feature = "serde",
        serde(
            deserialize_with = "deserialize_nonempty_errors",
            bound(deserialize = "E: serde::Deserialize<'de>")
        )
    )]
    errors: Vec<ExecuteError<E>>,
    completed: Vec<NodeId>,
    not_started: Vec<NodeId>,
}

impl<E> ConcurrentExecuteError<E> {
    pub(crate) fn new(
        errors: Vec<ExecuteError<E>>,
        mut completed: Vec<NodeId>,
        mut not_started: Vec<NodeId>,
    ) -> Self {
        debug_assert!(!errors.is_empty());
        completed.sort();
        not_started.sort();
        Self {
            errors,
            completed,
            not_started,
        }
    }
    /// Failures in observation order; the first failure stopped new launches.
    pub fn errors(&self) -> &[ExecuteError<E>] {
        &self.errors
    }
    /// The first observed operation or invariant error.
    pub fn primary_error(&self) -> &ExecuteError<E> {
        self.errors
            .first()
            .expect("concurrent failures are nonempty")
    }
    /// Consumes this report and returns the original structured errors.
    pub fn into_errors(self) -> Vec<ExecuteError<E>> {
        self.errors
    }
    /// Nodes whose bodies completed successfully, sorted in declaration order.
    pub fn completed_nodes(&self) -> &[NodeId] {
        &self.completed
    }
    /// Nodes never launched after scheduling stopped, in declaration order.
    pub fn not_started_nodes(&self) -> &[NodeId] {
        &self.not_started
    }
}
impl<E: fmt::Display> fmt::Display for ConcurrentExecuteError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; {} failure(s), {} completed, {} not started",
            self.primary_error(),
            self.errors.len(),
            self.completed.len(),
            self.not_started.len()
        )
    }
}
impl<E: Error + 'static> Error for ConcurrentExecuteError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.primary_error())
    }
}

#[cfg(feature = "serde")]
fn deserialize_nonempty_errors<'de, D, E>(deserializer: D) -> Result<Vec<ExecuteError<E>>, D::Error>
where
    D: serde::Deserializer<'de>,
    E: serde::Deserialize<'de>,
{
    let errors = <Vec<ExecuteError<E>> as serde::Deserialize>::deserialize(deserializer)?;
    if errors.is_empty() {
        return Err(serde::de::Error::custom(
            "concurrent errors must be nonempty",
        ));
    }
    Ok(errors)
}

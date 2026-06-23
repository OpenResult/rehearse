use crate::NodeId;
use std::error::Error;
use std::fmt;

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
    Internal(String),
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
            Self::Internal(_) => None,
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

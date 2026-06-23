use crate::{Impact, OperationMetadata};
use std::fmt;

/// The action a dry-run policy assigns to an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DryRunAction {
    /// Invoke the operation body during dry-run.
    Run,
    /// Do not invoke the operation body, but report it as skipped by policy.
    Skip,
    /// Do not invoke the operation body, and report it as denied by policy.
    Deny,
}

impl fmt::Display for DryRunAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Run => "run",
            Self::Skip => "skip",
            Self::Deny => "deny",
        };
        f.write_str(value)
    }
}

/// Decides how each operation behaves during dry-run.
pub trait DryRunPolicy {
    /// Returns the dry-run action for an operation's static metadata.
    fn action(&self, metadata: &OperationMetadata) -> DryRunAction;
}

/// Default conservative dry-run policy.
///
/// Pure, session, and read operations run. Writes and deletes are skipped.
/// Opaque operations are denied.
#[derive(Debug, Default, Clone, Copy)]
pub struct SafeDryRun;

impl DryRunPolicy for SafeDryRun {
    fn action(&self, metadata: &OperationMetadata) -> DryRunAction {
        match metadata.impact() {
            Impact::Pure | Impact::Session | Impact::Read => DryRunAction::Run,
            Impact::Write | Impact::Delete => DryRunAction::Skip,
            Impact::Opaque => DryRunAction::Deny,
        }
    }
}

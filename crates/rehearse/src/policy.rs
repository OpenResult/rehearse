use crate::{Impact, OperationMetadata};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DryRunAction {
    Run,
    Skip,
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

pub trait DryRunPolicy {
    fn action(&self, metadata: &OperationMetadata) -> DryRunAction;
}

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

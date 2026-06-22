use crate::{DryRunFailure, Impact, NodeId, OperationMetadata};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOutcome<E> {
    Executed,
    Skipped {
        reason: String,
    },
    Denied {
        reason: String,
    },
    Blocked {
        missing_dependencies: Vec<NodeId>,
    },
    Failed {
        error: E,
    },
    #[doc(hidden)]
    Internal {
        error: String,
    },
}

impl<E> NodeOutcome<E> {
    pub fn is_executed(&self) -> bool {
        matches!(self, Self::Executed)
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Internal { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeReport<E> {
    node: NodeId,
    name: String,
    impact: Impact,
    outcome: NodeOutcome<E>,
}

impl<E> NodeReport<E> {
    pub(crate) fn new(node: NodeId, metadata: &OperationMetadata, outcome: NodeOutcome<E>) -> Self {
        Self {
            node,
            name: metadata.name().to_owned(),
            impact: metadata.impact(),
            outcome,
        }
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn impact(&self) -> Impact {
        self.impact
    }

    pub fn outcome(&self) -> &NodeOutcome<E> {
        &self.outcome
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunStatus {
    Complete,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunReport<E> {
    plan_name: String,
    nodes: Vec<NodeReport<E>>,
}

impl<E> DryRunReport<E> {
    pub(crate) fn new(plan_name: impl Into<String>) -> Self {
        Self {
            plan_name: plan_name.into(),
            nodes: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, node: NodeReport<E>) {
        self.nodes.push(node);
    }

    pub fn plan_name(&self) -> &str {
        &self.plan_name
    }

    pub fn iter(&self) -> impl Iterator<Item = &NodeReport<E>> {
        self.nodes.iter()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn executed_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_executed())
            .count()
    }

    pub fn skipped_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_skipped())
            .count()
    }

    pub fn denied_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_denied())
            .count()
    }

    pub fn blocked_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_blocked())
            .count()
    }

    pub fn failure_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_failed())
            .count()
    }

    pub fn has_failures(&self) -> bool {
        self.failure_count() > 0
    }

    pub fn has_blocked(&self) -> bool {
        self.blocked_count() > 0
    }

    pub fn has_denied(&self) -> bool {
        self.denied_count() > 0
    }

    pub fn status(&self) -> DryRunStatus {
        if self.has_failures() {
            DryRunStatus::Failed
        } else if self.skipped_count() > 0 || self.has_denied() || self.has_blocked() {
            DryRunStatus::Incomplete
        } else {
            DryRunStatus::Complete
        }
    }

    pub fn require_no_failures(&self) -> Result<(), DryRunFailure> {
        let failures = self.failure_count();
        if failures == 0 {
            Ok(())
        } else {
            Err(DryRunFailure::new(failures))
        }
    }
}

impl<E: fmt::Display> fmt::Display for DryRunReport<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for node in &self.nodes {
            match node.outcome() {
                NodeOutcome::Executed => {
                    writeln!(f, "[ok] {} executed", node.name())?;
                }
                NodeOutcome::Skipped { reason } => {
                    writeln!(f, "[skip] {} skipped: {reason}", node.name())?;
                }
                NodeOutcome::Denied { reason } => {
                    writeln!(f, "[deny] {} denied: {reason}", node.name())?;
                }
                NodeOutcome::Blocked {
                    missing_dependencies,
                } => {
                    let missing = format_node_list(missing_dependencies);
                    writeln!(f, "[block] {} blocked: missing {missing}", node.name())?;
                }
                NodeOutcome::Failed { error } => {
                    writeln!(f, "[fail] {} failed: {error}", node.name())?;
                }
                NodeOutcome::Internal { error } => {
                    writeln!(f, "[fail] {} internal error: {error}", node.name())?;
                }
            }
        }

        if !self.nodes.is_empty() {
            writeln!(f)?;
        }

        write!(
            f,
            "Dry-run {}: {} executed, {} skipped, {} denied, {} blocked, {} failed.",
            match self.status() {
                DryRunStatus::Complete => "complete",
                DryRunStatus::Incomplete => "incomplete",
                DryRunStatus::Failed => "failed",
            },
            self.executed_count(),
            self.skipped_count(),
            self.denied_count(),
            self.blocked_count(),
            self.failure_count()
        )
    }
}

fn format_node_list(nodes: &[NodeId]) -> String {
    nodes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

use crate::{DryRunFailure, Impact, NodeId, OperationMetadata};
use std::fmt;

/// Outcome recorded for one dry-run node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOutcome<E> {
    /// The node body ran successfully and produced a real value.
    Executed,
    /// Policy skipped the node body.
    Skipped {
        /// Human-readable skip reason.
        reason: String,
    },
    /// Policy denied the node body.
    Denied {
        /// Human-readable denial reason.
        reason: String,
    },
    /// The node could not run because one or more value inputs were unavailable.
    Blocked {
        /// Producer nodes whose outputs were unavailable.
        missing_dependencies: Vec<NodeId>,
    },
    /// The node body ran and returned an operation error.
    Failed {
        /// Original operation error.
        error: E,
    },
    #[doc(hidden)]
    Internal { error: String },
}

impl<E> NodeOutcome<E> {
    /// Returns true when the outcome is [`Executed`](Self::Executed).
    pub fn is_executed(&self) -> bool {
        matches!(self, Self::Executed)
    }

    /// Returns true when the outcome is [`Skipped`](Self::Skipped).
    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    /// Returns true when the outcome is [`Denied`](Self::Denied).
    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }

    /// Returns true when the outcome is [`Blocked`](Self::Blocked).
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked { .. })
    }

    /// Returns true for operation failures and internal invariant errors.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Internal { .. })
    }
}

/// Report row for one dry-run node.
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

    /// Returns this node's id.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// Returns this node's operation name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns this node's declared impact.
    pub fn impact(&self) -> Impact {
        self.impact
    }

    /// Returns this node's dry-run outcome.
    pub fn outcome(&self) -> &NodeOutcome<E> {
        &self.outcome
    }
}

/// Aggregate dry-run status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunStatus {
    /// Every node executed successfully.
    Complete,
    /// At least one node was skipped, denied, or blocked, and no node failed.
    Incomplete,
    /// At least one executed node failed or an internal invariant error occurred.
    Failed,
}

/// Structured report returned by dry-run mode.
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

    /// Returns the plan name copied into the report.
    pub fn plan_name(&self) -> &str {
        &self.plan_name
    }

    /// Iterates over node reports in plan order.
    pub fn iter(&self) -> impl Iterator<Item = &NodeReport<E>> {
        self.nodes.iter()
    }

    /// Returns the number of node outcomes in the report.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true when the report contains no node outcomes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Counts successfully executed nodes.
    pub fn executed_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_executed())
            .count()
    }

    /// Counts policy-skipped nodes.
    pub fn skipped_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_skipped())
            .count()
    }

    /// Counts policy-denied nodes.
    pub fn denied_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_denied())
            .count()
    }

    /// Counts dependency-blocked nodes.
    pub fn blocked_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_blocked())
            .count()
    }

    /// Counts failed operation nodes and internal invariant errors.
    pub fn failure_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.outcome().is_failed())
            .count()
    }

    /// Returns true if one or more nodes failed.
    pub fn has_failures(&self) -> bool {
        self.failure_count() > 0
    }

    /// Returns true if one or more nodes were blocked.
    pub fn has_blocked(&self) -> bool {
        self.blocked_count() > 0
    }

    /// Returns true if one or more nodes were denied.
    pub fn has_denied(&self) -> bool {
        self.denied_count() > 0
    }

    /// Returns aggregate dry-run status derived from all node outcomes.
    pub fn status(&self) -> DryRunStatus {
        if self.has_failures() {
            DryRunStatus::Failed
        } else if self.skipped_count() > 0 || self.has_denied() || self.has_blocked() {
            DryRunStatus::Incomplete
        } else {
            DryRunStatus::Complete
        }
    }

    /// Returns an error only when one or more nodes failed.
    ///
    /// Ordinary skipped writes and deletes do not make this method fail.
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

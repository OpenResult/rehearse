use crate::{DryRunFailure, Impact, NodeId, OperationMetadata};
use std::fmt;

/// Outcome recorded for one dry-run node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    /// Returns an error unless every node executed successfully.
    ///
    /// This is stricter than [`require_no_failures`](Self::require_no_failures):
    /// skipped, denied, and blocked nodes are rejected too.
    pub fn require_complete(&self) -> Result<(), DryRunIncomplete> {
        match self.status() {
            DryRunStatus::Complete => Ok(()),
            status => Err(DryRunIncomplete::new(
                status,
                self.executed_count(),
                self.skipped_count(),
                self.denied_count(),
                self.blocked_count(),
                self.failure_count(),
            )),
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
                    let missing = format_node_list(missing_dependencies, &self.nodes);
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

/// Error returned by [`DryRunReport::require_complete`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DryRunIncomplete {
    status: DryRunStatus,
    executed_count: usize,
    skipped_count: usize,
    denied_count: usize,
    blocked_count: usize,
    failure_count: usize,
}

impl DryRunIncomplete {
    pub(crate) fn new(
        status: DryRunStatus,
        executed_count: usize,
        skipped_count: usize,
        denied_count: usize,
        blocked_count: usize,
        failure_count: usize,
    ) -> Self {
        Self {
            status,
            executed_count,
            skipped_count,
            denied_count,
            blocked_count,
            failure_count,
        }
    }

    /// Aggregate status that caused the report to be rejected.
    pub fn status(&self) -> DryRunStatus {
        self.status
    }

    /// Counts successfully executed nodes.
    pub fn executed_count(&self) -> usize {
        self.executed_count
    }

    /// Counts policy-skipped nodes.
    pub fn skipped_count(&self) -> usize {
        self.skipped_count
    }

    /// Counts policy-denied nodes.
    pub fn denied_count(&self) -> usize {
        self.denied_count
    }

    /// Counts dependency-blocked nodes.
    pub fn blocked_count(&self) -> usize {
        self.blocked_count
    }

    /// Counts failed operation nodes and internal invariant errors.
    pub fn failure_count(&self) -> usize {
        self.failure_count
    }
}

impl fmt::Display for DryRunIncomplete {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dry-run was not complete: {} executed, {} skipped, {} denied, {} blocked, {} failed",
            self.executed_count,
            self.skipped_count,
            self.denied_count,
            self.blocked_count,
            self.failure_count
        )
    }
}

impl std::error::Error for DryRunIncomplete {}

fn format_node_list<E>(nodes: &[NodeId], reports: &[NodeReport<E>]) -> String {
    nodes
        .iter()
        .map(|node| format_node_reference(*node, reports))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_node_reference<E>(node: NodeId, reports: &[NodeReport<E>]) -> String {
    match reports.iter().find(|report| report.node() == node) {
        Some(report) => format!("{node} ({})", report.name()),
        None => node.to_string(),
    }
}

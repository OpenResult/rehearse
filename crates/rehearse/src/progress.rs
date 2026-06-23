use crate::{DryRunAction, DryRunStatus, Impact, NodeId};

/// Runtime mode that emitted a progress event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    /// Static plan description.
    Describe,
    /// Safe dry-run traversal.
    DryRun,
    /// Execute traversal.
    Execute,
}

/// Aggregate plan outcome observed by a progress listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPlanOutcome {
    /// The mode completed without failures or incomplete dry-run nodes.
    Complete,
    /// Dry-run completed with skipped, denied, or blocked nodes, and no failures.
    Incomplete,
    /// The mode observed at least one failure or internal invariant error.
    Failed,
}

impl From<DryRunStatus> for ProgressPlanOutcome {
    fn from(status: DryRunStatus) -> Self {
        match status {
            DryRunStatus::Complete => Self::Complete,
            DryRunStatus::Incomplete => Self::Incomplete,
            DryRunStatus::Failed => Self::Failed,
        }
    }
}

/// Static metadata for the node currently being reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressNode<'a> {
    node: NodeId,
    position: usize,
    total: usize,
    name: &'a str,
    impact: Impact,
}

impl<'a> ProgressNode<'a> {
    pub(crate) fn new(
        node: NodeId,
        position: usize,
        total: usize,
        name: &'a str,
        impact: Impact,
    ) -> Self {
        Self {
            node,
            position,
            total,
            name,
            impact,
        }
    }

    /// Returns the node id.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the 1-based position in the plan.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns the total number of nodes in the plan.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Returns the operation name.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// Returns the operation's declared impact.
    pub fn impact(&self) -> Impact {
        self.impact
    }
}

/// Node-level outcome observed by a progress listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressOutcome<'a, E> {
    /// Static metadata was described. Dry-run descriptions include the policy
    /// action; execute descriptions do not.
    Described {
        /// Dry-run policy action selected for the node, when applicable.
        dry_run_action: Option<DryRunAction>,
    },
    /// The node body ran successfully and produced a real value.
    Executed,
    /// Policy skipped the node body during dry-run.
    Skipped {
        /// Human-readable skip reason.
        reason: &'a str,
    },
    /// Policy denied the node body during dry-run.
    Denied {
        /// Human-readable denial reason.
        reason: &'a str,
    },
    /// Dry-run could not run the node because inputs were unavailable.
    Blocked {
        /// Producer nodes whose outputs were unavailable.
        missing_dependencies: &'a [NodeId],
    },
    /// Execute mode found unavailable inputs before running the node.
    UnavailableDependencies {
        /// Producer nodes whose outputs were unavailable.
        missing_dependencies: &'a [NodeId],
    },
    /// The node body ran and returned an operation error.
    Failed {
        /// Original operation error.
        error: &'a E,
    },
    /// The runtime reported an internal invariant error.
    Internal {
        /// Human-readable internal error.
        error: &'a str,
    },
}

/// Progress event emitted while describing or running a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEvent<'a, E> {
    /// A describe, dry-run, or execute traversal started.
    PlanStarted {
        /// Runtime mode being observed.
        mode: ProgressMode,
        /// Plan name.
        plan_name: &'a str,
        /// Total number of nodes in the plan.
        total_nodes: usize,
    },
    /// A static description row was produced.
    NodeDescribed {
        /// Runtime mode being observed.
        mode: ProgressMode,
        /// Static node metadata.
        node: ProgressNode<'a>,
        /// Description outcome.
        outcome: ProgressOutcome<'a, E>,
    },
    /// A dry-run or execute node decision started.
    NodeStarted {
        /// Runtime mode being observed.
        mode: ProgressMode,
        /// Static node metadata.
        node: ProgressNode<'a>,
    },
    /// A dry-run or execute node decision finished.
    NodeFinished {
        /// Runtime mode being observed.
        mode: ProgressMode,
        /// Static node metadata.
        node: ProgressNode<'a>,
        /// Node outcome.
        outcome: ProgressOutcome<'a, E>,
    },
    /// A describe, dry-run, or execute traversal finished.
    PlanFinished {
        /// Runtime mode being observed.
        mode: ProgressMode,
        /// Plan name.
        plan_name: &'a str,
        /// Total number of nodes in the plan.
        total_nodes: usize,
        /// Aggregate outcome.
        outcome: ProgressPlanOutcome,
    },
}

/// Observes progress while a plan is described, dry-run, or executed.
///
/// Progress listeners are observation hooks only. Returning from `on_event`
/// cannot change plan traversal, policy decisions, value storage, or operation
/// execution.
pub trait ProgressListener<E> {
    /// Receives one progress event.
    fn on_event(&mut self, event: ProgressEvent<'_, E>);
}

impl<E, F> ProgressListener<E> for F
where
    F: for<'a> FnMut(ProgressEvent<'a, E>),
{
    fn on_event(&mut self, event: ProgressEvent<'_, E>) {
        self(event);
    }
}

/// Progress listener that ignores every event.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopProgress;

impl<E> ProgressListener<E> for NoopProgress {
    fn on_event(&mut self, _event: ProgressEvent<'_, E>) {}
}

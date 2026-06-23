use crate::{
    DryRunAction, DryRunPolicy, Impact, NodeId, NoopProgress, ProgressEvent, ProgressListener,
    ProgressMode, ProgressNode, ProgressOutcome, ProgressPlanOutcome,
};
use std::fmt;

/// Owned static description of a plan.
///
/// A description is copied from plan metadata and policy decisions. It never
/// touches context, value stores, or operation bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDescription {
    plan_name: String,
    rows: Vec<PlanDescriptionRow>,
}

impl PlanDescription {
    pub(crate) fn new(plan_name: impl Into<String>, rows: Vec<PlanDescriptionRow>) -> Self {
        Self {
            plan_name: plan_name.into(),
            rows,
        }
    }

    /// Returns the described plan name.
    pub fn plan_name(&self) -> &str {
        &self.plan_name
    }

    /// Iterates description rows in plan order.
    pub fn iter(&self) -> impl Iterator<Item = &PlanDescriptionRow> {
        self.rows.iter()
    }

    /// Returns the number of described nodes.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns true when the description contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl fmt::Display for PlanDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.plan_name)?;

        if !self.rows.is_empty() {
            writeln!(f)?;
        }

        for row in &self.rows {
            let impact = row.impact.to_string();
            let dry_run_action = row.dry_run_action.to_string();
            writeln!(
                f,
                "{:>3}  {:<20} {:<8} {}",
                row.position, row.name, impact, dry_run_action
            )?;
        }

        Ok(())
    }
}

/// Static description row for one plan node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDescriptionRow {
    node: NodeId,
    position: usize,
    name: String,
    impact: Impact,
    dry_run_action: DryRunAction,
}

impl PlanDescriptionRow {
    pub(crate) fn new(
        node: NodeId,
        position: usize,
        name: impl Into<String>,
        impact: Impact,
        dry_run_action: DryRunAction,
    ) -> Self {
        Self {
            node,
            position,
            name: name.into(),
            impact,
            dry_run_action,
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

    /// Returns the operation name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the operation's declared impact.
    pub fn impact(&self) -> Impact {
        self.impact
    }

    /// Returns the dry-run action selected by the policy used for description.
    pub fn dry_run_action(&self) -> DryRunAction {
        self.dry_run_action
    }
}

pub(crate) fn describe_plan<C, E, P>(
    name: &str,
    nodes: &[Box<dyn crate::plan::node::ErasedNode<C, E>>],
    policy: &P,
) -> PlanDescription
where
    P: DryRunPolicy,
{
    let mut progress = NoopProgress;
    describe_plan_with_listener(name, nodes, policy, &mut progress)
}

pub(crate) fn describe_plan_with_listener<C, E, P, L>(
    name: &str,
    nodes: &[Box<dyn crate::plan::node::ErasedNode<C, E>>],
    policy: &P,
    listener: &mut L,
) -> PlanDescription
where
    P: DryRunPolicy,
    L: ProgressListener<E> + ?Sized,
{
    listener.on_event(ProgressEvent::PlanStarted {
        mode: ProgressMode::Describe,
        plan_name: name,
        total_nodes: nodes.len(),
    });

    let rows = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let metadata = node.metadata();
            let action = policy.action(metadata);
            let progress_node = ProgressNode::new(
                node.id(),
                index + 1,
                nodes.len(),
                metadata.name(),
                metadata.impact(),
            );
            listener.on_event(ProgressEvent::NodeDescribed {
                mode: ProgressMode::Describe,
                node: progress_node,
                outcome: ProgressOutcome::Described {
                    dry_run_action: Some(action),
                },
            });
            PlanDescriptionRow::new(
                node.id(),
                index + 1,
                metadata.name(),
                metadata.impact(),
                action,
            )
        })
        .collect();

    listener.on_event(ProgressEvent::PlanFinished {
        mode: ProgressMode::Describe,
        plan_name: name,
        total_nodes: nodes.len(),
        outcome: ProgressPlanOutcome::Complete,
    });

    PlanDescription::new(name, rows)
}

/// Owned static description of execute-mode plan order.
///
/// Execution descriptions contain only static metadata. They never touch
/// context, value stores, or operation bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanExecutionDescription {
    plan_name: String,
    rows: Vec<PlanExecutionDescriptionRow>,
}

impl PlanExecutionDescription {
    pub(crate) fn new(
        plan_name: impl Into<String>,
        rows: Vec<PlanExecutionDescriptionRow>,
    ) -> Self {
        Self {
            plan_name: plan_name.into(),
            rows,
        }
    }

    /// Returns the described plan name.
    pub fn plan_name(&self) -> &str {
        &self.plan_name
    }

    /// Iterates description rows in plan order.
    pub fn iter(&self) -> impl Iterator<Item = &PlanExecutionDescriptionRow> {
        self.rows.iter()
    }

    /// Returns the number of described nodes.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns true when the description contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl fmt::Display for PlanExecutionDescription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.plan_name)?;

        if !self.rows.is_empty() {
            writeln!(f)?;
        }

        for row in &self.rows {
            let impact = row.impact.to_string();
            writeln!(f, "{:>3}  {:<20} {}", row.position, row.name, impact)?;
        }

        Ok(())
    }
}

/// Static execute-mode description row for one plan node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanExecutionDescriptionRow {
    node: NodeId,
    position: usize,
    name: String,
    impact: Impact,
}

impl PlanExecutionDescriptionRow {
    pub(crate) fn new(
        node: NodeId,
        position: usize,
        name: impl Into<String>,
        impact: Impact,
    ) -> Self {
        Self {
            node,
            position,
            name: name.into(),
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

    /// Returns the operation name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the operation's declared impact.
    pub fn impact(&self) -> Impact {
        self.impact
    }
}

pub(crate) fn describe_execution_plan<C, E>(
    name: &str,
    nodes: &[Box<dyn crate::plan::node::ErasedNode<C, E>>],
) -> PlanExecutionDescription {
    let mut progress = NoopProgress;
    describe_execution_plan_with_listener(name, nodes, &mut progress)
}

pub(crate) fn describe_execution_plan_with_listener<C, E, L>(
    name: &str,
    nodes: &[Box<dyn crate::plan::node::ErasedNode<C, E>>],
    listener: &mut L,
) -> PlanExecutionDescription
where
    L: ProgressListener<E> + ?Sized,
{
    listener.on_event(ProgressEvent::PlanStarted {
        mode: ProgressMode::Describe,
        plan_name: name,
        total_nodes: nodes.len(),
    });

    let rows = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let metadata = node.metadata();
            let progress_node = ProgressNode::new(
                node.id(),
                index + 1,
                nodes.len(),
                metadata.name(),
                metadata.impact(),
            );
            listener.on_event(ProgressEvent::NodeDescribed {
                mode: ProgressMode::Describe,
                node: progress_node,
                outcome: ProgressOutcome::Described {
                    dry_run_action: None,
                },
            });
            PlanExecutionDescriptionRow::new(
                node.id(),
                index + 1,
                metadata.name(),
                metadata.impact(),
            )
        })
        .collect();

    listener.on_event(ProgressEvent::PlanFinished {
        mode: ProgressMode::Describe,
        plan_name: name,
        total_nodes: nodes.len(),
        outcome: ProgressPlanOutcome::Complete,
    });

    PlanExecutionDescription::new(name, rows)
}

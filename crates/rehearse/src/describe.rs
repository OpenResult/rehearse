use crate::{DryRunAction, DryRunPolicy, Impact, NodeId};
use std::fmt;

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

    pub fn plan_name(&self) -> &str {
        &self.plan_name
    }

    pub fn iter(&self) -> impl Iterator<Item = &PlanDescriptionRow> {
        self.rows.iter()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

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

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn impact(&self) -> Impact {
        self.impact
    }

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
    let rows = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let metadata = node.metadata();
            PlanDescriptionRow::new(
                node.id(),
                index + 1,
                metadata.name(),
                metadata.impact(),
                policy.action(metadata),
            )
        })
        .collect();

    PlanDescription::new(name, rows)
}

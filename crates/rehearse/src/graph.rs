use crate::plan::ExecutionStep;
use crate::{Impact, NodeId, Plan};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// One operation's static, value-free graph metadata.
///
/// Annotations are application-declared display metadata. They are not inferred
/// from operation names, inputs, bodies, or external state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GraphNode {
    id: NodeId,
    position: usize,
    name: String,
    impact: Impact,
    dependencies: Vec<NodeId>,
    annotations: BTreeMap<String, String>,
}

impl GraphNode {
    /// Returns this plan-local node identifier.
    pub fn id(&self) -> NodeId {
        self.id
    }
    /// Returns the one-based declaration position.
    pub fn position(&self) -> usize {
        self.position
    }
    /// Returns the declared operation name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the operation's declared impact.
    pub fn impact(&self) -> Impact {
        self.impact
    }
    /// Returns producers of explicit value inputs, independently of control order.
    pub fn dependencies(&self) -> &[NodeId] {
        &self.dependencies
    }
    /// Returns application-declared, static display metadata.
    pub fn annotations(&self) -> &BTreeMap<String, String> {
        &self.annotations
    }
}

/// Canonical static graph description for CLI and browser renderers.
///
/// The graph records declaration order, explicit parallel regions, and value
/// dependencies separately. The default interpreter remains sequential, even
/// when the graph declares regions that a concurrent interpreter may honor.
///
/// This is description serialization, not executable-plan serialization. It
/// contains no operation bodies, context, literal inputs, or runtime outputs.
/// Runtime progress belongs in a separate overlay keyed by run, application
/// plan revision, and plan-local node id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlanGraphDescription {
    schema_version: u32,
    plan_name: String,
    plan_revision: Option<String>,
    default_interpretation: String,
    nodes: Vec<GraphNode>,
    execution: Vec<ExecutionStep>,
}

impl PlanGraphDescription {
    /// Current metadata schema version.
    pub const SCHEMA_VERSION: u32 = 1;
    /// Returns the serialized description schema version.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    /// Returns the plan's declared name.
    pub fn plan_name(&self) -> &str {
        &self.plan_name
    }
    /// Returns the application's declared code/configuration revision, if any.
    ///
    /// Rehearse cannot infer a revision from opaque operation bodies. An
    /// application must update this identity when its implementation or relevant
    /// configuration changes; a graph-metadata hash alone is insufficient.
    pub fn plan_revision(&self) -> Option<&str> {
        self.plan_revision.as_deref()
    }
    /// Returns the default execution interpretation: `sequential`.
    pub fn default_interpretation(&self) -> &str {
        &self.default_interpretation
    }
    /// Returns nodes in declaration order.
    pub fn nodes(&self) -> &[GraphNode] {
        &self.nodes
    }
    /// Returns declared sequence and parallel-region structure.
    pub fn execution(&self) -> &[ExecutionStep] {
        &self.execution
    }

    /// Renders the declared execution structure as Mermaid.
    ///
    /// Solid arrows express control structure; dotted arrows add value-input
    /// relationships not already represented by a direct control arrow. A
    /// dataflow fork alone never becomes a parallel region. Explicit region
    /// labels state their concurrency limit, and the legend identifies ordinary
    /// `execute()` as sequential.
    pub fn to_mermaid(&self) -> String {
        let mut output = String::from("flowchart TD\n");
        writeln!(output, "  %% plan: {}", comment(&self.plan_name)).unwrap();
        output.push_str("  %% Default execute(): sequential. Explicit parallel regions require a concurrent interpreter.\n");
        output.push_str(
            "  %% Solid arrows: control structure. Dotted arrows: additional Value inputs.\n",
        );
        for node in &self.nodes {
            let display = node.annotations.get("label").unwrap_or(&node.name);
            writeln!(
                output,
                "  n{}[\"{}. {}\\n{}\"]",
                node.id.index(),
                node.position,
                label(display),
                node.impact
            )
            .unwrap();
        }

        let mut direct_control = BTreeSet::new();
        let mut previous: Option<String> = None;
        for step in &self.execution {
            match step {
                ExecutionStep::Node { node } => {
                    let current = format!("n{}", node.index());
                    if let Some(previous) = previous {
                        edge(&mut output, &previous, &current);
                        direct_control.insert((previous, current.clone()));
                    }
                    previous = Some(current);
                }
                ExecutionStep::Parallel { group } => {
                    let fork = format!("g{}_fork", group.id);
                    let join = format!("g{}_join", group.id);
                    writeln!(
                        output,
                        "  {fork}{{\"{}\\nmax {} concurrent\"}}",
                        label(&group.name),
                        group.max_concurrency
                    )
                    .unwrap();
                    writeln!(output, "  {join}{{\"Join {}\"}}", label(&group.name)).unwrap();
                    if let Some(previous) = previous {
                        edge(&mut output, &previous, &fork);
                    }
                    for (index, branch) in group.branches.iter().enumerate() {
                        writeln!(
                            output,
                            "  subgraph g{}_b{}[\"{}\"]",
                            group.id,
                            index,
                            label(&branch.name)
                        )
                        .unwrap();
                        for node in &branch.nodes {
                            writeln!(output, "    n{}", node.index()).unwrap();
                        }
                        output.push_str("  end\n");
                        let mut previous = fork.clone();
                        for node in &branch.nodes {
                            let current = format!("n{}", node.index());
                            edge(&mut output, &previous, &current);
                            direct_control.insert((previous, current.clone()));
                            previous = current;
                        }
                        edge(&mut output, &previous, &join);
                    }
                    if group.branches.is_empty() {
                        edge(&mut output, &fork, &join);
                    }
                    previous = Some(join);
                }
            }
        }
        for node in &self.nodes {
            for dependency in &node.dependencies {
                let from = format!("n{}", dependency.index());
                let to = format!("n{}", node.id.index());
                if !direct_control.contains(&(from.clone(), to.clone())) {
                    writeln!(output, "  {from} -.-> {to}").unwrap();
                }
            }
        }
        output
    }
}

fn edge(output: &mut String, from: &str, to: &str) {
    writeln!(output, "  {from} --> {to}").unwrap();
}

fn comment(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

fn label(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "&#92;")
        .replace(['\n', '\r'], " ")
}

pub(crate) fn describe_graph<C, T, E>(plan: &Plan<C, T, E>) -> PlanGraphDescription
where
    C: Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + 'static,
{
    PlanGraphDescription {
        schema_version: PlanGraphDescription::SCHEMA_VERSION,
        plan_name: plan.name().to_owned(),
        plan_revision: plan.revision().map(str::to_owned),
        default_interpretation: "sequential".to_owned(),
        nodes: plan
            .nodes()
            .enumerate()
            .map(|(index, node)| GraphNode {
                id: node.id(),
                position: index + 1,
                name: node.name().to_owned(),
                impact: node.impact(),
                dependencies: node.dependencies().to_vec(),
                annotations: node.annotations().clone(),
            })
            .collect(),
        execution: plan.execution().to_vec(),
    }
}

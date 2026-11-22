use super::NodeId;

/// A declared execution step. Ordinary execution always remains sequential.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "snake_case"))]
pub enum ExecutionStep {
    /// One operation in declaration order.
    Node { node: NodeId },
    /// Explicit independent branches, followed by a barrier.
    Parallel { group: ParallelGroup },
}

/// A bounded parallel region. Only concurrent interpretations honor it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParallelGroup {
    pub id: usize,
    pub name: String,
    pub max_concurrency: usize,
    pub branches: Vec<ParallelBranch>,
}

/// An ordered branch within a parallel region.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParallelBranch {
    pub name: String,
    pub nodes: Vec<NodeId>,
}

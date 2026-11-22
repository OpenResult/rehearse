use crate::model::{LabError, Stack, Summary};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    #[default]
    DryRun,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailurePoint {
    Build,
    Test,
    VerifyDev,
    VerifyStaging,
    VerifyProduction,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRun {
    pub stack: Stack,
    #[serde(default)]
    pub mode: RunMode,
    pub fail_at: Option<FailurePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    AwaitingApproval,
    Succeeded,
    Incomplete,
    Rejected,
    TimedOut,
    Cancelled,
    Failed,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Running | Self::AwaitingApproval)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    NotStarted,
    Running,
    AwaitingApproval,
    Executed,
    Skipped,
    Denied,
    Blocked,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeSnapshot {
    pub id: usize,
    pub status: NodeStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalSubject {
    pub artifact_digest: String,
    pub revision: String,
    pub environment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
    pub decision: Decision,
    pub artifact_digest: String,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalSnapshot {
    pub id: String,
    pub subject: ApprovalSubject,
    pub question: String,
    pub requested_at_ms: u64,
    pub expires_at_ms: u64,
    pub decision: Option<Decision>,
    pub status: ApprovalStatus,
}

#[derive(Debug, Clone)]
pub struct ProductionPermit {
    pub run_id: String,
    pub gate_id: String,
    pub subject: ApprovalSubject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateError {
    Rejected,
    TimedOut,
    Cancelled,
}

impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Rejected => "production approval rejected",
            Self::TimedOut => "production approval expired",
            Self::Cancelled => "run cancelled",
        })
    }
}
impl Error for GateError {}

#[derive(Debug, Clone, Serialize)]
pub struct PublicError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSnapshot {
    pub id: String,
    pub plan_revision: String,
    pub graph: rehearse::PlanGraphDescription,
    pub stack: Stack,
    pub mode: RunMode,
    pub revision: String,
    pub status: RunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub last_event_id: u64,
    pub nodes: Vec<NodeSnapshot>,
    pub approval: Option<ApprovalSnapshot>,
    pub error: Option<PublicError>,
    pub summary: Option<Summary>,
    pub workspace: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunCreated,
    PlanStarted,
    PlanFinished,
    NodeStarted,
    NodeFinished,
    ApprovalRequested,
    ApprovalDecided,
    ApprovalExpired,
    RunFinished,
    Snapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnedEvent {
    pub seq: u64,
    pub run_id: String,
    pub plan_revision: String,
    pub at_ms: u64,
    pub kind: EventKind,
    pub node_id: Option<usize>,
    pub message: String,
    pub snapshot: RunSnapshot,
}

pub struct Replay {
    pub events: Vec<OwnedEvent>,
    pub reset: Option<OwnedEvent>,
}

#[derive(Debug)]
pub enum ControlError {
    Invalid(String),
    NotFound(String),
    Conflict(String),
    Capacity,
    Lab(LabError),
}

impl From<LabError> for ControlError {
    fn from(value: LabError) -> Self {
        Self::Lab(value)
    }
}
impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::NotFound(message) | Self::Conflict(message) => {
                f.write_str(message)
            }
            Self::Capacity => f.write_str(
                "run limit reached; restart this local server to begin a new in-memory session",
            ),
            Self::Lab(error) => error.fmt(f),
        }
    }
}
impl Error for ControlError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Lab(error) => Some(error),
            _ => None,
        }
    }
}

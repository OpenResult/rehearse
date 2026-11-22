use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stack {
    Rust,
    SpringBoot,
}

impl Stack {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::SpringBoot => "spring-boot",
        }
    }
}

impl fmt::Display for Stack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Authenticate,
    ReadSource,
    Validate,
    Build,
    Test,
    Deploy,
    Verify,
    Gateway,
    Observe,
    Cleanup,
    Legacy,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LabError {
    Io {
        action: &'static str,
        path: PathBuf,
        #[serde(serialize_with = "serialize_display")]
        source: io::Error,
    },
    Json {
        path: PathBuf,
        #[serde(serialize_with = "serialize_display")]
        source: serde_json::Error,
    },
    Invalid {
        path: PathBuf,
        reason: String,
    },
    Conflict {
        path: PathBuf,
    },
    Injected {
        stage: Stage,
    },
}

impl LabError {
    pub fn io(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
    pub fn invalid(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::Invalid {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for LabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "{action} {}: {source}", path.display()),
            Self::Json { path, source } => {
                write!(f, "invalid JSON in {}: {source}", path.display())
            }
            Self::Invalid { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::Conflict { path } => write!(
                f,
                "conflicting contents at {}; choose a new revision",
                path.display()
            ),
            Self::Injected { stage } => write!(
                f,
                "injected failure at {}",
                stage.to_possible_value().expect("stage name").get_name()
            ),
        }
    }
}

impl Error for LabError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn serialize_display<S: serde::Serializer, E: fmt::Display>(
    value: &E,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub actor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub stack: Stack,
    pub revision: String,
    pub contents: String,
    pub build_command: String,
    pub test_command: String,
    pub expected_health: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub stack: Stack,
    pub revision: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestedArtifact(pub Artifact);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub artifact: Artifact,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedDeployment(pub Deployment);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayReceipt {
    pub stack: Stack,
    pub revision: String,
    pub route: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub stack: Stack,
    pub artifacts: usize,
    pub deployments: usize,
    pub gateway_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReceipt {
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyReceipt {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub workflow: String,
    pub stack: Stack,
    pub revision: String,
    pub message: String,
}

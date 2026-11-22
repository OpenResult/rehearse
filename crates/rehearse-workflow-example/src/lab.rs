use crate::model::{
    Artifact, CleanupReceipt, Deployment, GatewayReceipt, Inventory, LabError, LegacyReceipt,
    Session, Source, Stack, Stage, TestedArtifact, VerifiedDeployment,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

const MARKER: &str = ".rehearse-workflow-lab";
const MARKER_CONTENTS: &str = "rehearse workflow fixtures v1\n";

pub struct Lab {
    pub root: PathBuf,
    pub stack: Stack,
    pub revision: String,
    pub fail_at: Option<Stage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    build_command: String,
    test_command: String,
    expected_health: String,
}

#[derive(Serialize, Deserialize)]
struct TestReport {
    stack: Stack,
    revision: String,
    passed: bool,
    observed_health: String,
    artifact_contents: String,
}

#[derive(Serialize, Deserialize)]
struct DeployedService {
    stack: Stack,
    revision: String,
    health: String,
}

#[derive(Serialize, Deserialize)]
struct LegacyRecord {
    stack: Stack,
    fixture_hook_completed: bool,
}

impl Lab {
    pub fn check_failure(&self, stage: Stage) -> Result<(), LabError> {
        if self.fail_at == Some(stage) {
            Err(LabError::Injected { stage })
        } else {
            Ok(())
        }
    }

    fn check_root(&self) -> Result<(), LabError> {
        require_directory(&self.root)?;
        let marker = self.root.join(MARKER);
        if read_text(&marker)? != MARKER_CONTENTS {
            return Err(LabError::invalid(marker, "not a workflow fixture lab"));
        }
        // The caller chooses the parent directory (which may be /tmp or /var
        // on macOS). The root itself and every managed child remain regular
        // directories; none of these namespace components may be a symlink.
        for area in ["sources", "artifacts", "test-reports", "deployments"] {
            require_directory(&self.root.join(area))?;
            require_directory(&self.root.join(area).join(self.stack.as_str()))?;
        }
        for area in ["gateway", "legacy"] {
            require_directory(&self.root.join(area))?;
        }
        validate_revision(&self.revision)?;
        Ok(())
    }

    fn path(&self, area: &str, revision: &str) -> PathBuf {
        self.root
            .join(area)
            .join(self.stack.as_str())
            .join(format!("{revision}.json"))
    }

    fn gateway_path(&self) -> PathBuf {
        self.root
            .join("gateway")
            .join(format!("{}.json", self.stack))
    }

    fn check_artifact(&self, artifact: &Artifact) -> Result<Source, LabError> {
        validate_revision(&artifact.revision)?;
        let path = self.path("artifacts", &artifact.revision);
        if artifact.stack != self.stack || artifact.path != path {
            return Err(LabError::invalid(
                &artifact.path,
                "artifact belongs to another lab or stack",
            ));
        }
        let source: Source = read_json(&path)?;
        if source.stack != artifact.stack || source.revision != artifact.revision {
            return Err(LabError::invalid(
                path,
                "artifact identity does not match its filename",
            ));
        }
        Ok(source)
    }

    pub fn authenticate(&self) -> Result<Session, LabError> {
        self.check_failure(Stage::Authenticate)?;
        self.check_root()?;
        Ok(Session {
            actor: "local-fixture-runner".to_owned(),
        })
    }

    pub fn read_source(&self) -> Result<Source, LabError> {
        self.check_failure(Stage::ReadSource)?;
        self.check_root()?;
        let directory = self.root.join("sources").join(self.stack.as_str());
        let fixture: Fixture = read_json(&directory.join("fixture.json"))?;
        let filename = match self.stack {
            Stack::Rust => "source.rs",
            Stack::SpringBoot => "DemoApplication.java",
        };
        Ok(Source {
            stack: self.stack,
            revision: self.revision.clone(),
            contents: read_text(&directory.join(filename))?,
            build_command: fixture.build_command,
            test_command: fixture.test_command,
            expected_health: fixture.expected_health,
        })
    }

    pub fn build(&self, source: Source) -> Result<Artifact, LabError> {
        self.check_failure(Stage::Build)?;
        self.check_root()?;
        if source.stack != self.stack || source.revision != self.revision {
            return Err(LabError::invalid(
                &self.root,
                "source belongs to another stack or revision",
            ));
        }
        let path = self.path("artifacts", &source.revision);
        // This is a source bundle fixture, never an executable or a JAR.
        write_json(&path, &source, false)?;
        Ok(Artifact {
            stack: self.stack,
            revision: source.revision,
            path,
        })
    }

    pub fn test(&self, artifact: Artifact) -> Result<TestedArtifact, LabError> {
        self.check_failure(Stage::Test)?;
        self.check_root()?;
        let source = self.check_artifact(&artifact)?;
        if source.contents.trim().is_empty() || source.expected_health != "UP" {
            return Err(LabError::invalid(
                &artifact.path,
                "fixture contract tests failed",
            ));
        }
        let report = TestReport {
            stack: self.stack,
            revision: artifact.revision.clone(),
            passed: true,
            observed_health: "UP".to_owned(),
            artifact_contents: read_text(&artifact.path)?,
        };
        write_json(
            &self.path("test-reports", &artifact.revision),
            &report,
            false,
        )?;
        Ok(TestedArtifact(artifact))
    }

    pub fn read_tested_artifact(&self) -> Result<TestedArtifact, LabError> {
        self.check_failure(Stage::ReadSource)?;
        self.check_root()?;
        let artifact = Artifact {
            stack: self.stack,
            revision: self.revision.clone(),
            path: self.path("artifacts", &self.revision),
        };
        self.check_artifact(&artifact)?;
        self.check_test_report(&artifact)?;
        Ok(TestedArtifact(artifact))
    }

    fn check_test_report(&self, artifact: &Artifact) -> Result<(), LabError> {
        let path = self.path("test-reports", &artifact.revision);
        let report: TestReport = read_json(&path)?;
        if report.stack != self.stack
            || report.revision != artifact.revision
            || !report.passed
            || report.observed_health != "UP"
            || report.artifact_contents != read_text(&artifact.path)?
        {
            return Err(LabError::invalid(
                path,
                "artifact has no passing fixture test report",
            ));
        }
        Ok(())
    }

    pub fn deploy(&self, tested: TestedArtifact) -> Result<Deployment, LabError> {
        self.check_failure(Stage::Deploy)?;
        self.check_root()?;
        let artifact = tested.0;
        self.check_artifact(&artifact)?;
        self.check_test_report(&artifact)?;
        let path = self.path("deployments", &artifact.revision);
        write_json(
            &path,
            &DeployedService {
                stack: self.stack,
                revision: artifact.revision.clone(),
                health: "UP".to_owned(),
            },
            false,
        )?;
        Ok(Deployment { artifact, path })
    }

    pub fn verify(&self, deployment: Deployment) -> Result<VerifiedDeployment, LabError> {
        self.check_failure(Stage::Verify)?;
        self.check_root()?;
        validate_revision(&deployment.artifact.revision)?;
        let path = self.path("deployments", &deployment.artifact.revision);
        if deployment.path != path || deployment.artifact.stack != self.stack {
            return Err(LabError::invalid(
                &deployment.path,
                "deployment belongs to another lab or stack",
            ));
        }
        let deployed: DeployedService = read_json(&path)?;
        if deployed.stack != self.stack
            || deployed.revision != deployment.artifact.revision
            || deployed.health != "UP"
        {
            return Err(LabError::invalid(
                path,
                "deployed fixture is unhealthy or has the wrong revision",
            ));
        }
        Ok(VerifiedDeployment(deployment))
    }

    pub fn publish(&self, verified: VerifiedDeployment) -> Result<GatewayReceipt, LabError> {
        self.check_failure(Stage::Gateway)?;
        self.check_root()?;
        let deployment = self.verify(verified.0)?.0;
        let receipt = GatewayReceipt {
            stack: self.stack,
            revision: deployment.artifact.revision,
            route: format!("/api/{}", self.stack),
        };
        write_json(&self.gateway_path(), &receipt, true)?;
        Ok(receipt)
    }

    pub fn inventory(&self) -> Result<Inventory, LabError> {
        self.check_failure(Stage::Observe)?;
        self.check_root()?;
        let gateway = self.read_gateway()?;
        let artifacts = self.records("artifacts")?;
        for (path, revision) in &artifacts {
            let artifact = Artifact {
                stack: self.stack,
                revision: revision.clone(),
                path: path.clone(),
            };
            self.check_artifact(&artifact)?;
        }
        Ok(Inventory {
            stack: self.stack,
            artifacts: artifacts.len(),
            deployments: self.deployment_records()?.len(),
            gateway_revision: gateway.revision,
        })
    }

    fn read_gateway(&self) -> Result<GatewayReceipt, LabError> {
        let path = self.gateway_path();
        let gateway: GatewayReceipt = read_json(&path)?;
        validate_revision(&gateway.revision)?;
        if gateway.stack != self.stack || gateway.route != format!("/api/{}", self.stack) {
            return Err(LabError::invalid(
                path,
                "gateway route does not match this stack",
            ));
        }
        let active_path = self.path("deployments", &gateway.revision);
        let active: DeployedService = read_json(&active_path)?;
        if active.stack != self.stack || active.revision != gateway.revision {
            return Err(LabError::invalid(
                active_path,
                "gateway points at a different deployment",
            ));
        }
        Ok(gateway)
    }

    fn records(&self, area: &str) -> Result<Vec<(PathBuf, String)>, LabError> {
        let directory = self.root.join(area).join(self.stack.as_str());
        require_directory(&directory)?;
        let mut records = Vec::new();
        for entry in
            fs::read_dir(&directory).map_err(|error| LabError::io("list", &directory, error))?
        {
            let path = entry
                .map_err(|error| LabError::io("read entry in", &directory, error))?
                .path();
            require_file(&path)?;
            let revision = path
                .file_stem()
                .and_then(|name| name.to_str())
                .filter(|_| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                })
                .ok_or_else(|| LabError::invalid(&path, "unexpected lab entry"))?;
            validate_revision(revision)?;
            records.push((path.clone(), revision.to_owned()));
        }
        records.sort();
        Ok(records)
    }

    fn deployment_records(&self) -> Result<Vec<(PathBuf, String)>, LabError> {
        let records = self.records("deployments")?;
        for (path, revision) in &records {
            let deployed: DeployedService = read_json(path)?;
            if deployed.stack != self.stack || &deployed.revision != revision {
                return Err(LabError::invalid(
                    path,
                    "deployment identity does not match its filename",
                ));
            }
        }
        Ok(records)
    }

    pub fn cleanup(&self) -> Result<CleanupReceipt, LabError> {
        self.check_failure(Stage::Cleanup)?;
        self.check_root()?;
        let gateway = self.read_gateway()?;
        // Inspect the entire set before deleting anything. Unknown entries and
        // symlinks fail validation; the active gateway revision is preserved.
        let stale: Vec<_> = self
            .deployment_records()?
            .into_iter()
            .filter(|(_, revision)| revision != &gateway.revision)
            .collect();
        for (path, _) in &stale {
            fs::remove_file(path)
                .map_err(|error| LabError::io("remove stale deployment", path, error))?;
        }
        Ok(CleanupReceipt {
            removed: stale.len(),
        })
    }

    pub fn legacy_hook(&self) -> Result<LegacyReceipt, LabError> {
        self.check_failure(Stage::Legacy)?;
        self.check_root()?;
        let path = self
            .root
            .join("legacy")
            .join(format!("{}.json", self.stack));
        write_json(
            &path,
            &LegacyRecord {
                stack: self.stack,
                fixture_hook_completed: true,
            },
            false,
        )?;
        Ok(LegacyReceipt { path })
    }

    pub fn inspect_legacy(&self, receipt: LegacyReceipt) -> Result<String, LabError> {
        self.check_root()?;
        let path = self
            .root
            .join("legacy")
            .join(format!("{}.json", self.stack));
        if receipt.path != path {
            return Err(LabError::invalid(
                receipt.path,
                "legacy receipt belongs to another lab",
            ));
        }
        let record: LegacyRecord = read_json(&path)?;
        if record.stack != self.stack || !record.fixture_hook_completed {
            return Err(LabError::invalid(
                path,
                "legacy fixture hook did not complete",
            ));
        }
        Ok("local legacy fixture hook verified".to_owned())
    }
}

pub fn initialize(root: &Path) -> Result<(), LabError> {
    require_directory(
        root.parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    fs::create_dir(root).map_err(|error| LabError::io("create new lab directory", root, error))?;
    write_bytes(&root.join(MARKER), MARKER_CONTENTS.as_bytes(), false)?;
    for area in [
        "sources",
        "artifacts",
        "test-reports",
        "deployments",
        "gateway",
        "legacy",
    ] {
        let directory = root.join(area);
        fs::create_dir(&directory)
            .map_err(|error| LabError::io("create directory", &directory, error))?;
        if !matches!(area, "gateway" | "legacy") {
            for stack in [Stack::Rust, Stack::SpringBoot] {
                let directory = directory.join(stack.as_str());
                fs::create_dir(&directory)
                    .map_err(|error| LabError::io("create directory", &directory, error))?;
            }
        }
    }
    for stack in [Stack::Rust, Stack::SpringBoot] {
        let directory = root.join("sources").join(stack.as_str());
        let (filename, source, fixture) = match stack {
            Stack::Rust => (
                "source.rs",
                include_str!("../fixtures/rust/source.rs"),
                include_str!("../fixtures/rust/fixture.json"),
            ),
            Stack::SpringBoot => (
                "DemoApplication.java",
                include_str!("../fixtures/spring-boot/DemoApplication.java"),
                include_str!("../fixtures/spring-boot/fixture.json"),
            ),
        };
        write_bytes(&directory.join(filename), source.as_bytes(), false)?;
        write_bytes(&directory.join("fixture.json"), fixture.as_bytes(), false)?;
        let lab = Lab {
            root: root.to_owned(),
            stack,
            revision: "seed".to_owned(),
            fail_at: None,
        };
        let artifact = lab.build(lab.read_source()?)?;
        lab.test(artifact)?;
        for revision in ["previous", "obsolete"] {
            write_json(
                &lab.path("deployments", revision),
                &DeployedService {
                    stack,
                    revision: revision.to_owned(),
                    health: "UP".to_owned(),
                },
                false,
            )?;
        }
        write_json(
            &lab.gateway_path(),
            &GatewayReceipt {
                stack,
                revision: "previous".to_owned(),
                route: format!("/api/{stack}"),
            },
            false,
        )?;
    }
    Ok(())
}

pub fn validate_revision(revision: &str) -> Result<(), LabError> {
    if revision.is_empty()
        || revision.len() > 64
        || !revision.as_bytes()[0].is_ascii_alphanumeric()
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(LabError::invalid(
            revision,
            "revision must be 1-64 ASCII letters, digits, dots, '-' or '_', starting with a letter or digit",
        ));
    }
    Ok(())
}

fn require_directory(path: &Path) -> Result<(), LabError> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(LabError::invalid(
            path,
            "parent traversal is not allowed in a lab path",
        ));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| LabError::io("inspect directory", path, error))?;
    if !metadata.is_dir() {
        return Err(LabError::invalid(
            path,
            "expected a directory, not a symlink or file",
        ));
    }
    Ok(())
}

fn require_file(path: &Path) -> Result<(), LabError> {
    require_directory(path.parent().unwrap_or(Path::new(".")))?;
    let metadata =
        fs::symlink_metadata(path).map_err(|error| LabError::io("inspect file", path, error))?;
    if !metadata.is_file() {
        return Err(LabError::invalid(
            path,
            "expected a regular file, not a symlink or directory",
        ));
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, LabError> {
    require_file(path)?;
    fs::read_to_string(path).map_err(|error| LabError::io("read", path, error))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, LabError> {
    let contents = read_text(path)?;
    serde_json::from_str(&contents).map_err(|source| LabError::Json {
        path: path.to_owned(),
        source,
    })
}

fn write_json<T: Serialize>(path: &Path, value: &T, replace: bool) -> Result<(), LabError> {
    let contents = serde_json::to_vec_pretty(value).map_err(|source| LabError::Json {
        path: path.to_owned(),
        source,
    })?;
    write_bytes(path, &contents, replace)
}

fn write_bytes(path: &Path, contents: &[u8], replace: bool) -> Result<(), LabError> {
    require_directory(path.parent().unwrap_or(Path::new(".")))?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            require_file(path)?;
            if fs::read(path).map_err(|error| LabError::io("read existing file", path, error))?
                == contents
            {
                return Ok(());
            }
            if !replace {
                return Err(LabError::Conflict {
                    path: path.to_owned(),
                });
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(LabError::io("inspect destination", path, error)),
    }
    let destination = if replace {
        path.with_extension(format!("{}-new", std::process::id()))
    } else {
        path.to_owned()
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| LabError::io("create file", &destination, error))?;
    file.write_all(contents)
        .map_err(|error| LabError::io("write", &destination, error))?;
    if replace {
        fs::rename(&destination, path)
            .map_err(|error| LabError::io("replace gateway route", path, error))?;
    }
    Ok(())
}

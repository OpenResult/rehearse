use super::model::{ApprovalSubject, FailurePoint, GateError, ProductionPermit};
use super::state::Run;
use crate::lab::Lab;
use crate::model::{
    Artifact, Deployment, GatewayReceipt, Inventory, LabError, Session, Source, Stack, Summary,
    TestedArtifact, VerifiedDeployment,
};
use rehearse::{
    operation, DryRunAction, DryRunPolicy, Operation, OperationMetadata, Plan, PlanBuilder,
    PlanGraphDescription, SafeDryRun,
};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt::{self, Write as _};
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

/// Per-run application services; the rehearse runtime remains unaware of HTTP.
pub struct RunContext {
    pub run: Arc<Run>,
    pub workspace: PathBuf,
    pub stack: Stack,
    pub revision: String,
    pub fail_at: Option<FailurePoint>,
}

impl RunContext {
    fn lab(&self, environment: Environment) -> Lab {
        Lab {
            root: self.workspace.join(environment.as_str()),
            stack: self.stack,
            revision: self.revision.clone(),
            fail_at: None,
        }
    }

    fn inject(&self, stage: FailurePoint) -> Result<(), WorkflowError> {
        if self.fail_at == Some(stage) {
            Err(WorkflowError::Injected { stage })
        } else {
            Ok(())
        }
    }

    /// Check current test evidence and bytes, rather than trusting a receipt
    /// retained across a potentially long wait for a human decision.
    fn check_candidate(&self, candidate: &Candidate) -> Result<(), WorkflowError> {
        let tested = self.lab(Environment::Dev).read_tested_artifact()?;
        if tested.0.stack != candidate.tested.0.stack
            || tested.0.revision != candidate.tested.0.revision
            || tested.0.path != candidate.tested.0.path
            || digest(&read_artifact(&tested.0)?) != candidate.digest
        {
            return Err(WorkflowError::ArtifactChanged);
        }
        Ok(())
    }

    fn check_environment_bundle(
        &self,
        candidate: &Candidate,
        environment: Environment,
    ) -> Result<(), WorkflowError> {
        let tested = self.lab(environment).read_tested_artifact()?;
        if digest(&read_artifact(&tested.0)?) != candidate.digest {
            return Err(WorkflowError::ArtifactChanged);
        }
        Ok(())
    }

    fn check_non_production(&self, evidence: &VerifiedNonProduction) -> Result<(), WorkflowError> {
        self.check_candidate(&evidence.dev.candidate)?;
        self.check_environment_bundle(&evidence.dev.candidate, Environment::Staging)?;
        self.lab(Environment::Dev)
            .verify(evidence.dev.deployment.clone().0)?;
        self.lab(Environment::Staging)
            .verify(evidence.staging.clone().0)?;
        Ok(())
    }

    fn deploy_candidate(
        &self,
        candidate: &Candidate,
        environment: Environment,
    ) -> Result<Deployment, WorkflowError> {
        self.check_candidate(candidate)?;
        let lab = self.lab(environment);
        // The fixture adapter materializes the identical bundle and its test
        // evidence in the target environment. These local writes are included
        // in the deploy operation's declared Write impact; no tool is invoked.
        let artifact = lab.build(candidate.source.clone())?;
        let tested = lab.test(artifact)?;
        Ok(lab.deploy(tested)?)
    }
}

#[derive(Debug)]
pub enum WorkflowError {
    Lab(LabError),
    Gate(GateError),
    Injected { stage: FailurePoint },
    ArtifactChanged,
    ApprovalMismatch,
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lab(error) => error.fmt(f),
            Self::Gate(error) => error.fmt(f),
            Self::Injected { stage } => write!(
                f,
                "injected failure at {}",
                match stage {
                    FailurePoint::Build => "build",
                    FailurePoint::Test => "test",
                    FailurePoint::VerifyDev => "verify-dev",
                    FailurePoint::VerifyStaging => "verify-staging",
                    FailurePoint::VerifyProduction => "verify-production",
                }
            ),
            Self::ArtifactChanged => {
                f.write_str("the tested artifact changed after release evidence was collected")
            }
            Self::ApprovalMismatch => {
                f.write_str("production approval does not match this artifact and environment")
            }
        }
    }
}

impl Error for WorkflowError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Lab(error) => Some(error),
            Self::Gate(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LabError> for WorkflowError {
    fn from(error: LabError) -> Self {
        Self::Lab(error)
    }
}

impl From<GateError> for WorkflowError {
    fn from(error: GateError) -> Self {
        Self::Gate(error)
    }
}

/// Rehearsal never opens a human question or fabricates an approval permit.
#[derive(Debug, Clone, Copy)]
pub struct ServerDryRun;

impl DryRunPolicy for ServerDryRun {
    fn action(&self, metadata: &OperationMetadata) -> DryRunAction {
        if metadata
            .annotations()
            .get("kind")
            .is_some_and(|kind| kind == "approval")
        {
            DryRunAction::Skip
        } else {
            SafeDryRun.action(metadata)
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Environment {
    Dev,
    Staging,
    Production,
}

impl Environment {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }
}

#[derive(Clone)]
struct Candidate {
    source: Source,
    tested: TestedArtifact,
    digest: String,
}

impl Candidate {
    fn approval_subject(&self) -> ApprovalSubject {
        ApprovalSubject {
            artifact_digest: self.digest.clone(),
            revision: self.tested.0.revision.clone(),
            environment: "production".to_owned(),
        }
    }
}

#[derive(Clone)]
struct DevDeployment {
    candidate: Candidate,
    deployment: Deployment,
}

#[derive(Clone)]
struct VerifiedDev {
    candidate: Candidate,
    deployment: VerifiedDeployment,
}

#[derive(Clone)]
struct StagingDeployment {
    candidate: Candidate,
    deployment: Deployment,
}

#[derive(Clone)]
struct VerifiedStaging {
    candidate: Candidate,
    deployment: VerifiedDeployment,
}

#[derive(Clone)]
struct VerifiedNonProduction {
    dev: VerifiedDev,
    staging: VerifiedDeployment,
}

#[derive(Clone)]
struct ApprovedRelease {
    non_production: VerifiedNonProduction,
    permit: ProductionPermit,
}

impl ApprovedRelease {
    fn candidate(&self) -> &Candidate {
        &self.non_production.dev.candidate
    }

    fn check_permit(&self, run_id: &str) -> Result<(), WorkflowError> {
        if self.permit.run_id != run_id
            || self.permit.gate_id != "production"
            || self.permit.subject != self.candidate().approval_subject()
        {
            return Err(WorkflowError::ApprovalMismatch);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct ProductionDeployment {
    release: ApprovedRelease,
    deployment: Deployment,
}

#[derive(Clone)]
struct VerifiedProduction {
    release: ApprovedRelease,
    deployment: VerifiedDeployment,
}

fn read_artifact(artifact: &Artifact) -> Result<Vec<u8>, LabError> {
    fs::read(&artifact.path)
        .map_err(|error| LabError::io("read tested artifact", &artifact.path, error))
}

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(71);
    output.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").expect("formatting a digest into a String cannot fail");
    }
    output
}

#[operation(impact = session)]
async fn authenticate(#[context] context: &RunContext) -> Result<Session, WorkflowError> {
    context.run.checkpoint().await?;
    Ok(context.lab(Environment::Dev).authenticate()?)
}

#[operation(impact = read)]
async fn read_source(
    #[context] context: &RunContext,
    session: Session,
) -> Result<Source, WorkflowError> {
    context.run.checkpoint().await?;
    if session.actor.is_empty() {
        return Err(LabError::invalid(&context.workspace, "fixture session has no actor").into());
    }
    Ok(context.lab(Environment::Dev).read_source()?)
}

#[operation(impact = write)]
async fn build(#[context] context: &RunContext, source: Source) -> Result<Artifact, WorkflowError> {
    context.run.checkpoint().await?;
    context.inject(FailurePoint::Build)?;
    Ok(context.lab(Environment::Dev).build(source)?)
}

#[operation(impact = write)]
async fn test(
    #[context] context: &RunContext,
    artifact: Artifact,
) -> Result<Candidate, WorkflowError> {
    context.run.checkpoint().await?;
    context.inject(FailurePoint::Test)?;
    let lab = context.lab(Environment::Dev);
    let tested = lab.test(artifact)?;
    // Bind both the permit and the test report to the exact immutable bundle.
    lab.read_tested_artifact()?;
    let bytes = read_artifact(&tested.0)?;
    let source = serde_json::from_slice(&bytes).map_err(|source| LabError::Json {
        path: tested.0.path.clone(),
        source,
    })?;
    Ok(Candidate {
        source,
        tested,
        digest: digest(&bytes),
    })
}

#[operation(impact = write)]
async fn deploy_dev(
    #[context] context: &RunContext,
    candidate: Candidate,
) -> Result<DevDeployment, WorkflowError> {
    context.run.checkpoint().await?;
    context.check_candidate(&candidate)?;
    let deployment = context
        .lab(Environment::Dev)
        .deploy(candidate.tested.clone())?;
    Ok(DevDeployment {
        candidate,
        deployment,
    })
}

#[operation(impact = read)]
async fn verify_dev(
    #[context] context: &RunContext,
    deployed: DevDeployment,
) -> Result<VerifiedDev, WorkflowError> {
    context.run.checkpoint().await?;
    context.inject(FailurePoint::VerifyDev)?;
    context.check_candidate(&deployed.candidate)?;
    let deployment = context.lab(Environment::Dev).verify(deployed.deployment)?;
    Ok(VerifiedDev {
        candidate: deployed.candidate,
        deployment,
    })
}

#[operation(impact = write)]
async fn deploy_staging(
    #[context] context: &RunContext,
    candidate: Candidate,
) -> Result<StagingDeployment, WorkflowError> {
    context.run.checkpoint().await?;
    let deployment = context.deploy_candidate(&candidate, Environment::Staging)?;
    Ok(StagingDeployment {
        candidate,
        deployment,
    })
}

#[operation(impact = read)]
async fn verify_staging(
    #[context] context: &RunContext,
    staged: StagingDeployment,
) -> Result<VerifiedStaging, WorkflowError> {
    context.run.checkpoint().await?;
    context.inject(FailurePoint::VerifyStaging)?;
    context.check_environment_bundle(&staged.candidate, Environment::Staging)?;
    let staging = context
        .lab(Environment::Staging)
        .verify(staged.deployment)?;
    Ok(VerifiedStaging {
        candidate: staged.candidate,
        deployment: staging,
    })
}

#[operation(impact = session)]
async fn request_production_approval(
    #[context] context: &RunContext,
    dev: VerifiedDev,
    staging: VerifiedStaging,
) -> Result<ApprovedRelease, WorkflowError> {
    context.run.checkpoint().await?;
    if dev.candidate.digest != staging.candidate.digest
        || dev.candidate.source.revision != staging.candidate.source.revision
    {
        return Err(WorkflowError::ArtifactChanged);
    }
    let evidence = VerifiedNonProduction {
        dev,
        staging: staging.deployment,
    };
    context.check_non_production(&evidence)?;
    let subject = evidence.dev.candidate.approval_subject();
    let permit = context.run.request_approval(subject).await?;
    let approved = ApprovedRelease {
        non_production: evidence,
        permit,
    };
    approved.check_permit(context.run.id())?;
    Ok(approved)
}

#[operation(impact = write)]
async fn deploy_production(
    #[context] context: &RunContext,
    release: ApprovedRelease,
) -> Result<ProductionDeployment, WorkflowError> {
    context.run.checkpoint().await?;
    release.check_permit(context.run.id())?;
    context.check_non_production(&release.non_production)?;
    // A decision authorizes the captured bundle, not a later rebuilt artifact.
    let deployment = context.deploy_candidate(release.candidate(), Environment::Production)?;
    Ok(ProductionDeployment {
        release,
        deployment,
    })
}

#[operation(impact = read)]
async fn verify_production(
    #[context] context: &RunContext,
    deployed: ProductionDeployment,
) -> Result<VerifiedProduction, WorkflowError> {
    context.run.checkpoint().await?;
    context.inject(FailurePoint::VerifyProduction)?;
    context.check_environment_bundle(deployed.release.candidate(), Environment::Production)?;
    let deployment = context
        .lab(Environment::Production)
        .verify(deployed.deployment)?;
    Ok(VerifiedProduction {
        release: deployed.release,
        deployment,
    })
}

#[operation(impact = write)]
async fn publish_api_gateway(
    #[context] context: &RunContext,
    verified: VerifiedProduction,
) -> Result<GatewayReceipt, WorkflowError> {
    context.run.checkpoint().await?;
    verified.release.check_permit(context.run.id())?;
    context.check_candidate(verified.release.candidate())?;
    context.check_environment_bundle(verified.release.candidate(), Environment::Production)?;
    Ok(context
        .lab(Environment::Production)
        .publish(verified.deployment)?)
}

#[operation(impact = read)]
async fn observe_gateway(#[context] context: &RunContext) -> Result<Inventory, WorkflowError> {
    context.run.checkpoint().await?;
    Ok(context.lab(Environment::Production).inventory()?)
}

#[operation(impact = pure)]
async fn summarize(
    gateway: GatewayReceipt,
    inventory: Inventory,
) -> Result<Summary, WorkflowError> {
    Ok(Summary {
        workflow: "server-release".to_owned(),
        stack: gateway.stack,
        revision: gateway.revision,
        message: format!(
            "Approved fixture release published to production route {}; {} deployment(s) retained.",
            gateway.route, inventory.deployments
        ),
    })
}

/// Identifies the application code/configuration contract behind this graph.
/// Update this when changing executable operation behavior or configuration.
pub const PLAN_REVISION: &str = concat!(env!("CARGO_PKG_VERSION"), ":server-parallel-release-v1");

fn annotated<T: Clone + Send + Sync + 'static>(
    operation: Operation<RunContext, T, WorkflowError>,
    label: &str,
    environment: &str,
) -> Operation<RunContext, T, WorkflowError> {
    operation
        .with_annotation("label", label)
        .with_annotation("environment", environment)
}

/// Build the static plan without accessing a context, values, or fixture files.
pub fn plan() -> Plan<RunContext, Summary, WorkflowError> {
    let mut builder = PlanBuilder::new("production_release").with_revision(PLAN_REVISION);
    let session = builder.add(annotated(
        authenticate(),
        "Authenticate local session",
        "build",
    ));
    let source = builder.add(annotated(
        read_source(session),
        "Read source fixture",
        "build",
    ));
    let artifact = builder.add(annotated(build(source), "Build fixture bundle", "build"));
    let candidate = builder.add(annotated(test(artifact), "Test fixture bundle", "build"));
    let (dev, staging) = builder.parallel(
        "Non-production deployments",
        NonZeroUsize::new(2).expect("nonzero limit"),
        |group| {
            let dev = group.branch("Dev", |branch| {
                let deployed = branch.add(annotated(deploy_dev(candidate), "Deploy to dev", "dev"));
                branch.add(annotated(
                    verify_dev(deployed),
                    "Verify dev deployment",
                    "dev",
                ))
            });
            let staging = group.branch("Staging", |branch| {
                let deployed = branch.add(annotated(
                    deploy_staging(candidate),
                    "Deploy to staging",
                    "staging",
                ));
                branch.add(annotated(
                    verify_staging(deployed),
                    "Verify staging deployment",
                    "staging",
                ))
            });
            (dev, staging)
        },
    );
    let approved = builder.add(
        annotated(
            request_production_approval(dev, staging),
            "Approve production deployment",
            "production",
        )
        .with_annotation("kind", "approval"),
    );
    let production = builder.add(annotated(
        deploy_production(approved),
        "Deploy to production",
        "production",
    ));
    let production = builder.add(annotated(
        verify_production(production),
        "Verify production deployment",
        "production",
    ));
    let gateway = builder.add(annotated(
        publish_api_gateway(production),
        "Publish API gateway route",
        "gateway",
    ));
    // No value edge from publication: rehearsal still observes the current route.
    let inventory = builder.add(annotated(
        observe_gateway(),
        "Observe gateway independently",
        "gateway",
    ));
    let summary = builder.add(annotated(
        summarize(gateway, inventory),
        "Summarize release",
        "summary",
    ));
    builder.finish(summary)
}

/// The HTTP plan response is the library's canonical Describe graph JSON.
pub fn plan_snapshot() -> PlanGraphDescription {
    plan().describe_graph()
}

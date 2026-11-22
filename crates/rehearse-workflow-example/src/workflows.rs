use crate::lab::Lab;
use crate::model::{
    Artifact, CleanupReceipt, Deployment, GatewayReceipt, Inventory, LabError, LegacyReceipt,
    Session, Source, Stage, Summary, TestedArtifact, VerifiedDeployment,
};
use clap::ValueEnum;
use rehearse::{
    operation, pipeline, Impact, Input, Operation, OperationMetadata, Plan, PlanBuilder,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Workflow {
    Ci,
    Release,
    Promote,
    Audit,
    Maintenance,
}

pub fn plan(workflow: Workflow, fail_at: Option<Stage>) -> Plan<Lab, Summary, LabError> {
    // Select one static plan before interpreting it; no runtime workflow branching.
    match workflow {
        Workflow::Ci => ci(fail_at),
        Workflow::Release => release(fail_at),
        Workflow::Promote => promote(),
        Workflow::Audit => audit(fail_at),
        Workflow::Maintenance => maintenance(),
    }
}

#[operation(impact = session)]
async fn authenticate(#[context] lab: &Lab) -> Result<Session, LabError> {
    lab.authenticate()
}

#[operation(impact = read)]
async fn read_source(#[context] lab: &Lab, session: Session) -> Result<Source, LabError> {
    if session.actor.is_empty() {
        return Err(LabError::invalid(&lab.root, "fixture session has no actor"));
    }
    lab.read_source()
}

fn validate(source: Source, fail_at: Option<Stage>) -> Result<Source, LabError> {
    if fail_at == Some(Stage::Validate) {
        return Err(LabError::Injected {
            stage: Stage::Validate,
        });
    }
    if source.contents.trim().is_empty() || source.expected_health != "UP" {
        return Err(LabError::invalid(
            "source fixture",
            "source must be nonempty and its expected health must be UP",
        ));
    }
    Ok(source)
}

#[operation(impact = pure)]
async fn validate_source(source: Source, fail_at: Option<Stage>) -> Result<Source, LabError> {
    validate(source, fail_at)
}

// Fixture builds and tests persist bundles/reports, so both declare Write.
#[operation(impact = write)]
async fn build(#[context] lab: &Lab, source: Source) -> Result<Artifact, LabError> {
    lab.build(source)
}

#[operation(impact = write)]
async fn test(#[context] lab: &Lab, artifact: Artifact) -> Result<TestedArtifact, LabError> {
    lab.test(artifact)
}

#[operation(impact = read)]
async fn verify_test_report(
    #[context] lab: &Lab,
    tested: TestedArtifact,
) -> Result<TestedArtifact, LabError> {
    let recorded = lab.read_tested_artifact()?;
    if recorded.0.revision != tested.0.revision || recorded.0.stack != tested.0.stack {
        return Err(LabError::invalid(
            &tested.0.path,
            "test report belongs to another artifact",
        ));
    }
    Ok(tested)
}

#[operation(impact = read)]
async fn read_tested_artifact(
    #[context] lab: &Lab,
    session: Session,
) -> Result<TestedArtifact, LabError> {
    if session.actor.is_empty() {
        return Err(LabError::invalid(&lab.root, "fixture session has no actor"));
    }
    lab.read_tested_artifact()
}

#[operation(impact = write)]
async fn deploy(#[context] lab: &Lab, tested: TestedArtifact) -> Result<Deployment, LabError> {
    lab.deploy(tested)
}

#[operation(impact = read)]
async fn verify_deploy(
    #[context] lab: &Lab,
    deployment: Deployment,
) -> Result<VerifiedDeployment, LabError> {
    lab.verify(deployment)
}

#[operation(impact = write)]
async fn publish_api_gateway(
    #[context] lab: &Lab,
    verified: VerifiedDeployment,
) -> Result<GatewayReceipt, LabError> {
    lab.publish(verified)
}

#[operation(impact = read)]
async fn observe_gateway(#[context] lab: &Lab) -> Result<Inventory, LabError> {
    lab.inventory()
}

#[operation(impact = pure)]
async fn summarize_release(
    receipt: GatewayReceipt,
    inventory: Inventory,
    workflow: String,
) -> Result<Summary, LabError> {
    Ok(Summary {
        workflow,
        stack: receipt.stack,
        revision: receipt.revision,
        message: format!(
            "Verified fixture deployment and published local gateway route {}; {} deployment(s) retained.",
            receipt.route, inventory.deployments
        ),
    })
}

#[operation(impact = pure)]
async fn summarize_ci(tested: TestedArtifact, inventory: Inventory) -> Result<Summary, LabError> {
    Ok(Summary {
        workflow: "ci".to_owned(),
        stack: tested.0.stack,
        revision: tested.0.revision,
        message: format!(
            "Fixture bundle and test report verified; gateway remains on {}.",
            inventory.gateway_revision
        ),
    })
}

#[pipeline]
fn release(fail_at: Option<Stage>) -> Plan<Lab, Summary, LabError> {
    let session = step!(authenticate())?;
    let source = step!(read_source(session))?;
    let source = step!(validate_source(source, fail_at))?;
    let artifact = step!(build(source))?;
    let tested = step!(test(artifact))?;
    let deployment = step!(deploy(tested))?;
    let verified = step!(verify_deploy(deployment))?;
    let gateway = step!(publish_api_gateway(verified))?;
    // No Value input from publish: this still runs in dry-run after skipped writes.
    let inventory = step!(observe_gateway())?;
    let summary = step!(summarize_release(gateway, inventory, "release".to_owned()))?;
    Ok(summary)
}

#[pipeline]
fn ci(fail_at: Option<Stage>) -> Plan<Lab, Summary, LabError> {
    let session = step!(authenticate())?;
    let source = step!(read_source(session))?;
    let source = step!(validate_source(source, fail_at))?;
    let artifact = step!(build(source))?;
    let tested = step!(test(artifact))?;
    let verified = step!(verify_test_report(tested))?;
    let inventory = step!(observe_gateway())?;
    let summary = step!(summarize_ci(verified, inventory))?;
    Ok(summary)
}

#[pipeline]
fn promote() -> Plan<Lab, Summary, LabError> {
    let session = step!(authenticate())?;
    let tested = step!(read_tested_artifact(session))?;
    let deployment = step!(deploy(tested))?;
    let verified = step!(verify_deploy(deployment))?;
    let gateway = step!(publish_api_gateway(verified))?;
    let inventory = step!(observe_gateway())?;
    let summary = step!(summarize_release(gateway, inventory, "promote".to_owned()))?;
    Ok(summary)
}

// The manual frontend expresses the same delayed, typed, validated plan model.
fn audit(fail_at: Option<Stage>) -> Plan<Lab, Summary, LabError> {
    let mut builder = PlanBuilder::<Lab, LabError>::new("audit");
    let session = builder.add(Operation::sync(
        OperationMetadata::new("authenticate", Impact::Session),
        (),
        |lab: &Lab, ()| lab.authenticate(),
    ));
    let source = builder.add(Operation::sync(
        OperationMetadata::new("read_source", Impact::Read),
        Input::value(session),
        |lab: &Lab, _session: Session| lab.read_source(),
    ));
    let validated = builder.add(Operation::sync(
        OperationMetadata::new("validate_source", Impact::Pure),
        Input::value(source),
        move |_: &Lab, source| validate(source, fail_at),
    ));
    let inventory = builder.add(Operation::sync(
        OperationMetadata::new("observe_gateway", Impact::Read),
        (),
        |lab: &Lab, ()| lab.inventory(),
    ));
    let summary = builder.add(Operation::sync(
        OperationMetadata::new("summarize_audit", Impact::Pure),
        (Input::value(validated), Input::value(inventory)),
        |_: &Lab, (source, inventory)| {
            Ok(Summary {
                workflow: "audit".to_owned(),
                stack: source.stack,
                revision: source.revision,
                message: format!(
                    "Source fixture valid; {} artifact(s), {} deployment(s), gateway on {}.",
                    inventory.artifacts, inventory.deployments, inventory.gateway_revision
                ),
            })
        },
    ));
    builder
        .try_finish(summary)
        .expect("audit plan has valid local handles")
}

#[operation(impact = read)]
async fn inventory_deployments(#[context] lab: &Lab) -> Result<Inventory, LabError> {
    lab.inventory()
}

#[operation(impact = delete)]
async fn cleanup_stale_deployments(
    #[context] lab: &Lab,
    inventory: Inventory,
) -> Result<CleanupReceipt, LabError> {
    if inventory.stack != lab.stack {
        return Err(LabError::invalid(
            &lab.root,
            "inventory belongs to another stack",
        ));
    }
    lab.cleanup()
}

#[operation(impact = read)]
async fn verify_cleanup(
    #[context] lab: &Lab,
    receipt: CleanupReceipt,
) -> Result<CleanupReceipt, LabError> {
    let inventory = lab.inventory()?;
    if inventory.deployments != 1 {
        return Err(LabError::invalid(
            &lab.root,
            "cleanup should retain only the active deployment",
        ));
    }
    Ok(receipt)
}

#[operation(impact = opaque)]
async fn legacy_hook(
    #[context] lab: &Lab,
    _cleanup: CleanupReceipt,
) -> Result<LegacyReceipt, LabError> {
    lab.legacy_hook()
}

#[operation(impact = read)]
async fn inspect_legacy_hook(
    #[context] lab: &Lab,
    receipt: LegacyReceipt,
) -> Result<String, LabError> {
    lab.inspect_legacy(receipt)
}

#[operation(impact = pure)]
async fn summarize_maintenance(
    cleanup: CleanupReceipt,
    hook: String,
    inventory: Inventory,
) -> Result<Summary, LabError> {
    Ok(Summary {
        workflow: "maintenance".to_owned(),
        stack: inventory.stack,
        revision: inventory.gateway_revision,
        message: format!("Removed {} stale deployment(s); {hook}.", cleanup.removed),
    })
}

#[pipeline]
fn maintenance() -> Plan<Lab, Summary, LabError> {
    let inventory = step!(inventory_deployments())?;
    let cleanup = step!(cleanup_stale_deployments(inventory))?;
    let cleaned = step!(verify_cleanup(cleanup))?;
    let legacy = step!(legacy_hook(cleaned))?;
    let hook = step!(inspect_legacy_hook(legacy))?;
    let current = step!(observe_gateway())?;
    let summary = step!(summarize_maintenance(cleaned, hook, current))?;
    Ok(summary)
}

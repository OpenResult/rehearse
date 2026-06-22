use rehearse::{operation, pipeline, Impact, Input, NodeOutcome, Plan, PlanBuilder};
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
struct PipelineError;

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("pipeline error")
    }
}

impl std::error::Error for PipelineError {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeployInput {
    credentials: String,
    app: String,
    desired: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session(String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Deployment(String);

#[derive(Clone, Default)]
struct Services {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl Services {
    fn record(&self, name: &'static str) {
        self.calls.lock().expect("calls mutex poisoned").push(name);
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("calls mutex poisoned").clone()
    }
}

#[operation(impact = session)]
async fn login(
    #[context] services: &Services,
    credentials: String,
) -> Result<Session, PipelineError> {
    services.record("login");
    Ok(Session(credentials))
}

#[operation(impact = read)]
async fn read_current(
    #[context] services: &Services,
    session: Session,
    app: String,
) -> Result<String, PipelineError> {
    services.record("read_current");
    Ok(format!("{}:{app}", session.0))
}

#[operation(impact = pure)]
async fn calculate_changes(current: String, desired: String) -> Result<String, PipelineError> {
    Ok(format!("{current}->{desired}"))
}

#[operation(impact = write)]
async fn apply_changes(
    #[context] services: &Services,
    changes: String,
) -> Result<Deployment, PipelineError> {
    services.record("apply_changes");
    Ok(Deployment(changes))
}

#[operation(impact = read)]
async fn read_account_quota(
    #[context] services: &Services,
    app: String,
) -> Result<u32, PipelineError> {
    services.record("read_account_quota");
    Ok(app.len() as u32)
}

#[operation(impact = read)]
async fn verify_deployment(
    #[context] services: &Services,
    session: Session,
    deployment: Deployment,
) -> Result<(), PipelineError> {
    services.record("verify_deployment");
    let _ = (session, deployment);
    Ok(())
}

#[operation(impact = delete)]
async fn delete_old_releases(
    #[context] services: &Services,
    app: String,
) -> Result<(), PipelineError> {
    services.record("delete_old_releases");
    let _ = app;
    Ok(())
}

#[pipeline]
fn deploy(input: DeployInput) -> Plan<Services, Deployment, PipelineError> {
    let app = input.app.clone();

    let session = rehearse::step!(login(input.credentials))?;
    let current = rehearse::step!(read_current(session, app.clone()))?;
    let changes = rehearse::step!(calculate_changes(current, input.desired))?;
    let deployment = rehearse::step!(apply_changes(changes))?;
    rehearse::step!(read_account_quota(app.clone()))?;
    rehearse::step!(verify_deployment(session, deployment))?;
    rehearse::step!(delete_old_releases(app))?;

    Ok(deployment)
}

#[tokio::test]
async fn pipeline_constructor_builds_plan_without_invoking_bodies() {
    let services = Services::default();
    let plan = deploy(input());

    assert_eq!(services.calls(), Vec::<&str>::new());
    assert_eq!(plan.name(), "deploy");
    assert_eq!(plan.len(), 7);
    assert_eq!(
        plan.describe()
            .iter()
            .map(|row| (row.name().to_owned(), row.impact()))
            .collect::<Vec<_>>(),
        vec![
            ("login".to_owned(), Impact::Session),
            ("read_current".to_owned(), Impact::Read),
            ("calculate_changes".to_owned(), Impact::Pure),
            ("apply_changes".to_owned(), Impact::Write),
            ("read_account_quota".to_owned(), Impact::Read),
            ("verify_deployment".to_owned(), Impact::Read),
            ("delete_old_releases".to_owned(), Impact::Delete),
        ]
    );
}

#[tokio::test]
async fn pipeline_execute_runs_steps_in_source_order() {
    let services = Services::default();
    let output = deploy(input()).execute(&services).await.unwrap();

    assert_eq!(output, Deployment("alice:api->desired".to_owned()));
    assert_eq!(
        services.calls(),
        vec![
            "login",
            "read_current",
            "apply_changes",
            "read_account_quota",
            "verify_deployment",
            "delete_old_releases",
        ]
    );
}

#[tokio::test]
async fn pipeline_dry_run_preserves_dependency_semantics() {
    let services = Services::default();
    let report = deploy(input()).dry_run(&services).await;

    assert_eq!(
        services.calls(),
        vec!["login", "read_current", "read_account_quota"]
    );
    assert!(matches!(
        report
            .iter()
            .find(|node| node.name() == "apply_changes")
            .expect("apply report")
            .outcome(),
        NodeOutcome::Skipped { reason } if reason == "write operation"
    ));
    assert!(matches!(
        report
            .iter()
            .find(|node| node.name() == "verify_deployment")
            .expect("verify report")
            .outcome(),
        NodeOutcome::Blocked { .. }
    ));
}

#[operation(impact = pure)]
async fn add_one(value: u32) -> Result<u32, PipelineError> {
    Ok(value + 1)
}

#[pipeline]
fn contextless_pipeline(value: u32) -> Plan<Services, u32, PipelineError> {
    let output = rehearse::step!(add_one(value))?;
    Ok(output)
}

#[tokio::test]
async fn pipeline_supports_contextless_operations_in_contextful_plans() {
    let services = Services::default();
    let output = contextless_pipeline(41).execute(&services).await.unwrap();

    assert_eq!(output, 42);
}

#[test]
fn pipeline_lowering_matches_manual_plan_shape() {
    let macro_plan = deploy(input());

    let mut builder = PlanBuilder::<Services, PipelineError>::new("deploy");
    let session = builder.add(login("alice".to_owned()));
    let current = builder.add(read_current(session, Input::literal("api".to_owned())));
    let changes = builder.add(calculate_changes(current, "desired".to_owned()));
    let deployment = builder.add(apply_changes(changes));
    builder.add(read_account_quota("api".to_owned()));
    builder.add(verify_deployment(session, deployment));
    builder.add(delete_old_releases("api".to_owned()));
    let manual_plan = builder.finish(deployment);

    let macro_rows = macro_plan
        .describe()
        .iter()
        .map(|row| row.name().to_owned())
        .collect::<Vec<_>>();
    let manual_rows = manual_plan
        .describe()
        .iter()
        .map(|row| row.name().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(macro_rows, manual_rows);
}

fn input() -> DeployInput {
    DeployInput {
        credentials: "alice".to_owned(),
        app: "api".to_owned(),
        desired: "desired".to_owned(),
    }
}

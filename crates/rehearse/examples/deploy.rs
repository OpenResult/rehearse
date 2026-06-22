use rehearse::{operation, pipeline, Plan};
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeployError;

impl fmt::Display for DeployError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("deploy operation failed")
    }
}

impl Error for DeployError {}

#[derive(Clone, Debug)]
struct DeployInput {
    credentials: String,
    app: String,
    desired: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session(String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct CurrentState(String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChangeSet(String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Deployment {
    id: String,
}

#[derive(Clone, Default)]
struct Services {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl Services {
    fn record(&self, call: &'static str) {
        self.calls.lock().expect("calls mutex poisoned").push(call);
    }
}

#[operation(impact = session)]
async fn login(
    #[context] services: &Services,
    credentials: String,
) -> Result<Session, DeployError> {
    services.record("login");
    Ok(Session(format!("session:{credentials}")))
}

#[operation(impact = read)]
async fn read_current(
    #[context] services: &Services,
    session: Session,
    app: String,
) -> Result<CurrentState, DeployError> {
    services.record("read_current");
    Ok(CurrentState(format!("{}:{app}:current", session.0)))
}

#[operation(impact = pure)]
async fn calculate_changes(
    current: CurrentState,
    desired: String,
) -> Result<ChangeSet, DeployError> {
    Ok(ChangeSet(format!("{}->{desired}", current.0)))
}

#[operation(impact = write)]
async fn apply_changes(
    #[context] services: &Services,
    session: Session,
    changes: ChangeSet,
) -> Result<Deployment, DeployError> {
    services.record("apply_changes");
    Ok(Deployment {
        id: format!("{}:{}", session.0, changes.0),
    })
}

#[operation(impact = read)]
async fn verify_deployment(
    #[context] services: &Services,
    session: Session,
    deployment: Deployment,
) -> Result<(), DeployError> {
    services.record("verify_deployment");
    let _ = (session, deployment);
    Ok(())
}

#[operation(impact = delete)]
async fn delete_old_releases(
    #[context] services: &Services,
    session: Session,
    app: String,
) -> Result<(), DeployError> {
    services.record("delete_old_releases");
    let _ = (session, app);
    Ok(())
}

#[pipeline]
fn deploy(input: DeployInput) -> Plan<Services, Deployment, DeployError> {
    let app = input.app.clone();

    let session = rehearse::step!(login(input.credentials))?;
    let current = rehearse::step!(read_current(session, app.clone()))?;
    let changes = rehearse::step!(calculate_changes(current, input.desired))?;
    let deployment = rehearse::step!(apply_changes(session, changes))?;
    rehearse::step!(verify_deployment(session, deployment))?;
    rehearse::step!(delete_old_releases(session, app))?;

    Ok(deployment)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let input = DeployInput {
        credentials: "alice".to_owned(),
        app: "api".to_owned(),
        desired: "v2".to_owned(),
    };
    let services = Services::default();
    let plan = deploy(input);

    println!("{}", plan.describe());

    let report = plan.dry_run(&services).await;
    println!("{report}");
    report.require_no_failures()?;

    let deployment = plan.execute(&services).await?;
    println!("executed deployment {}", deployment.id);

    Ok(())
}

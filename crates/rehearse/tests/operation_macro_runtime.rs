use rehearse::{operation, DryRunAction, Impact, Input, NodeOutcome, PlanBuilder};
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacroError;

impl fmt::Display for MacroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("operation failed")
    }
}

impl std::error::Error for MacroError {}

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
async fn login(#[context] services: &Services, username: String) -> Result<Session, MacroError> {
    services.record("login");
    Ok(Session(username))
}

#[operation(impact = read)]
async fn read_current(
    #[context] services: &Services,
    session: Session,
    app: String,
) -> Result<String, MacroError> {
    services.record("read_current");
    Ok(format!("{}:{app}", session.0))
}

#[operation(impact = pure)]
async fn calculate_changes(current: String, desired: String) -> Result<String, MacroError> {
    Ok(format!("{current}->{desired}"))
}

#[operation(impact = write)]
async fn apply_changes(
    #[context] services: &Services,
    changes: String,
) -> Result<Deployment, MacroError> {
    services.record("apply_changes");
    Ok(Deployment(changes))
}

#[tokio::test]
async fn operation_macro_builds_and_executes_manual_plan() {
    let services = Services::default();
    let mut builder = PlanBuilder::<Services, MacroError>::new("macro_deploy");

    let session = builder.add(login("alice".to_owned()));
    let current = builder.add(read_current(session, Input::literal("api".to_owned())));
    let changes = builder.add(calculate_changes(current, "desired".to_owned()));
    let deployment = builder.add(apply_changes(Input::value(changes)));
    let plan = builder.finish(deployment);

    assert_eq!(services.calls(), Vec::<&str>::new());
    assert_eq!(
        plan.describe().iter().nth(3).unwrap().impact(),
        Impact::Write
    );
    assert_eq!(
        plan.describe().iter().nth(3).unwrap().dry_run_action(),
        DryRunAction::Skip
    );

    let output = plan.execute(&services).await.expect("execute succeeds");

    assert_eq!(output, Deployment("alice:api->desired".to_owned()));
    assert_eq!(
        services.calls(),
        vec!["login", "read_current", "apply_changes"]
    );
}

#[tokio::test]
async fn operation_macro_dry_run_preserves_write_skip_semantics() {
    let services = Services::default();
    let mut builder = PlanBuilder::<Services, MacroError>::new("macro_dry_run");

    let session = builder.add(login("alice".to_owned()));
    let current = builder.add(read_current(session, "api".to_owned()));
    let changes = builder.add(calculate_changes(current, "desired".to_owned()));
    let deployment = builder.add(apply_changes(changes));
    let plan = builder.finish(deployment);

    let report = plan.dry_run(&services).await;

    assert_eq!(services.calls(), vec!["login", "read_current"]);
    assert!(matches!(
        report.iter().last().expect("write report").outcome(),
        NodeOutcome::Skipped { reason } if reason == "write operation"
    ));
}

#[operation(impact = read)]
async fn contextless_read(value: u32) -> Result<u32, MacroError> {
    Ok(value + 1)
}

#[operation(impact = pure)]
#[allow(clippy::too_many_arguments)]
async fn sum_eight(
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
    f: u32,
    g: u32,
    h: u32,
) -> Result<u32, MacroError> {
    Ok(a + b + c + d + e + f + g + h)
}

#[tokio::test]
async fn contextless_operation_composes_into_contextful_plan() {
    let services = Services::default();
    let mut builder = PlanBuilder::<Services, MacroError>::new("contextless");

    let output = builder.add(contextless_read(41_u32));
    let plan = builder.finish(output);

    assert_eq!(plan.execute(&services).await.unwrap(), 42);
}

#[tokio::test]
async fn operation_macro_supports_eight_non_context_parameters() {
    let services = Services::default();
    let mut builder = PlanBuilder::<Services, MacroError>::new("eight");

    let output = builder.add(sum_eight(1, 2, 3, 4, 5, 6, 7, 8));
    let plan = builder.finish(output);

    assert_eq!(plan.execute(&services).await.unwrap(), 36);
}

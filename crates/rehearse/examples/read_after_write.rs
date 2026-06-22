use rehearse::{BoxFuture, Impact, Input, Operation, OperationMetadata, PlanBuilder};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DemoError;

impl std::fmt::Display for DemoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("demo operation failed")
    }
}

impl std::error::Error for DemoError {}

#[derive(Clone, Default)]
struct Services {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl Services {
    fn record(&self, call: &'static str) {
        self.calls.lock().expect("calls mutex poisoned").push(call);
    }
}

fn metadata(name: &'static str, impact: Impact) -> OperationMetadata {
    OperationMetadata::new(name, impact)
}

fn main_operation<T>(
    name: &'static str,
    impact: Impact,
    output: T,
) -> Operation<Services, T, DemoError>
where
    T: Clone + Send + Sync + 'static,
{
    Operation::new(
        metadata(name, impact),
        (),
        move |services: &Services, ()| -> BoxFuture<'_, Result<T, DemoError>> {
            let services = services.clone();
            let output = output.clone();
            Box::pin(async move {
                services.record(name);
                Ok(output)
            })
        },
    )
}

fn dependent_operation<A, T, F>(
    name: &'static str,
    impact: Impact,
    input: Input<A>,
    f: F,
) -> Operation<Services, T, DemoError>
where
    A: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    F: Fn(A) -> T + Send + Sync + 'static,
{
    let f = Arc::new(f);
    Operation::new(
        metadata(name, impact),
        input,
        move |services: &Services, input: A| -> BoxFuture<'_, Result<T, DemoError>> {
            let services = services.clone();
            let f = Arc::clone(&f);
            Box::pin(async move {
                services.record(name);
                Ok(f(input))
            })
        },
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut builder = PlanBuilder::<Services, DemoError>::new("read_after_write");

    let login = builder.add(main_operation("login", Impact::Session, ()));
    builder.add(main_operation("read_current", Impact::Read, "current"));
    builder.add(main_operation("calculate_changes", Impact::Pure, "changes"));
    let deployment = builder.add(dependent_operation(
        "apply_changes",
        Impact::Write,
        Input::value(login),
        |_| "deployment",
    ));
    let quota = builder.add(main_operation("read_account_quota", Impact::Read, 100_u32));
    builder.add(dependent_operation(
        "verify_deployment",
        Impact::Read,
        Input::value(deployment),
        |_| (),
    ));
    builder.add(main_operation("delete_old_releases", Impact::Delete, ()));

    let plan = builder.finish(quota);
    let services = Services::default();
    let report = plan.dry_run(&services).await;

    println!("{report}");
}

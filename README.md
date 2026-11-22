# rehearse

Build typed operation plans in Rust, then inspect, rehearse, or execute them.

`rehearse` is an effect-aware operation planning library. Users explicitly
declare operation impact, compose operations into an ordered plan, and then pick
an interpretation:

- describe the static plan;
- dry-run safe work while skipping or denying mutations;
- execute the full plan with fail-fast semantics.

The library does not infer whether arbitrary Rust code mutates state. Impact is
metadata supplied by operation authors.

## Why

Deployment, migration, and administrative tools often need three related
workflows: show what would happen, perform safe validation, and execute the real
change. Without a plan abstraction, those modes tend to become scattered
conditionals around direct service calls.

`rehearse` keeps the meaningful actions explicit. A pipeline builds a plan of
declared operations, and runners decide how each operation behaves.

## Install

Add rehearse to your project:

```toml
[dependencies]
rehearse = "0.4.0"
```

For local checkout development, use `rehearse = { path = "crates/rehearse" }`.

Release notes live in [CHANGELOG.md](CHANGELOG.md). The maintainer publish
runbook lives in [RELEASE.md](RELEASE.md).

Enable structured serialization for descriptions, reports, and public status
types with:

```toml
[dependencies]
rehearse = { version = "0.4.0", features = ["serde"] }
```

Rust 1.85 or newer is supported. See [MIGRATION.md](MIGRATION.md) for the 0.3 API changes.

## Five-Minute Quickstart

Define operation bodies, compose them into a plan, then choose how to interpret
the plan:

```rust
use rehearse::{operation, pipeline, Plan};

#[derive(Clone)]
struct Services;

#[derive(Debug)]
struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation failed")
    }
}

impl std::error::Error for Error {}

#[operation(impact = read)]
async fn read_version(#[context] _services: &Services) -> Result<String, Error> {
    Ok("current".to_owned())
}

#[operation(impact = write)]
async fn deploy(version: String) -> Result<String, Error> {
    Ok(format!("deployed {version}"))
}

#[pipeline]
fn release(version: String) -> Plan<Services, String, Error> {
    let _current = rehearse::step!(read_version())?;
    let result = rehearse::step!(deploy(version))?;
    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let services = Services;
    let plan = release("v2".to_owned());

    println!("{}", plan.describe());
    let report = plan.dry_run(&services).await;
    println!("{report}");

    // Use execute only when you intend to run write/delete operations.
    let _output = plan.execute(&services).await?;
    Ok(())
}
```

Calling `release(...)` builds the plan only. `read_version` and `deploy` run
only through `dry_run` or `execute`, and the default dry-run policy skips the
write.

## Local CI/CD Workflow Lab

[`crates/rehearse-workflow-example`](crates/rehearse-workflow-example) is a
standalone CLI with Rust and Spring Boot fixture workflows. Try the release
path `build → test → deploy → verify deploy → publish to API gateway`, or choose
CI, promotion, audit, and maintenance scenarios to explore all declared impacts,
custom policies, typed dependencies, and failure behavior.

```bash
cargo run -p rehearse-workflow-example -- --init
cargo run -p rehearse-workflow-example -- --describe
cargo run -p rehearse-workflow-example -- --describe --format json
cargo run -p rehearse-workflow-example                # dry-run by default
cargo run -p rehearse-workflow-example -- --execute --progress
cargo run -p rehearse-workflow-example -- --stack spring-boot --execute
```

These commands use `target/workflow-lab`. Everything happens in local fixture
files; Cargo and Maven command labels illustrate where real CI commands would
go. Java, Maven, Docker, servers, credentials, and cloud accounts are unnecessary.
See the example's [tutorial](crates/rehearse-workflow-example/README.md) for the
scenario matrix, JSON/Mermaid output, failure injection, and repeatable exercises.

For a browser view with streamed progress and a production approval gate, run:

```bash
cargo run -p rehearse-workflow-example --bin rehearse-workflow-server
```

Open the printed loopback URL, normally `http://127.0.0.1:3000`. The
[server guide](crates/rehearse-workflow-example/SERVER.md) includes approve,
reject, timeout, cancel, and reconnect exercises. Runs and approvals stay in
memory; deployment and gateway effects remain local fixture files.

The checkout's Unreleased server example uses the canonical Describe graph to
draw independent dev and staging deploy/verify lanes before execution. Both
branches must verify the same candidate before the production approval gate
opens. The server opts into concurrent execution and rehearsal, then overlays
streamed node progress on the static graph. The CLI workflows retain their
sequential interpretation. See [PARALLELISM.md](PARALLELISM.md) for the model and
failure contract.

## GitHub Actions reports (Unreleased)

The reusable [`rehearse-github`](crates/rehearse-github) adapter observes existing
progress events and produces live Actions notices, a per-operation timing/status
table, and downloadable graph/event/report files. It is a non-published workspace
crate in this checkout. Execution semantics remain in `rehearse`.

The fixture CLI enables it with `--github-report <fresh-directory>`. Reports must
be outside the lab; safe dry-run still preserves all fixture files. Live notices
go to stderr, preserving JSON on stdout. On Actions, the final Markdown report is
also appended to `GITHUB_STEP_SUMMARY`; this is a completed-step summary, while
operation notices appear live in the logs.

The **Workflow Example E2E** workflow runs only through manual dispatch. Choose
Rust, Spring Boot, or both, and a full suite, dry-run, successful release, or
expected-failure scenario. It drives the actual CLI and uploads reports even when
verification fails. It uses local fixtures and does not exercise the HTTP server
or browser. See the [example guide](crates/rehearse-workflow-example/README.md)
for local commands and the manual-run instructions.

```bash
cargo build -p rehearse-workflow-example --bin rehearse-workflow-example
python3 scripts/workflow_e2e.py --stack all --scenario suite
```

Each invocation retains fresh case workspaces and reports under
`target/workflow-e2e`. An injected verification failure is expected test evidence;
the E2E suite succeeds when the failure prevents publication and is reported
accurately.

## Local Publish Smoke Test

The repository includes a no-server local publish check that simulates registry
artifact resolution without writing to user-level Cargo state:

```bash
scripts/publish-local.sh
```

With Python 3.11 or newer, it creates an owned `target/local-registry` directory,
recreates only its `generated/` child, packages both crates, and writes a
git-backed Cargo registry index and local `.crate` downloads, then builds consumers for default, manual, serde-only, and all-feature configurations.
The all-feature consumer uses:

```toml
rehearse = { version = "0.4.0", registry = "rehearse-local", features = ["serde"] }
```

Expected final output includes the generated `.crate` paths and:

```text
all four consumers checked successfully using registry 'rehearse-local'
```

Use `LOCAL_REGISTRY_DIR=/path/to/registry scripts/publish-local.sh` to choose a
different generated registry location. New locations must be empty or absent;
existing locations must carry this checkout's ownership marker. Symlink paths
and repository/home/root directories are refused. To test consumers on the MSRV,
set `REHEARSE_CONSUMER_TOOLCHAIN=1.85.0` after installing that Rust toolchain.

## Define Operations

The `#[operation]` macro turns an async function into an operation constructor.
The original body becomes delayed executor code; calling the constructor only
records metadata and inputs.

```rust
use rehearse::operation;

#[derive(Clone)]
struct Services;

#[derive(Debug)]
struct DeployError;

#[derive(Clone)]
struct Session;

#[derive(Clone)]
struct Deployment;

#[operation(impact = session)]
async fn login(
    #[context] services: &Services,
    credentials: String,
) -> Result<Session, DeployError> {
    let _ = (services, credentials);
    Ok(Session)
}

#[operation(impact = write)]
async fn apply_changes(
    #[context] services: &Services,
    session: Session,
) -> Result<Deployment, DeployError> {
    let _ = (services, session);
    Ok(Deployment)
}
```

Operation inputs and outputs currently require `Clone + Send + Sync + 'static`.
Operation constructors currently accept up to eight non-context inputs.
All operations in a plan share one context type and one plan error type.

## Compose A Pipeline

The `#[pipeline]` macro lowers straight-line `step!(...)` calls into an ordered
static plan. Calling the function builds a plan; it does not run operation
bodies.

```rust
use rehearse::{pipeline, Plan};

#[pipeline]
fn deploy(credentials: String) -> Plan<Services, Deployment, DeployError> {
    let session = rehearse::step!(login(credentials))?;
    let deployment = rehearse::step!(apply_changes(session))?;
    Ok(deployment)
}

let plan = deploy("secret".to_owned());
```

For a manual-builder example demonstrating independent work during dry-run:

```bash
cargo run -p rehearse --example read_after_write
```

The `configure_vscode` example uses a `#[pipeline]` plan to add missing
rust-analyzer settings to `.vscode/settings.json`:

```bash
cargo run -p rehearse --example configure_vscode -- --dry-run
cargo run -p rehearse --example configure_vscode
```

The `conditional_rollout` example uses seeded random conditions and progress
listeners to rehearse or execute a simulated feature rollout:

```bash
cargo run -p rehearse --example conditional_rollout -- --seed 7
cargo run -p rehearse --example conditional_rollout -- --seed 7 --execute
```

The `deploy` example is this repository's guarded crates.io publish workflow.
By default it describes the publish plan and runs safe dry-run checks; real
`cargo publish` uploads require `--execute`.

```bash
cargo run -p rehearse --example deploy
cargo run -p rehearse --example deploy -- --execute
```

Provide the crates.io token through `.env.local` without committing it:

```bash
export CARGO_REGISTRY_TOKEN=cio_your_crates_io_token_here
```

## Describe

`describe()` renders static plan metadata and the default dry-run action for
each node. It does not contact services or resolve values.

```rust
println!("{}", plan.describe());
```

Example output:

```text
deploy

  1  login                session  run
  2  apply_changes        write    skip
```

Use `describe_with_policy(&policy)` to render actions for a custom
`DryRunPolicy`.

Use `describe_execution()` before execute mode when the dry-run action column
would be misleading. It renders the same static plan order without an action
column:

```text
deploy

  1  login                session
  2  apply_changes        write
```

## Dry-run

Dry-run uses `SafeDryRun` by default:

| Impact | Default dry-run action |
|---|---|
| `Pure` | Run |
| `Session` | Run |
| `Read` | Run |
| `Write` | Skip |
| `Delete` | Skip |
| `Opaque` | Deny |

Skipped, denied, failed, or blocked operations produce no value. Later nodes can
still run when they do not depend on unavailable values.

The crate includes a compiled example:

```bash
cargo run -p rehearse --example read_after_write
```

Its dry-run output demonstrates the central read-after-write case:

```text
[ok] login executed
[ok] read_current executed
[ok] calculate_changes executed
[skip] apply_changes skipped: write operation
[ok] read_account_quota executed
[block] verify_deployment blocked: missing #3 (apply_changes)
[skip] delete_old_releases skipped: delete operation

Dry-run incomplete: 4 executed, 2 skipped, 0 denied, 1 blocked, 0 failed.
```

`read_account_quota` runs even though it appears after the skipped write because
it has no value dependency on that write. `verify_deployment` is blocked because
it needs the unavailable `Deployment` output from `apply_changes`.

Use `report.require_no_failures()?` when skipped writes are acceptable but
executed operation failures are not. Use `report.require_complete()?` when dry-run
should be treated as successful only if every node executed.

## Execute

Execute mode runs every operation in plan order and stops on the first operation
failure.

```rust
let deployment = plan.execute(&services).await?;
```

Execute mode never applies dry-run policy.

## Explicit Concurrency (Unreleased)

The graph and concurrency APIs described here are available in this checkout,
not the published 0.4.0 release. Use a path or Git dependency to try them.

Declare bounded parallel groups through `PlanBuilder::parallel(name, limit,
build)` with a `NonZeroUsize` limit. Within its closure, use
`group.branch(name, build)` and `branch.add(operation)` to build named ordered
branches. The [crate README](crates/rehearse/README.md#static-graphs-and-explicit-concurrency-unreleased)
contains a complete manual-builder example. The `#[pipeline]` grammar stays
straight-line; use the manual builder for groups.

Call `execute_concurrent` or `dry_run_concurrent` to honor those groups. Branches
retain their order, at most the group's limit of operations run at once, and the
next step waits for the group. Branches may consume preceding outer values and
their own earlier outputs; sibling branch dependencies and nested groups are
unsupported. The core polls futures without spawning tasks or choosing a
runtime. Authors choose independent branches and coordinate shared external
resources.

Existing `execute()` and `dry_run()` keep their sequential behavior even for
plans with groups. Concurrent execute stops starting operations on the first
observed failure, drains operations already running, and returns structured
failures plus completed and never-started node IDs. Effects already committed
remain; a hanging operation can keep the drain pending. No following step
starts after failure. Concurrent dry-run preserves policy-before-dependency
checks and continues independent work; report rows remain in declaration order
while progress events may interleave. Listener and custom policy variants are
available. [PARALLELISM.md](PARALLELISM.md) defines these contracts.

## Progress listeners

Use listener variants when a CLI or automation runner wants live progress while
preserving the same semantics:

```rust
use rehearse::{ProgressEvent, ProgressListener};

struct Logger;

impl<E> ProgressListener<E> for Logger {
    fn on_event(&mut self, event: ProgressEvent<'_, E>) {
        if let ProgressEvent::NodeStarted { node, .. } = event {
            println!("starting {}", node.name());
        }
    }
}

let mut logger = Logger;
let report = plan.dry_run_with_listener(&services, &mut logger).await;
let deployment = plan.execute_with_listener(&services, &mut logger).await?;
let description = plan.describe_with_listener(&mut logger);
```

Listeners also work with custom dry-run policies through
`describe_with_policy_and_listener` and `dry_run_with_policy_and_listener`.
They observe node order, impact, selected dry-run actions, node outcomes, and
plan completion. They do not change policy decisions, dependency checks, value
storage, or operation execution.

For simple CLIs, `ConsoleProgress` provides a ready-to-use stdout listener:

```rust
use rehearse::ConsoleProgress;

let mut progress = ConsoleProgress::new();
let report = plan.dry_run_with_listener(&services, &mut progress).await;
```

## Graph Output

Use `to_mermaid()` when a CLI, README, or issue needs a visual dependency graph:

```rust
println!("{}", plan.to_mermaid());
```

The graph is static. It shows plan nodes and explicit `Value<T>` dependencies;
it does not inspect operation bodies.

For canonical CLI JSON and browser previews, the Unreleased
`plan.describe_graph()` API returns a `PlanGraphDescription`. With `serde`
enabled, serialize it directly:

```rust
let graph = plan.describe_graph();
println!("{}", serde_json::to_string_pretty(&graph)?);
println!("{}", graph.to_mermaid());
```

Schema version 1 records `plan_name`, optional `plan_revision`,
`default_interpretation: "sequential"`, `nodes`, and `execution`. Nodes contain
plan-local IDs, declaration positions, names, impacts, value dependencies, and
string annotations. Execution steps preserve sequence and explicit parallel
groups separately from value edges. A value fan-out grants no permission to
reorder work. The graph's Mermaid output draws sequence, forks and joins with
solid arrows, plus additional value edges with dotted arrows. Existing
`plan.to_mermaid()` keeps its value-dependency view, and `describe()` keeps its
table and serialized `rows` format.

Set the application code/configuration revision with
`PlanBuilder::with_revision` and display hints such as labels or environments
with `Operation::with_annotation` or `OperationMetadata::with_annotation`.
Descriptions never invoke bodies, access context, resolve or clone inputs, or
export literal values and runtime outputs. Live progress is a separate overlay
keyed by run ID, revision, and node ID. Applications must update the revision
when relevant code or configuration changes; a graph-metadata hash cannot
identify opaque operation bodies. This is static metadata serialization, not
executable-plan serialization.

## Dry-run Contract

Dry-run may authenticate, observe external state, perform local computation, and
invoke explicitly non-persisting validation. It must not intentionally commit
writes or deletes to managed domain state.

This is a declaration-based contract, not a proof of non-mutation. It depends on
correct impact classification and on user-supplied operation bodies honoring
their declared role.

## Manual Builder

The macros are a frontend over the manual runtime API. Tests and lower-level
integrations may still build plans directly.

```rust
use rehearse::{Input, PlanBuilder};

let mut builder = PlanBuilder::<Services, DeployError>::new("deploy");
let session = builder.add(login("secret".to_owned()));
let deployment = builder.add(apply_changes(Input::value(session)));
let plan = builder.try_finish(deployment)?;
```

Handles belong to the builder that created them. `try_finish` rejects foreign
inputs/outputs and invalid producer order or types without running operation
bodies. `finish` performs the same validation and panics on invalid construction.
`OperationInputs` is sealed; use the supported input shapes.

Synchronous work can use `Operation::sync` without manually boxing a future:

```rust
use rehearse::{Impact, Operation, OperationMetadata};

let read = Operation::sync(
    OperationMetadata::new("read_config", Impact::Read),
    (),
    |_services: &Services, ()| Ok::<_, DeployError>("config".to_owned()),
);
```

## Limitations

- `#[operation]` currently supports async free functions with owned
  non-context parameters, zero or one `#[context] &C` parameter, concrete
  `Result<Output, Error>` returns, up to eight non-context parameters, and no
  generics.
- `#[pipeline]` currently supports straight-line plan constructors ending in
  `Ok(value)`, with step-produced values usable only as direct arguments to later `step!(...)`
  calls or the final output. Transformations and aliases are rejected, including
  inside step arguments. Opaque macro arguments mentioning a handle (including
  string literals) are conservatively rejected.
- No preview hooks or predicted values.
- No runtime branching, loops over operation outputs, retries, rollback,
  durable execution, or executable-plan serialization. Descriptions and reports
  support optional serde serialization.
- No automatic mutation detection.
- Operation inputs and outputs must be owned cloneable values.
- Dry-run and execute currently use the same context type.
- The operation macro is async-only; use the manual API's `Operation::sync` for
  synchronous work.

## Status

The current crate includes ordered plans, execute, dry-run, reports, static
describe output, Mermaid graph output, `#[operation]`, `#[pipeline]`, `step!`,
compiled examples, API docs, optional serde support, Apache-2.0 packaging
metadata, local publish smoke testing, and a guarded crates.io publish workflow
example.

The Unreleased checkout adds canonical graph descriptions and opt-in explicit
concurrency; see [PARALLELISM.md](PARALLELISM.md).

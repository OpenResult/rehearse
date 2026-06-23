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

Use the released crate from crates.io:

```toml
[dependencies]
rehearse = "0.1.1"
```

For local checkout development, use `rehearse = { path = "crates/rehearse" }`.

Release notes live in [CHANGELOG.md](CHANGELOG.md). The maintainer publish
runbook lives in [RELEASE.md](RELEASE.md).

## Local Publish Smoke Test

The repository includes a no-server local publish check that simulates registry
artifact resolution without writing to user-level Cargo state:

```bash
scripts/publish-local.sh
```

By default it recreates `target/local-registry`, packages both crates, writes a
git-backed Cargo registry index and local `.crate` downloads, then compiles a
throwaway consumer crate with:

```toml
rehearse = { version = "0.1.1", registry = "rehearse-local" }
```

Expected final output includes the generated `.crate` paths and:

```text
consumer checked successfully using registry 'rehearse-local'
```

Use `LOCAL_REGISTRY_DIR=/path/to/registry scripts/publish-local.sh` to choose a
different generated registry location.

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

For a complete macro-based local example covering describe, dry-run, and
execute:

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
[block] verify_deployment blocked: missing #3
[skip] delete_old_releases skipped: delete operation

Dry-run incomplete: 4 executed, 2 skipped, 0 denied, 1 blocked, 0 failed.
```

`read_account_quota` runs even though it appears after the skipped write because
it has no value dependency on that write. `verify_deployment` is blocked because
it needs the unavailable `Deployment` output from `apply_changes`.

## Execute

Execute mode runs every operation in plan order and stops on the first operation
failure.

```rust
let deployment = plan.execute(&services).await?;
```

Execute mode never applies dry-run policy.

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
let plan = builder.finish(deployment);
```

## Limitations

- `#[operation]` currently supports async free functions with owned
  non-context parameters, zero or one `#[context] &C` parameter, concrete
  `Result<Output, Error>` returns, and no generics.
- `#[pipeline]` currently supports straight-line plan constructors ending in
  `Ok(value)`, with step-produced values usable only in later `step!(...)`
  calls or the final output.
- No preview hooks or predicted values.
- No runtime branching, loops over operation outputs, retries, rollback,
  durable execution, or serialization.
- No automatic mutation detection.
- Operation inputs and outputs must be owned cloneable values.
- Dry-run and execute currently use the same context type.

## Status

The current crate includes ordered plans, execute, dry-run, reports, static
describe output, `#[operation]`, `#[pipeline]`, `step!`, compiled examples, API
docs, Apache-2.0 packaging metadata, local publish smoke testing, and a guarded
crates.io publish workflow example.

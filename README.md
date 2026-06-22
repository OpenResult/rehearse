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

This crate is in early local development and is not published yet.

```toml
[dependencies]
rehearse = { path = "crates/rehearse" }
```

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
use rehearse::{pipeline, step, Plan};

#[pipeline]
fn deploy(credentials: String) -> Plan<Services, Deployment, DeployError> {
    let session = step!(login(credentials))?;
    let deployment = step!(apply_changes(session))?;
    Ok(deployment)
}

let plan = deploy("secret".to_owned());
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

Runtime phases 0-5 are implemented: ordered plans, execute, dry-run, reports,
static describe output, documentation, `#[operation]`, `#[pipeline]`, and
`step!`. Polish and packaging work remains.

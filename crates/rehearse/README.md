# rehearse

Build typed operation plans in Rust, then inspect, rehearse, or execute them.

`rehearse` is an effect-aware operation planning library. Users explicitly
declare operation impact, compose operations into an ordered plan, and then pick
an interpretation:

- describe the static plan without invoking operation bodies;
- dry-run safe work while skipping or denying mutations;
- execute the full plan with fail-fast semantics.

The library records declared impact. It does not infer or prove whether
arbitrary Rust code mutates state.

## Install

```toml
[dependencies]
rehearse = "0.4.0"
```

The `macros` feature is enabled by default and re-exports the `#[operation]`,
`#[pipeline]`, and `step!` macros from `rehearse-macros`.

Enable structured serialization for descriptions, reports, and public status
types with:

```toml
[dependencies]
rehearse = { version = "0.4.0", features = ["serde"] }
```

Rust 1.85 or newer is supported. In 0.3, `Value<T>` belongs to one builder;
`PlanBuilder::try_finish` returns a `PlanBuildError` for invalid construction,
and `finish` validates and panics on invalid construction. `OperationInputs` is
sealed. Internal execution/report errors now carry `InvariantError` and
`ValueError` instead of strings. Reports and descriptions support serde;
executable plans are not serialized.

## Quickstart

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
```

Calling `release(...)` builds the plan only. Operation bodies run only through
`dry_run` or `execute`, and the default safe dry-run policy skips writes and
deletes.

## Static graphs and explicit concurrency (Unreleased)

The APIs in this section are available in this checkout; they are not part of
the published 0.4.0 release. Use a path or Git dependency to try them.

`plan.describe_graph()` returns the canonical static `PlanGraphDescription` for
CLI JSON and browser previews. With the `serde` feature, serialize it with
`serde_json::to_string_pretty(&plan.describe_graph())`. Schema version 1 contains
`plan_name`, optional `plan_revision`, `default_interpretation: "sequential"`,
`nodes`, and `execution`. Each node includes its plan-local ID, declaration
position, name, impact, explicit value dependencies, and string annotations.
Execution steps record sequence and explicitly declared parallel groups;
value dependencies alone do not authorize concurrency.

Description never invokes bodies, accesses context, resolves or clones inputs,
or exports literal values and runtime outputs. Supply a code/configuration
revision with `PlanBuilder::with_revision`, and attach display metadata with
`Operation::with_annotation` or `OperationMetadata::with_annotation`. A live
client combines this static graph with separate progress keyed by run ID,
revision, and node ID. A metadata hash cannot identify changes inside operation
bodies. The existing `describe()` table and its serialized `rows` remain
available.

The manual builder can declare named branches and a nonzero concurrency limit:

```rust
use rehearse::{Impact, Input, Operation, OperationMetadata, Plan, PlanBuilder, Value};
use std::num::NonZeroUsize;

fn verify(environment: &'static str, candidate: Value<String>) -> Operation<(), String, ()> {
    Operation::sync(
        OperationMetadata::new("verify", Impact::Read).with_annotation("environment", environment),
        Input::value(candidate),
        move |_: &(), candidate| Ok(format!("{environment}: {candidate}")),
    )
}

fn workflow() -> Plan<(), String, ()> {
    let mut builder = PlanBuilder::new("release").with_revision("example-v1");
    let candidate = builder.add(Operation::sync(
        OperationMetadata::new("build_and_test", Impact::Pure),
        (),
        |_: &(), ()| Ok("tested fixture".to_owned()),
    ));
    let (dev, staging) = builder.parallel(
        "verify environments",
        NonZeroUsize::new(2).unwrap(),
        |group| {
            let dev = group.branch("dev", |branch| branch.add(verify("dev", candidate)));
            let staging =
                group.branch("staging", |branch| branch.add(verify("staging", candidate)));
            (dev, staging)
        },
    );
    let joined = builder.add(Operation::sync(
        OperationMetadata::new("join_verified", Impact::Pure),
        (Input::value(dev), Input::value(staging)),
        |_: &(), (dev, staging)| Ok(format!("{dev}; {staging}")),
    ));
    builder.finish(joined)
}

fn main() {
    let plan = workflow();
    println!("{}", plan.describe_graph().to_mermaid());
}
```

On your async runtime, call `plan.execute_concurrent(&()).await` or
`plan.dry_run_concurrent(&()).await` to honor the declared groups. The core polls
futures without spawning tasks or choosing a runtime. Branches retain their own
order, at most the declared number of operations run at once, and the next step
waits for the group. Branches may use preceding outer values and their own earlier
outputs; sibling branch dependencies and nested groups are unsupported. Authors
must choose branches that can safely share external resources.

Existing `execute()` and `dry_run()` remain sequential, including plans with
groups. Execute still stops at its first failure. Concurrent execute stops
starting work on the first observed failure, drains work already running, and
returns structured failures with completed and never-started node IDs. It does
not undo effects, and a hanging operation can keep the drain pending. Concurrent
dry-run retains policy-before-dependency checks and independent continuation;
report rows stay in declaration order even when progress events interleave.
Listener and custom dry-run policy variants are available. The `#[pipeline]`
grammar stays straight-line; use the manual builder for groups.

`plan.describe_graph().to_mermaid()` renders control sequence, explicit forks and
joins, and additional value edges with dotted arrows. The existing
`plan.to_mermaid()` remains the compatible value-dependency view.

## Documentation

- API docs: <https://docs.rs/rehearse>
- Repository: <https://github.com/OpenResult/rehearse>
- Graph and concurrency design: <https://github.com/OpenResult/rehearse/blob/main/PARALLELISM.md>

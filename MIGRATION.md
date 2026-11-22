# Unreleased graph and concurrency additions

The new, initially non-published `rehearse-github` workspace crate is an optional
consumer of the existing progress API; the runtime does not depend on it. The
workflow CLI opts in with `--github-report DIR`. Choose a fresh report directory
outside the fixture lab. Its live notices use stderr so `--json` stdout remains
machine-readable. Reporting adds graph, event, report, and summary files without
changing plan execution or dry-run policy. The manual-only E2E workflow and its
local Python driver use the same fixture CLI.

Existing `execute`, `dry_run`, table description JSON, and pipeline macros keep
their sequential behavior. Manual builders may declare bounded parallel groups;
select `execute_concurrent` or `dry_run_concurrent` explicitly to honor them.
Concurrent execute returns `ConcurrentExecuteError<E>` with all drained errors,
successful node IDs, and never-started IDs, rather than `ExecuteError<E>`.

`describe_graph()` is an additive versioned metadata DTO. Prefer it for new web
clients; value dependencies alone do not describe control order. Supply
`with_revision` and static operation annotations where the client needs plan
identity and display hints. Existing empty annotation metadata serializes as
before. Executable plans and literal values remain unserialized.

`PlanBuildError` adds empty-group, empty-branch, cross-branch-dependency, and
invalid-execution-structure variants. Existing external matches should already
use a fallback arm because the enum is non-exhaustive.

The non-published workflow CLI now emits the canonical graph for
`--describe --json` (or `--describe --format json`); report JSON is unchanged. The local server's
`GET /api/plan` and run `graph` use that same DTO. Run `nodes` now contains only
status overlays, matched to graph nodes by ID, and snapshots/events include a
plan revision. Update prototype browser/API consumers together.

# Migrating from 0.2 to 0.3

0.3 preserves ordered execution, policy-before-dependency dry-run behavior,
independent work after unavailable values, and static description. It changes
construction validation, macro validation, and public error types.

## Plan construction

`Value<T>` now belongs to the builder that created it. A value from another
builder is rejected even when its node index and output type match a local
producer. `Value<T>` remains `Copy` regardless of `T`.

Use `try_finish` when construction errors need to be returned to a caller:

```rust
use rehearse::{Impact, Operation, OperationMetadata, PlanBuildError, PlanBuilder};

let mut builder = PlanBuilder::<(), ()>::new("example");
let output = builder.add(Operation::sync(
    OperationMetadata::new("answer", Impact::Pure),
    (),
    |_, ()| Ok(42_u32),
));
let plan = builder.try_finish(output)?;
# Ok::<(), PlanBuildError>(())
```

Existing `finish(output)` calls continue to work for valid plans. Invalid
construction now panics at the caller rather than returning a plan that might
silently consume another producer's output. Pipeline signatures remain
`Plan<C, T, E>` and use the validated `finish` convenience.

Validation checks all recorded input references and the final output for owner,
producer existence, order, and type. It never resolves values, clones literal
inputs, touches context, or invokes operation bodies. An invalid plan cannot
reach describe or a runner through the public constructors.

## Input extension boundary

`OperationInputs` is now sealed. Replace external implementations with `()`, an
`Input<T>`, or tuples of two to eight inputs. Group related data into one owned
input struct when appropriate. `IntoInput<T>` remains public for constructor
conversions. The hidden `__private` module and `plan::store` access are removed;
store internals are not a public extension API.

## Pipeline grammar

Pass step-produced values directly to the next operation:

```rust,ignore
let current = step!(read_current())?;
let next = step!(calculate(current))?;
Ok(next)
```

Put transformations of runtime outputs inside operations. Aliasing, inspecting,
borrowing, or transforming a handle is rejected both in ordinary statements and
inside `step!` arguments. Wrapping a handle in another function call is also
rejected; generated constructors already accept the handle directly.

Repeated step bindings remain supported, including
`let state = step!(next(state))?`. Ordinary Rust shadowing is tracked, so an
ordinary local that replaces a step binding cannot be used as the final output.
Nested block, closure, loop, and match bindings have their own scopes.

Opaque macro arguments mentioning an active step binding are conservatively
rejected, including names in string literals for implicit format captures.
Ordinary plan-time macros remain ordinary Rust; this frontend does not inspect
arbitrary external macro expansions or prove that user code is free of effects.

## Structured errors and JSON

`PlanBuildError` identifies foreign/missing producers, invalid ordering, and type
mismatches. For variants with `consumer: Option<NodeId>`, `None` means the selected
final output; `Some(node)` means an input of that node.

These existing public fields now carry `InvariantError` instead of `String`:

- `ExecuteError::Internal(error)`
- `NodeOutcome::Internal { error }`

`ProgressOutcome::Internal { error }` borrows `&InvariantError` instead of `&str`.
Use `Display` or `.to_string()` at presentation boundaries. Match structured
variants to handle errors programmatically. New error enums are non-exhaustive;
include a fallback arm when matching them outside the crate.

`InvariantError` distinguishes unavailable dependencies, input resolution, and
final-output resolution. Resolution sources carry `ValueError` with the node
and, for downcast failures, the expected type name. `std::error::Error::source`
preserves internal and operation error chains. Operation errors still require
neither `Clone` nor `Display` to execute or observe them.

With serde enabled, internal-error payloads are now structured objects rather
than strings. Update saved-report consumers that inspect those fields. Node ids
remain zero-based numbers local to a plan; private builder identities are never
serialized. Serde applies to metadata, descriptions, reports, and errors, not
executable plans or stored operation outputs.

## Toolchain and local packaging

The library and macro crates support Rust 1.85 or newer. CI verifies independent
consumers with default features, no default features, serde alone, and all
features on Rust 1.85.0. Development tooling and diagnostic snapshots use stable
Rust; the current trybuild dev-dependency requires Rust 1.88 or newer.

The local registry script now needs Python 3.11 or newer. It reads versions and
dependencies from Cargo metadata and stages manifests by parsing TOML. Archives
and consumers live under `target/local-registry/generated/` by default.

An existing nonempty directory without this checkout's ownership marker is
refused. Choose a fresh `LOCAL_REGISTRY_DIR` when migrating from an old registry
layout. Inspect any old artifacts before removing them yourself. Subsequent runs
clean only the owned `generated/` child, preserving other files in the parent.

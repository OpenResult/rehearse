# Design Notes

## Implementation scope

The implementation centers on an ordered runtime plan plus a small procedural
macro frontend. The runtime owns the semantic contract; macros provide a
restricted ergonomic syntax over the same `PlanBuilder` model.

## Public shape

- The runtime uses `Plan<C, T, E>` generic order: shared context, final output,
  and common plan error.
- Manual operations use one common error type `E`. Per-operation error
  conversion is not part of the current API.
- Operation inputs and outputs require `Clone + Send + Sync + 'static` for the
  current implementation. This keeps the value store simple and avoids unsafe
  code.
- Sync operation functions are not part of the current API. Users can wrap sync
  work in an async block that returns the crate's `BoxFuture`.
- `IntoInput<T>` is public so generated operation constructors can accept
  literals, `Value<T>` handles, or explicit `Input<T>` values.

## Internal representation

- Plans are ordered lists of type-erased nodes. Order controls execute order;
  explicit `Value<T>` inputs control value dependencies.
- Every run creates a fresh type-erased value store keyed by `NodeId`.
- Successful outputs are stored as `Arc<dyn Any + Send + Sync>` and resolved
  through checked downcasts. A downcast failure is reported as an internal
  invariant error.
- Operation metadata currently stores owned `String` names and an `Impact`.

## Dry-run status

`DryRunStatus::Complete` means every node executed successfully. Any skipped,
denied, or blocked node makes the report `Incomplete`; any executed operation
failure or internal invariant error makes it `Failed`. `require_no_failures()`
does not reject ordinary skipped writes or deletes.

## Examples

The `read_after_write` example lives under `crates/rehearse/examples` so
workspace clippy compiles it with the rest of the crate.

The `deploy` example is the repository's guarded crates.io publish workflow. It
constructs the publish sequence with `#[pipeline]`, renders `describe()`, runs
safe dry-run checks by default, and requires `--execute` before invoking real
`cargo publish` uploads.

The `configure_vscode` example uses `#[pipeline]` to add missing rust-analyzer
settings to `.vscode/settings.json`, with an optional `--dry-run` flag that
rehearses the write without changing the file.

## Static describe

- `Plan::describe()` returns an owned `PlanDescription` snapshot using
  `SafeDryRun`.
- `Plan::describe_with_policy(&policy)` renders the same static plan metadata
  with a caller-supplied dry-run policy.
- `Plan::describe_execution()` returns an owned `PlanExecutionDescription`
  snapshot with node order, operation name, and impact only. It omits dry-run
  actions so execute-mode tools do not print misleading `skip` labels for
  write/delete operations.
- Dry-run description rows copy node id, 1-based position, operation name,
  impact, and dry-run action. Execution description rows omit the action.
  Formatting does not touch context, stores, or operation bodies.

## Operation macro

- `rehearse` re-exports `#[operation]` through the default `macros` feature.
- The proc macro crate does not depend on the runtime crate; generated code uses
  `proc_macro_crate` to refer to `rehearse`, including renamed dependencies.
- `#[operation]` supports async free functions with zero or one
  `#[context] &C` parameter, owned non-context parameters, and concrete
  `Result<Output, Error>` returns.
- Contextless operations generate constructors generic over the chosen plan
  context so they can compose into any compatible plan.
- Sync operation functions, generic operation functions, and borrowed
  non-context operation parameters are not part of the current macro surface.

## Pipeline macro

- `rehearse` re-exports `#[pipeline]` and `step!` through the default `macros`
  feature.
- `#[pipeline]` lowers synchronous free functions returning
  `Plan<Context, Output, Error>` into manual `PlanBuilder` code.
- The first supported body language is straight-line:
  `let value = step!(operation(...))?;`, ignored `step!(operation(...))?;`,
  ordinary plan-time statements that do not inspect step-produced values, and a
  final `Ok(value)`.
- Step-produced values are handles, not runtime outputs. They may be passed to
  later operation constructors or returned as the final plan output; inspecting,
  borrowing, branching on, or transforming them is rejected.
- Runtime control-flow nodes and arbitrary Rust control-flow lowering are
  not part of the current macro surface.

## Packaging

- Both crates inherit workspace version, edition, and license metadata.
- The workspace uses the Apache-2.0 license for published packages.
- The runtime crate's optional dependency on `rehearse-macros` includes a
  version as well as the local path so packaging checks model the eventual
  published dependency relationship.
- `cargo package -p rehearse-macros --allow-dirty` verifies locally.
- `cargo package -p rehearse --allow-dirty` cannot prepare the upload package
  until `rehearse-macros` is available from the target registry. Cargo
  strips local paths during package preparation and resolves even optional
  dependencies from the registry.
- Both crates include repository metadata and are publish-enabled; real publish
  is guarded by the `deploy` example's dry-run-first workflow.

## Local publish smoke test

- `scripts/publish-local.sh` provides a no-server local registry smoke test
  under `target/local-registry` by default.
- The script writes a git-backed Cargo registry index and file-backed `.crate`
  downloads, following Cargo's registry/index model:
  <https://doc.rust-lang.org/cargo/reference/registries.html> and
  <https://doc.rust-lang.org/cargo/reference/registry-index.html>.
- `cargo publish --registry ...` is intentionally not used because Cargo publish
  requires a registry API. The local smoke test simulates package resolution
  instead: first publish `rehearse-macros` into the local index, stage `rehearse`
  with a registry-qualified macro dependency, then compile a generated consumer
  crate against `rehearse = { version = "0.1.1", registry = "rehearse-local" }`.
- External dependencies in the local index are explicitly marked as crates.io
  dependencies so Cargo does not try to resolve them from the local rehearse-only
  registry.

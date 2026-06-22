# Design Notes

## Runtime-first scope

The first implementation proved the non-macro runtime before adding procedural
macros. Phase 4 added `#[operation]`; phase 5 added the restricted
`#[pipeline]` and `step!` frontend. Phase 6 keeps the MVP shape intact and
polishes documentation, examples, and packaging metadata.

## Public shape

- The runtime uses `Plan<C, T, E>` generic order: shared context, final output,
  and common plan error.
- Manual operations use one common error type `E`. Per-operation error
  conversion is deferred.
- Operation inputs and outputs require `Clone + Send + Sync + 'static` for the
  first pass. This keeps the value store simple and avoids unsafe code.
- Sync operation adaptation is deferred. Users can wrap sync work in an async
  block that returns the crate's `BoxFuture`.
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

The phase-2 `read_after_write` example lives under `crates/rehearse/examples`
so workspace clippy compiles it during the first review pass.

The phase-6 `deploy` example is macro-first and demonstrates the complete MVP
flow: construct a plan with `#[pipeline]`, render `describe()`, run dry-run, and
then execute the same plan.

## Static describe

- `Plan::describe()` returns an owned `PlanDescription` snapshot using
  `SafeDryRun`.
- `Plan::describe_with_policy(&policy)` renders the same static plan metadata
  with a caller-supplied dry-run policy.
- Description rows copy node id, 1-based position, operation name, impact, and
  dry-run action. Formatting does not touch context, stores, or operation
  bodies.

## Operation macro

- `rehearse` re-exports `#[operation]` through the default `macros` feature.
- The proc macro crate does not depend on the runtime crate; generated code uses
  `proc_macro_crate` to refer to `rehearse`, including renamed dependencies.
- `#[operation]` supports async free functions with zero or one
  `#[context] &C` parameter, owned non-context parameters, and concrete
  `Result<Output, Error>` returns.
- Contextless operations generate constructors generic over the eventual plan
  context so they can compose into any compatible plan.
- Sync operation functions, generic operation functions, and borrowed
  non-context operation parameters are deferred.

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
  deferred.

## Packaging

- Both crates inherit workspace version, edition, and license metadata.
- The runtime crate's optional dependency on `rehearse-macros` includes a
  version as well as the local path so packaging checks model the eventual
  published dependency relationship.
- `cargo package -p rehearse-macros --allow-dirty` verifies locally.
- `cargo package -p rehearse --allow-dirty` cannot prepare the upload package
  until `rehearse-macros = 0.1.0` is available from the target registry. Cargo
  strips local paths during package preparation and resolves even optional
  dependencies from the registry.
- The workspace now includes the dual MIT/Apache-2.0 license files. Both crates
  remain `publish = false` until crate naming and registry checks are complete.

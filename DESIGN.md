# Design Notes

## Runtime-first scope

The first implementation proved the non-macro runtime before adding procedural
macros. Phase 4 adds only the `#[operation]` macro; `#[pipeline]` and `step!`
remain deferred.

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
- Sync functions, generics, borrowed non-context parameters, and pipeline
  lowering are deferred.

# Design Notes

## Runtime-first scope

The first implementation contains only the non-macro runtime crate. The
procedural macro crate is intentionally deferred until the runtime semantics are
covered by tests.

## Public shape

- The runtime uses `Plan<C, T, E>` generic order: shared context, final output,
  and common plan error.
- Manual operations use one common error type `E`. Per-operation error
  conversion is deferred.
- Operation inputs and outputs require `Clone + Send + Sync + 'static` for the
  first pass. This keeps the value store simple and avoids unsafe code.
- Sync operation adaptation is deferred. Users can wrap sync work in an async
  block that returns the crate's `BoxFuture`.

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


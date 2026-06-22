# Rehearse — contributor instructions

This repository contains **`rehearse`**, an effect-aware operation planning
library for Rust.

The crate lets users declare explicit operations, compose them into a static
ordered plan, and then choose one of three interpretations:

1. **Describe**: render static metadata without invoking operation bodies.
2. **Dry-run**: run allowed non-committing operations, skip or deny mutations,
   and continue independent work.
3. **Execute**: run every operation in plan order and stop at the first failure.

`rehearse` is declaration based. It records declared impact; it does not infer
or prove whether arbitrary Rust code mutates state.

## Repository layout

```text
.
├── crates/
│   ├── rehearse/          # runtime facade crate, examples, integration tests
│   └── rehearse-macros/   # #[operation], #[pipeline], and step! macros
├── scripts/
│   └── publish-local.sh   # no-server local registry smoke test
├── DESIGN.md
├── SEMANTICS.md
└── README.md
```

The runtime crate re-exports the proc macros behind the default `macros`
feature. The library API must remain runtime-agnostic; tests and examples may
use Tokio.

## Core invariants

- Building a plan must not invoke operation bodies.
- Execute mode visits nodes in plan order and fails fast on the first operation
  error.
- Dry-run checks policy before dependencies.
- Dry-run does not fail fast on skipped, denied, blocked, or failed nodes.
- Skipped, denied, blocked, and failed nodes produce no value.
- Later nodes are blocked only by explicit unavailable `Value<T>` inputs, not
  merely by appearing after a skipped or failed node.
- Describe is static metadata rendering only. It must not touch context, value
  stores, or operation bodies.
- Do not use unsafe code.

## Public model

Use **impact** in public APIs and docs.

Current impacts:

```rust
pub enum Impact {
    Pure,
    Session,
    Read,
    Write,
    Delete,
    Opaque,
}
```

Default `SafeDryRun` behavior:

| Impact | Action |
|---|---|
| `Pure` | `Run` |
| `Session` | `Run` |
| `Read` | `Run` |
| `Write` | `Skip` |
| `Delete` | `Skip` |
| `Opaque` | `Deny` |

Dry-run may authenticate, observe external state, perform local computation, and
invoke explicitly non-persisting validation. It must not intentionally commit
writes or deletes to managed domain state. This guarantee depends on correct
operation classification and user-supplied operation bodies honoring that
classification.

## Current API shape

- `Plan<C, T, E>` uses generic order: shared context, final output, common error.
- `PlanBuilder<C, E>` is the manual builder.
- `Operation<C, T, E>` stores metadata, inputs, and a delayed executor.
- `Value<T>` is a typed node handle and is `Copy` regardless of `T`.
- Operation inputs support `()`, one `Input<T>`, and tuples up to three inputs.
- Inputs and outputs require `Clone + Send + Sync + 'static`.
- All operations in one plan share one context type and one error type.
- Operation metadata uses owned `String` names and declared `Impact`.
- Successful outputs are stored per run as `Arc<dyn Any + Send + Sync>` and
  resolved through checked downcasts.

## Macro frontend

`#[operation(impact = ...)]` supports async free functions with:

- zero or one `#[context] context: &C` parameter;
- owned non-context parameters;
- concrete `Result<Output, Error>` return type;
- no generics, explicit lifetimes, borrowed non-context parameters, receivers,
  or methods.

`#[pipeline]` supports synchronous free functions returning
`Plan<Context, Output, Error>` with this restricted body shape:

- ordinary plan-construction statements that do not inspect step-produced
  values;
- `let value = step!(operation(...))?;`;
- `step!(operation(...))?;` for ignored outputs;
- final `Ok(value)` where `value` came from a previous `step!`.

The pipeline macro rejects unsupported control flow and unsupported operations
on step-produced values. `step!` outside `#[pipeline]` expands to a compile
error.

## Examples and tools

- `cargo run -p rehearse --example read_after_write`
  demonstrates dry-run skipping a write while an independent later read still
  runs.
- `cargo run -p rehearse --example deploy`
  runs the guarded crates.io publish workflow in safe dry-run mode.
- `cargo run -p rehearse --example configure_vscode -- --dry-run`
  rehearses adding the project rust-analyzer settings to `.vscode/settings.json`.
- `scripts/publish-local.sh`
  creates a file-backed local Cargo registry under `target/local-registry` and
  verifies a generated consumer crate resolves both local crates from it.

## Verification

Before handing off substantive changes, run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Also run the relevant example or script when changing example behavior,
packaging, macros, or dry-run semantics.

## Change guidelines

- Preserve the semantic contract in `SEMANTICS.md`.
- Prefer existing runtime and macro patterns over new abstractions.
- Keep changes scoped. Do not turn the crate into a general workflow engine.
- Keep operation impact explicit; do not add automatic mutation detection.
- Do not add persistent state, distributed execution, retries, rollback,
  serialization, graph parallelism, or runtime branching without a separate
  design update.
- Use structured errors internally; stringify only for display or diagnostics.
- Update `DESIGN.md` when a consequential representation or packaging choice
  changes.

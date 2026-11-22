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
│   ├── rehearse-macros/   # #[operation], #[pipeline], and step! macros
│   ├── rehearse-github/   # reusable, initially non-published reporting adapter
│   └── rehearse-workflow-example/ # non-published local CI/CD workflow lab
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
  error. This remains the default even for plans declaring parallel groups.
- Opt-in concurrent execution runs only explicit bounded groups, preserves branch
  order and group barriers, and stops launching on the first observed failure
  while draining operations already in flight. See `PARALLELISM.md`.
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
- `Value<T>` is a typed, builder-owned node handle and is `Copy` regardless of `T`.
- `try_finish` validates all references and the final output before returning a plan;
  `finish` performs the same validation and panics on invalid construction.
- `OperationInputs` is sealed; store and dependency-resolution plumbing are private.
- Keep `PlanBuildError`, `InvariantError`, and `ValueError` structured until display.
- Operation inputs support `()`, one `Input<T>`, and tuples up to eight inputs.
- Inputs and outputs require `Clone + Send + Sync + 'static`.
- All operations in one plan share one context type and one error type.
- Operation metadata uses owned `String` names and declared `Impact`.
- Metadata may carry static string annotations. `describe_graph` exports versioned
  nodes, value edges, explicit execution structure, and application revision;
  runtime overlays remain separate from the static graph.
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
- `cargo run -p rehearse-workflow-example -- --init`
  creates a local CI/CD lab with Rust and Spring Boot fixtures. Omit the flag to
  dry-run the release workflow, or use `--execute` to change the local lab files.
- `cargo run -p rehearse-workflow-example --bin rehearse-workflow-server`
  serves a loopback browser dashboard with streamed progress and an in-memory
  production approval gate; all deployment effects remain local fixture files.
- `scripts/publish-local.sh`
  uses Python 3.11+ to create a marked file-backed local Cargo registry under
  `target/local-registry/generated` and verifies four consumer configurations.
- `scripts/workflow_e2e.py`
  drives the actual fixture CLI locally and in the manual-only Actions E2E
  workflow; reports remain separate from fixture files.

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
- Keep GitHub command formatting, report files, and summary integration in
  `rehearse-github`; the runtime must not depend on the reporting adapter.
- Do not add persistent state, distributed execution, retries, rollback,
  executable-plan serialization, nested parallel groups, or runtime branching
  without a separate design update. Canonical metadata serialization and explicit
  bounded groups follow `PARALLELISM.md`.
- Use structured errors internally; stringify only for display or diagnostics.
- Update `DESIGN.md` when a consequential representation or packaging choice
  changes.

# Rehearse — project brief and implementation instructions

This file is the source of truth for the initial implementation. The working crate name is **`rehearse`**. Treat the name as provisional until registry and trademark checks are completed.

## Instructions for Codex

Build this project incrementally. Start with the runtime planning engine and its tests. **Do not begin the procedural macros until the non-macro runtime proves the semantics described below.**

When an implementation detail is not fixed here, choose the smallest design that preserves the stated invariants. Record consequential choices in `DESIGN.md`. Avoid speculative abstractions, unsafe code, a hard dependency on a particular async runtime, and features outside the MVP.

Before considering a phase complete, run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

The first implementation pass should complete phases 0–2 and stop for review unless explicitly asked to continue.

---

## 1. Project summary

`rehearse` is an effect-aware operation planning library for Rust.

Users define explicit operations, compose them into a static ordered plan, and then choose one of three interpretations:

1. **Describe** — render the static plan without contacting external systems.
2. **Dry-run** — execute allowed non-committing operations such as login, reads, and local computation; skip or deny writes and deletes; continue running independent operations.
3. **Execute** — run the complete plan with normal fail-fast semantics.

The project is not an analyzer for arbitrary Rust effects. It must never claim that it can infer whether an arbitrary function mutates state. Effect classification is explicit metadata attached to operations.

Suggested tagline:

> Build typed operation plans in Rust, then inspect, rehearse, or execute them.

---

## 2. Product thesis

This is a good project only when kept focused on the following abstraction:

> Every externally meaningful action is an explicitly declared operation. A pipeline builds a plan of those operations. Interpreters decide how each declared operation behaves.

It is not intended to mean:

> Add an attribute to arbitrary Rust and automatically discover which calls are safe.

The plan and interpreters are the product. Procedural macros are an ergonomic frontend added after the engine is correct.

---

## 3. Core semantic contract

These are non-negotiable invariants.

### 3.1 Building a plan does not execute operations

Calling a pipeline constructor may evaluate ordinary plan-construction Rust, but it must not invoke any operation body.

### 3.2 Execute mode preserves source order and fails fast

Execute nodes in plan order. On the first operation failure, stop and return an execution error. This corresponds to the user's sequential `?`-style expectations.

### 3.3 Default dry-run policy

The default safe policy maps operation impact as follows:

| Impact | Default dry-run action |
|---|---|
| `Pure` | Run |
| `Session` | Run |
| `Read` | Run |
| `Write` | Skip |
| `Delete` | Skip |
| `Opaque` | Deny |

`Session` covers authentication, token acquisition, and similar setup that may have incidental side effects but does not intentionally commit managed domain changes.

The dry-run guarantee should be phrased carefully:

> Dry-run may authenticate, observe external state, perform local computation, and invoke explicitly non-persisting validation. It must not intentionally commit writes or deletes to managed domain state.

Do not market the initial version as a proof of non-mutation. The guarantee depends on correct operation classification and the behavior of user-supplied code.

### 3.4 Dry-run continues independent work

Dry-run must not fail fast merely because a mutation was skipped or an earlier read failed.

For each node:

- Run it when policy allows it and all required value inputs are available.
- Skip it when policy says `Skip`.
- Deny it when policy says `Deny`.
- Mark it `Blocked` when one or more required value inputs are unavailable.
- Mark it `Failed` when its body ran and returned an error.
- Continue to later nodes whenever they do not depend on an unavailable value.

### 3.5 Never fabricate outputs

A skipped, denied, failed, or blocked operation produces no value. Do not construct fake `T` values merely to keep later code moving.

### 3.6 Order and value dependency are distinct

The MVP plan is an ordered list. List position gives execution order. Explicit `Value<T>` inputs give value dependencies.

Example:

```text
1. login                  -> Session
2. create_resource        -> Resource       [skipped in dry-run]
3. read_account_quota     -> Quota          [does not use Resource]
4. inspect_resource       -> Inspection     [uses Resource]
```

During dry-run:

- node 2 is skipped;
- node 3 still runs because it has no value dependency on node 2;
- node 4 is blocked because the required `Resource` value is unavailable.

Do not model every later node as transitively blocked merely because it appears after a skipped mutation.

### 3.7 Describe is static and secondary

`describe()` is a nice-to-have renderer over plan metadata. It does not contact services and does not attempt runtime control-flow discovery.

---

## 4. Working terminology

Use **impact**, not “effect system,” in the public API. The library records declared impact; it does not provide compiler-proven effect safety.

Target public enums:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Impact {
    Pure,
    Session,
    Read,
    Write,
    Delete,
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DryRunAction {
    Run,
    Skip,
    Deny,
    // Preview is intentionally deferred.
}
```

Target node outcomes:

```rust
pub enum NodeOutcome<E> {
    Executed,
    Skipped {
        reason: String,
    },
    Denied {
        reason: String,
    },
    Blocked {
        missing_dependencies: Vec<NodeId>,
    },
    Failed {
        error: E,
    },
}
```

The concrete shape may change to improve ergonomics, but these states and their meanings must remain distinguishable.

---

## 5. Workspace layout

Start with two crates in one workspace:

```text
rehearse/
├── Cargo.toml
├── README.md
├── AGENTS.md
├── DESIGN.md
├── SEMANTICS.md
├── LICENSE-APACHE
├── LICENSE-MIT
│
├── crates/
│   ├── rehearse/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── impact.rs
│   │   │   ├── operation.rs
│   │   │   ├── policy.rs
│   │   │   ├── report.rs
│   │   │   ├── error.rs
│   │   │   ├── __private.rs
│   │   │   │
│   │   │   ├── plan/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── builder.rs
│   │   │   │   ├── node.rs
│   │   │   │   ├── value.rs
│   │   │   │   └── store.rs
│   │   │   │
│   │   │   └── runner/
│   │   │       ├── mod.rs
│   │   │       ├── execute.rs
│   │   │       └── dry_run.rs
│   │   │
│   │   └── tests/
│   │       ├── execute.rs
│   │       ├── dry_run.rs
│   │       ├── plan.rs
│   │       └── reports.rs
│   │
│   └── rehearse-macros/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── operation.rs
│           ├── diagnostics.rs
│           ├── crate_path.rs
│           └── pipeline/
│               ├── mod.rs
│               ├── parse.rs
│               ├── validate.rs
│               └── lower.rs
│
└── examples/
    ├── deploy.rs
    └── read_after_write.rs
```

Initially, `rehearse-macros` may be an empty compiling proc-macro crate or omitted until phase 4. Do not introduce a third `rehearse-core` crate unless a concrete need appears.

The normal user dependency should eventually be only:

```toml
[dependencies]
rehearse = "0.1"
```

The `rehearse` facade crate should later re-export proc macros behind a default `macros` feature.

---

## 6. MVP scope

### Included

- In-memory, local plan representation.
- Ordered operation nodes.
- Typed `Value<T>` handles.
- Async operation execution without requiring a particular async runtime.
- A typed context shared by operations in a plan.
- Execute runner.
- Dry-run runner and structured report.
- Default safe dry-run policy.
- Static description renderer.
- Straight-line pipeline syntax added later through proc macros.
- Useful diagnostics and compile-fail tests for unsupported syntax.

### Explicitly deferred

- Server-side preview operations.
- Predicted values.
- Runtime branches based on operation outputs.
- Loops over operation outputs.
- Parallel scheduling.
- Retries, rollback, compensation, and transactions.
- Durable or distributed execution.
- Plan serialization.
- Resuming partially completed plans.
- Borrowed values spanning operation boundaries.
- Automatic effect inference.
- Arbitrary Rust control-flow transformation.

### Initial simplifying constraints

It is acceptable for the first runtime implementation to require operation inputs and outputs to be:

```rust
Clone + Send + Sync + 'static
```

This makes type-erased storage and reuse of outputs straightforward. Document the limitation. Do not hide it behind unsafe code.

A plan should be reusable: each run creates a fresh value store and report.

---

## 7. Target user-facing API

The exact internal traits may differ. Preserve this conceptual usage.

### 7.1 Operation declarations — desired end state

```rust
use rehearse::operation;

#[operation(impact = session)]
async fn login(
    #[context] services: &Services,
    credentials: Credentials,
) -> Result<Session, DeployError> {
    services.api.login(credentials).await
}

#[operation(impact = read)]
async fn read_current(
    #[context] services: &Services,
    session: Session,
    app: AppId,
) -> Result<CurrentState, DeployError> {
    services.api.read_current(&session, app).await
}

#[operation(impact = pure)]
async fn calculate_changes(
    current: CurrentState,
    desired: DesiredState,
) -> Result<ChangeSet, DeployError> {
    Ok(ChangeSet::between(current, desired))
}

#[operation(impact = write)]
async fn apply_changes(
    #[context] services: &Services,
    session: Session,
    changes: ChangeSet,
) -> Result<Deployment, DeployError> {
    services.api.apply(&session, changes).await
}

#[operation(impact = read)]
async fn verify_deployment(
    #[context] services: &Services,
    session: Session,
    deployment: Deployment,
) -> Result<(), DeployError> {
    services.api.verify(&session, deployment).await
}

#[operation(impact = delete)]
async fn delete_old_releases(
    #[context] services: &Services,
    session: Session,
    app: AppId,
) -> Result<(), DeployError> {
    services.api.delete_old_releases(&session, app).await
}
```

The operation macro should eventually generate an operation descriptor constructor. The original body becomes the delayed executor. Calling the generated operation constructor must not invoke the body.

For the MVP runtime, operations may be built manually with a builder API before this macro exists.

### 7.2 Pipeline declaration — desired end state

Prefer an explicit return type showing that calling the function builds a plan:

```rust
use rehearse::{pipeline, step, Plan};

#[pipeline]
fn deploy(
    input: DeployInput,
) -> Plan<Services, Deployment, DeployError> {
    let app = input.app;

    let session =
        step!(login(input.credentials))?;

    let current =
        step!(read_current(session, app))?;

    let changes =
        step!(calculate_changes(current, input.desired))?;

    let deployment =
        step!(apply_changes(session, changes))?;

    step!(verify_deployment(session, deployment))?;
    step!(delete_old_releases(session, app))?;

    Ok(deployment)
}
```

`step!` is an explicit DSL marker consumed by `#[pipeline]`. It distinguishes delayed operations from ordinary Rust expressions.

Calling `deploy(input)` returns a static plan. It does not perform login, reads, writes, or deletes.

### 7.3 Describe

```rust
let plan = deploy(input);
println!("{}", plan.describe());
```

Illustrative output:

```text
deploy

  1  login                 session   run
  2  read_current          read      run
  3  calculate_changes     pure      run
  4  apply_changes         write     skip
  5  verify_deployment     read      run
  6  delete_old_releases   delete    skip
```

### 7.4 Dry-run

```rust
let report = plan.dry_run(&services).await;
println!("{report}");
report.require_no_failures()?;
```

Illustrative output:

```text
✓ login                 executed
✓ read_current          executed
✓ calculate_changes     executed
– apply_changes         skipped: write operation
⊘ verify_deployment     blocked: apply_changes produced no value
– delete_old_releases   skipped: delete operation

Dry-run completed with 2 skipped and 1 blocked operation.
```

A skipped mutation is not an execution error. A read that needs the skipped mutation's output is blocked. An unrelated read later in the plan still runs.

### 7.5 Execute

```rust
let deployment = plan.execute(&services).await?;
println!("deployed {}", deployment.id);
```

Execution runs every operation in source order and fails fast on the first error.

---

## 8. Runtime architecture

Implement the runtime before macros.

### 8.1 Core identifiers and handles

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Value<T> {
    node: NodeId,
    _marker: std::marker::PhantomData<fn() -> T>,
}
```

`Value<T>` should be `Copy` and `Clone` regardless of `T`; it is only a typed node handle.

### 8.2 Plan shape

The conceptual plan type is:

```rust
pub struct Plan<C, T, E> {
    name: String,
    nodes: Vec<Box<dyn ErasedNode<C, E>>>,
    output: Value<T>,
}
```

The implementation can differ, but retain:

- a stable node order;
- operation metadata;
- explicit typed value dependencies;
- a typed final output handle;
- no stored run-specific output values.

### 8.3 Type-erased value store

A runner needs a per-run store keyed by `NodeId`. A straightforward initial representation is:

```rust
Arc<dyn Any + Send + Sync>
```

Store successful outputs as `Arc<T>`. Resolve them with checked downcasts. Return cloned `T` values to operation executors under the MVP `Clone` constraint.

Treat a failed downcast as an internal invariant violation with a clear error; never silently continue.

The store must distinguish value availability. A skipped, denied, blocked, or failed node has no value.

### 8.4 Inputs

Operation descriptors need to support both literal plan-time values and values produced by earlier nodes.

A possible public/internal abstraction is:

```rust
pub enum Input<T> {
    Literal(T),
    Value(Value<T>),
}
```

Operation constructors may accept an `IntoInput<T>` abstraction so users can pass either a concrete `T` or `Value<T>`.

Each erased node must expose the `NodeId`s of all `Value<T>` inputs. Literal inputs are not dependencies.

### 8.5 Runtime-agnostic async

Do not require Tokio, async-std, or another executor in the library API.

A type-erased node may return a boxed future similar to:

```rust
type BoxFuture<'a, T> =
    Pin<Box<dyn Future<Output = T> + Send + 'a>>;
```

Using a small helper dependency is acceptable if it materially simplifies the implementation, but keep the public model runtime-independent. Tests may use Tokio as a dev-dependency.

### 8.6 Context

All context-requiring operations in one plan should use the same context type `C`.

Operations that do not need context should be implementable for any compatible `C`, or should simply ignore it.

The initial version may use the same context type for execute and dry-run. Document that stronger capability-separated contexts and read-only credentials are recommended defense in depth, not yet enforced by the type system.

### 8.7 Errors

Operations may have their own error type if it converts into the plan error `E` when added. A simpler first version may require one common `E` across all nodes.

Execution should return a wrapper carrying node context:

```rust
pub enum ExecuteError<E> {
    Operation {
        node: NodeId,
        name: String,
        source: E,
    },
    Internal(String),
}
```

Dry-run should preserve each actual operation error in its node outcome so independent work can continue.

Do not stringify errors internally unless required for `Display`; structured errors should remain available to callers.

---

## 9. Runner algorithms

### 9.1 Execute runner

Pseudocode:

```text
create empty value store

for node in plan order:
    resolve every value dependency
    if a dependency is unavailable:
        return Internal invariant error

    invoke node body
    on success:
        store output under node id
    on error:
        return ExecuteError::Operation immediately

resolve and return the plan's final output
```

Execute mode never applies dry-run policy.

### 9.2 Dry-run runner

Pseudocode:

```text
create empty value store
create empty report

for node in plan order:
    action = policy.action(node.metadata)

    if action is Skip:
        report Skipped
        leave output unavailable
        continue

    if action is Deny:
        report Denied
        leave output unavailable
        continue

    find unavailable value dependencies
    if any are unavailable:
        report Blocked with those node ids
        leave output unavailable
        continue

    invoke node body
    on success:
        store actual output
        report Executed
    on error:
        report Failed with structured error
        leave output unavailable
        continue

return complete report
```

Policy is checked before dependencies for skipped or denied nodes. This avoids reporting a write as merely blocked when it should be visibly identified as skipped by policy.

### 9.3 Dry-run status

Provide an aggregate status, but retain full node detail. A reasonable shape is:

```rust
pub enum DryRunStatus {
    Complete,
    Incomplete,
    Failed,
}
```

Suggested meaning:

- `Complete`: every node allowed by policy executed successfully and no node was denied or blocked; policy skips may still make the run “incomplete” depending on naming. Choose and document one consistent interpretation.
- `Incomplete`: one or more nodes were skipped, denied, or blocked, but no executed node failed.
- `Failed`: one or more executed nodes failed.

Also provide direct counters and predicates rather than forcing users to infer meaning from one aggregate enum.

Suggested report helpers:

```rust
report.has_failures()
report.has_blocked()
report.has_denied()
report.require_no_failures()
report.iter()
```

Do not make `require_no_failures()` reject ordinary skipped writes unless the method name or policy explicitly says so.

---

## 10. Dry-run policy design

Define a policy trait:

```rust
pub trait DryRunPolicy {
    fn action(&self, metadata: &OperationMetadata) -> DryRunAction;
}
```

Provide a default safe policy, tentatively named `SafeDryRun`:

```text
Pure    -> Run
Session -> Run
Read    -> Run
Write   -> Skip
Delete  -> Skip
Opaque  -> Deny
```

The policy is authoritative. Operation metadata describes impact; the runner decides the action.

Do not initially allow a write operation to mark itself `Run` and bypass the policy. A future preview API can add an explicit non-committing execution hook without invoking the actual mutation body.

---

## 11. Procedural macro design — later phases

There should eventually be two proc macros re-exported from `rehearse`:

```rust
#[operation(...)]
#[pipeline]
```

`step!(...)` is a marker recognized inside `#[pipeline]`.

### 11.1 `#[operation]`

Responsibilities:

- Parse `impact = pure | session | read | write | delete | opaque`.
- Recognize zero or one `#[context]` parameter.
- Validate that the function returns `Result<Output, Error>`.
- Preserve the function body as a delayed executor.
- Generate an operation descriptor constructor that accepts literal inputs or `Value<T>` inputs.
- Generate metadata including operation name and impact.
- Ensure calling the constructor does not execute the body.
- Generate code using the actual dependency name if `rehearse` was renamed in `Cargo.toml`.

MVP operation restrictions may include:

- async functions only, or support both sync and async through a common boxed-future adapter;
- owned, `Clone + Send + Sync + 'static` non-context parameters;
- no generic operation functions initially;
- no borrowed non-context parameters initially;
- one concrete output and error type.

Emit targeted diagnostics for unsupported signatures.

### 11.2 `#[pipeline]`

The first supported body subset should be intentionally small:

```rust
let value = step!(operation(...))?;
step!(operation(...))?;
Ok(value)
```

Also allow ordinary plan-construction statements and plan-time control flow that do not inspect a `Value<T>` produced by a step.

The macro should:

- create a `PlanBuilder`;
- lower each `step!(operation(...))?` to `builder.add(operation(...))`;
- bind the returned `Value<T>` handle;
- lower the final `Ok(value)` to `builder.finish(value)`;
- preserve useful source spans in diagnostics and metadata where practical.

Initially reject with clear errors:

- `return`, `break`, or `continue` crossing pipeline steps;
- branching on a step-produced value;
- matching on a step-produced value;
- loops over a step-produced collection;
- arbitrary method calls or operators on a step-produced value;
- references that live across step boundaries;
- closures containing steps;
- nested async blocks containing steps;
- unsupported uses of `?` outside `step!(...)`.

The macro is a frontend for a restricted Rust-like pipeline language, not a general control-flow analyzer.

### 11.3 `step!` outside pipelines

Provide a small exported `step!` macro that emits a clear compile error when expanded outside `#[pipeline]`. Inside a pipeline, the attribute macro consumes its syntax before normal expansion.

### 11.4 Macro tests

Use compile-pass and compile-fail UI tests for:

- valid straight-line pipelines;
- unsupported branches on step values;
- unsupported borrows;
- missing `?` or malformed `step!` syntax;
- invalid operation signatures;
- dependency renamed in `Cargo.toml`;
- good diagnostic spans.

---

## 12. Required test matrix

The runtime is not complete until all of these behaviors are covered.

### Plan construction

- Building a plan invokes no operation body.
- Node order matches insertion order.
- `Value<T>` points to the correct producer.
- Reusing one `Value<T>` in multiple later operations works.
- Running the same plan twice uses independent stores.

### Execute

- Pure, session, read, write, and delete operations all run.
- Outputs flow to dependent operations correctly.
- Execute preserves source order.
- Execute stops at the first operation failure.
- Nodes after the failure are not invoked.
- Final output is returned when all nodes succeed.

### Dry-run policy

- Pure operations run.
- Session/login operations run.
- Read operations run.
- Write operation bodies are not invoked.
- Delete operation bodies are not invoked.
- Opaque operation bodies are not invoked and are reported denied.

### Dry-run dependencies

- A read depending on a skipped write is blocked.
- A read not depending on a skipped write still runs even when it appears later.
- A node depending on a failed read is blocked.
- An independent node after a failed read still runs.
- A node depending on multiple missing values reports all relevant missing dependencies where practical.
- No fake value appears for a skipped, denied, failed, or blocked node.

### Reporting

- Every node has exactly one outcome.
- Outcomes retain node id, name, impact, and useful error information.
- Counts and predicates are correct.
- Display output is deterministic.
- `require_no_failures()` behaves as documented.

### Safety regression tests

Use counters, atomics, or functions that panic if invoked to prove that write/delete/opaque bodies are never called by the default dry-run policy.

---

## 13. Implementation phases

### Phase 0 — scaffold

Create the workspace and `rehearse` runtime crate.

Deliverables:

- Workspace manifests.
- Module skeleton.
- `Impact`, `DryRunAction`, `NodeId`, and operation metadata.
- Formatting, linting, and test commands pass.
- `DESIGN.md` records initial internal representation choices.

### Phase 1 — plan runtime

Implement the non-macro plan builder and type-erased node/value machinery.

Deliverables:

- `Value<T>`.
- Per-run type-erased value store.
- Ordered plan nodes.
- A manual way to add async operations with metadata and explicit dependencies.
- Typed final output handle.
- Execute runner.
- Tests for construction, data flow, ordering, reuse, and fail-fast execution.

The manual builder API may be lower-level than the eventual macro API. Keep it usable enough for integration tests and examples.

### Phase 2 — dry-run

Implement `SafeDryRun`, dry-run traversal, and reports.

Deliverables:

- Default impact-to-action mapping.
- `Executed`, `Skipped`, `Denied`, `Blocked`, and `Failed` outcomes.
- Continue-on-independent-work behavior.
- Structured report helpers and deterministic `Display`.
- Full dry-run test matrix.
- An example reproducing read-after-write blocking and an independent later read.

Stop here for the first review.

### Phase 3 — describe and README

Deliverables:

- Static description renderer.
- Root README with a manual-builder example and the intended macro syntax labeled as upcoming if macros are not implemented.
- `SEMANTICS.md` documenting execute and dry-run behavior.

### Phase 4 — `#[operation]`

Deliverables:

- Proc-macro crate.
- Operation descriptor generation.
- Context parameter handling.
- Impact metadata.
- Renamed dependency support.
- UI tests.

### Phase 5 — `#[pipeline]` and `step!`

Deliverables:

- Straight-line lowering.
- Explicit `Plan<C, T, E>` return syntax.
- Targeted rejection of unsupported control flow.
- Compile-pass and compile-fail tests.

### Phase 6 — polish

Deliverables:

- Complete README end-to-end example.
- API docs.
- Examples.
- Feature flags and facade re-exports.
- Crate packaging checks.

---

## 14. README outline

The root README should eventually use this structure:

```text
# rehearse
One-sentence value proposition

## Why
Avoid duplicated describe/dry-run/execute workflows and scattered mode checks

## Install
Cargo dependency

## Define operations
One login, one read, one pure computation, one write, one delete

## Compose a pipeline
Straight-line step syntax

## Describe
Static output

## Dry-run
Actual reads plus skipped mutation and blocked dependent read

## Execute
Normal final value

## Dry-run contract
Precise safety wording and default policy table

## Limitations
Explicit restricted pipeline language and declaration-based safety

## Status
Maturity and deferred features
```

The README must include the read-after-write case. That behavior is central to the library's value and prevents an overly simplistic “just skip writes” interpretation.

---

## 15. Non-goals and guardrails

Do not turn the initial crate into a general workflow engine.

Specifically, do not add the following without a separate design decision:

- persistent state;
- task queues;
- distributed workers;
- automatic retries;
- rollback or compensation;
- graph parallelism;
- generic expression evaluation;
- runtime branching DSLs;
- automatic mutation detection;
- shell-command safety inference;
- plan serialization;
- plugin systems.

Do not use unsafe code to remove the MVP `Clone + 'static` constraints.

Do not let operation-level metadata override the central safe policy in a way that causes an actual write/delete body to run during default dry-run.

Do not treat skipped operations as ordinary errors, and do not terminate dry-run traversal at the first skipped or failed node.

---

## 16. Future extension points

These are plausible later additions, not MVP requirements.

### Preview hook

A write operation may eventually expose a separate non-committing preview implementation:

```rust
DryRunAction::Preview
```

The preview hook must be separate from the actual mutation body. It may validate, calculate a diff, or call a server's genuine dry-run endpoint.

Predicted outputs require a separate design. Do not introduce them accidentally by returning ordinary `T` values with unclear provenance.

### Capability-separated contexts

A later API may use different contexts or capabilities for dry-run and execute so preview/read code cannot access mutation clients.

### Runtime control-flow nodes

A future graph DSL may add explicit constructs such as:

```text
when!
match_value!
for_each!
parallel!
```

These should become explicit plan nodes, not implicit analysis of arbitrary Rust control flow.

---

## 17. Open decisions to record, not block on

Choose reasonable defaults for the runtime prototype and document them:

- exact generic order of `Plan<C, T, E>`;
- whether the manual builder requires one common error type or `Into<E>`;
- exact boxed-future helper;
- internal representation of node metadata strings;
- whether `DryRunStatus::Complete` permits policy-skipped mutations;
- naming of `SafeDryRun`, `NodeOutcome`, and report helper methods;
- whether sync operations are adapted immediately or deferred.

Do not block implementation on crate-name availability. Use `rehearse` as the working package name locally.

---

## 18. Definition of success for the first review

The first review should demonstrate this scenario using the manual builder API:

```text
login                  Session   -> runs and succeeds
read_current           Read      -> runs and succeeds
calculate_changes      Pure      -> runs and succeeds
apply_changes           Write     -> body is never called; reported skipped
read_account_quota     Read      -> runs despite appearing after skipped write
verify_deployment      Read      -> blocked because it needs apply_changes output
delete_old_releases    Delete    -> body is never called; reported skipped
```

The same plan in execute mode must invoke every operation in order and return its final typed output.

The codebase must be formatted, warning-free under the stated Clippy command, fully tested, and documented well enough that the macro frontend can later be built without changing the core semantics.

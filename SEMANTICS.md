# rehearse Semantics

This document describes the current runtime semantics and the implemented
`#[operation]` frontend.

## Declared Impact

`rehearse` records declared impact; it does not infer effects from arbitrary
Rust code.

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

Operation authors are responsible for choosing accurate metadata.

## Operation Macro

`#[operation(impact = ...)]` is implemented for async free functions with:

- zero or one `#[context] context: &C` parameter;
- owned non-context parameters;
- concrete `Result<Output, Error>` return types;
- no generics, explicit lifetimes, borrowed non-context parameters, or methods.

The macro replaces the function with an operation constructor of the same name.
Calling that constructor does not invoke the original body. The body is wrapped
as delayed executor code that runs only when execute or dry-run policy allows the
node to run.

Constructor arguments accept literal values, `Value<T>` handles, or explicit
`Input<T>` values through `IntoInput<T>`.

## Plan Construction

Building a plan does not execute operation bodies. `PlanBuilder::add` records an
operation's metadata, ordered position, and explicit value dependencies. Literal
inputs are stored in the operation descriptor. `Value<T>` inputs are typed
handles to outputs from earlier nodes.

Plan construction may evaluate ordinary Rust used to build the plan, but it must
not invoke the delayed operation executor.

## Order And Value Dependencies

The plan is an ordered list. List order controls execute traversal and report
order.

Value dependencies are separate. A node is value-dependent only on the
`Value<T>` handles it consumes. A later node is not blocked simply because an
earlier unrelated node was skipped, denied, blocked, or failed.

## Execute Mode

Execute mode creates a fresh value store and visits nodes in plan order.

For each node:

- every value dependency must already be available;
- the operation body is invoked;
- successful output is stored under that node id;
- the first operation error stops execution and returns `ExecuteError`;
- nodes after the first failure are not invoked.

Execute mode never applies dry-run policy.

## Dry-run Mode

Dry-run creates a fresh value store and visits nodes in plan order. The default
policy is `SafeDryRun`.

| Impact | Default action |
|---|---|
| `Pure` | `Run` |
| `Session` | `Run` |
| `Read` | `Run` |
| `Write` | `Skip` |
| `Delete` | `Skip` |
| `Opaque` | `Deny` |

For each node:

- policy is checked before dependencies;
- `Skip` records `Skipped` and produces no value;
- `Deny` records `Denied` and produces no value;
- `Run` checks required value dependencies;
- unavailable dependencies record `Blocked`;
- a successful operation records `Executed` and stores the real output;
- an operation error records `Failed` and produces no value;
- traversal continues to later independent nodes.

Policy-before-dependency means a write is reported as skipped by policy even if
one of its inputs is unavailable.

## No Fabricated Outputs

Skipped, denied, blocked, and failed nodes do not produce values. The runtime
never constructs placeholder `T` values to keep later operations moving.

If a later node depends on an unavailable output, that later node is blocked.
If it does not depend on the unavailable output, it may still run.

## Reports And Status

Every dry-run node has exactly one outcome:

- `Executed`
- `Skipped`
- `Denied`
- `Blocked`
- `Failed`

`DryRunStatus` is derived from node outcomes:

- `Complete`: every node executed successfully;
- `Incomplete`: one or more nodes were skipped, denied, or blocked, and no
  executed node failed;
- `Failed`: one or more executed nodes failed or an internal invariant error was
  reported.

`require_no_failures()` rejects failed operations only. It does not reject
ordinary skipped writes or deletes.

## Static Describe

`Plan::describe()` renders static plan metadata and default dry-run actions.
`Plan::describe_with_policy(&policy)` renders actions for a custom policy.

Describe does not use a context, create a value store, resolve dependencies, or
invoke operation bodies.

## Current Runtime Constraints

- Operation inputs and outputs must be `Clone + Send + Sync + 'static`.
- Operations in one plan share one context type `C`.
- Operations in one plan share one error type `E`.
- Async execution is runtime-independent in the library API through boxed
  futures. Tests and examples may use Tokio.
- Sync operation adaptation is deferred; callers can wrap sync work in an async
  block.
- `#[pipeline]` and `step!` are deferred.

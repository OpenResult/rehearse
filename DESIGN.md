# Design Notes

## Implementation scope

The implementation centers on an ordered runtime plan plus a small procedural
macro frontend. The runtime owns the semantic contract; macros provide a
restricted ergonomic syntax over the same `PlanBuilder` model.

## Describe graph and concurrency design

[PARALLELISM.md](PARALLELISM.md) specifies the additive canonical Describe graph
and explicit bounded parallel groups. Existing execute and dry-run methods stay
sequential. New opt-in interpreters honor declared groups, preserve typed value
ownership, and keep runtime scheduling independent of Tokio. The graph separates
execution structure from value edges and provides metadata for pre-execution web
rendering. Application annotations and revision identity bind that preview to its
live run overlay.

## Public shape

- The runtime uses `Plan<C, T, E>` generic order: shared context, final output,
  and common plan error.
- Manual operations use one common error type `E`. Per-operation error
  conversion is not part of the current API.
- Operation inputs and outputs require `Clone + Send + Sync + 'static` for the
  current implementation. This keeps the value store simple and avoids unsafe
  code.
- Operation inputs are supported as `()`, one input, or tuples up to eight
  inputs.
- The macro frontend remains async-only. Manual operations can use
  `Operation::sync` when the implementation does not need to await internally.
- `IntoInput<T>` is public so generated operation constructors can accept
  literals, `Value<T>` handles, or explicit `Input<T>` values.

## Internal representation

- Builders receive a private, non-reused process-local identity from a checked
  atomic counter. Handles retain that identity and a node index; public `NodeId`
  stays a plan-local index for descriptions, graphs, and serialized reports.
- Internal dependency references retain ownership, `TypeId`, and a type name.
  `try_finish` validates ownership, existence, producer order, and type for all
  dependencies and the selected output before producing a `Plan`. `finish` is
  the caller-tracked panic convenience. Neither path resolves or clones inputs.
- `OperationInputs` is sealed behind private resolution plumbing; supported
  shapes remain unchanged. `IntoInput` remains public.
- Plans retain an ordered list of type-erased nodes plus execution steps for
  ordinary nodes and explicit bounded parallel groups. Default interpreters use
  flat order; opt-in concurrent interpreters honor groups and barriers. Explicit
  `Value<T>` inputs control value dependencies separately from control structure.
- Every run creates a fresh type-erased value store keyed by `NodeId`.
- Successful outputs are stored as `Arc<dyn Any + Send + Sync>` and resolved
  through checked downcasts. A downcast failure is reported as an internal
  invariant error. Stores also check handle ownership.
- Operation metadata stores owned names, declared `Impact`, and static string
  annotations. Plans may carry an application-supplied code/configuration revision.
- Input resolution prepares owned input data before creating a running future.
  The concurrent coordinator owns the store and commits outputs while other
  futures remain in flight; it polls `FuturesUnordered` without spawning tasks.

## Errors

`PlanBuildError` represents invalid construction. Runtime invariant failures use
`InvariantError`, with `ValueError` sources for resolution failures. Errors retain
node and dependency data until `Display`; execute, dry-run reports, and borrowed
progress outcomes use the same structured types. Original operation errors stay
in `E` without new cloning or formatting bounds. These changes and sealing the
input trait are part of the 0.3 API boundary; see `MIGRATION.md`.

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

Examples that need live node progress use the shared `ConsoleProgress` listener
instead of carrying local listener implementations.

Examples that expose command-line flags use `clap` through dev-dependencies.
This keeps the runtime crate dependency surface small while making example help
and argument validation consistent.

The standalone `crates/rehearse-workflow-example` application is a non-published
workspace member. It owns its CLI, Tokio, and serialization dependencies and
models local CI/CD with deterministic Rust and Spring Boot fixtures. Its CI,
release, promotion, audit, and maintenance workflows are selected before plan
construction; each resulting plan is static and ordered. Most plans use the
macro frontend; the audit plan demonstrates `Operation::sync`, `PlanBuilder`,
and `try_finish`. The example exposes progress, Mermaid, structured descriptions
and reports, a custom no-session dry-run policy, and controlled failures.

Build and test operations are declared `Write` because their fixture artifacts
and reports are managed lab state. Deployments and gateway publications also
write files, cleanup declares `Delete`, and a deliberately opaque legacy hook
demonstrates default denial. A later independent gateway observation remains
available after skipped or failed work. Verification consumes actual receipts;
dry-run never fabricates an artifact or deployment output. The local domain
files represent mock external systems, not persisted workflow-engine state.
The CLI fixture operations do not invoke Cargo, Maven, Java, a server, or cloud
infrastructure. The optional server binary serves only its own local dashboard.
CI selects the library crates explicitly for feature-isolation checks so the
application's macro and serde dependencies do not enable those features in the
manual and serde-only library configurations.

## Local workflow server application

The non-published example also exposes `rehearse-workflow-server`, a loopback-only
Axum application with a static browser client. The server constructs one release
plan: build/test, an explicit two-branch dev/staging deploy-and-verify group, a
joining production approval gate, production deploy/verify, gateway publication,
and observation. It selects the opt-in concurrent interpreter and reuses the
fixture backend in isolated environment directories;
these domain files are not execution checkpoints.

The production gate is an ordinary async `Session` operation awaiting a
server-owned decision channel. It returns a typed permit bound to run, gate,
artifact SHA-256 digest, revision, and production target. Production consumes
that permit and rechecks current test and environment evidence before writes.
The server's rehearsal policy skips the interactive gate and delegates other
impacts to `SafeDryRun`; dry-run opens no question and fabricates no permit.
Static plan descriptions remain context-free. `GET /api/plan` and each run's
graph use the canonical `PlanGraphDescription`; status overlays reference its
node IDs and events carry run ID, plan revision, and optional node ID. The static
browser client draws sequence and parallel lanes from that graph before a run.
Annotations provide display labels, environments, and gate kinds independently
of operation names.

An in-memory registry owns execution tasks independently of browser requests.
The synchronous progress listener projects borrowed library events into owned
JSON snapshots with run IDs, sequence numbers, and timestamps. A bounded replay
buffer feeds SSE; reconnect uses event IDs and falls back to a full snapshot
when history has expired. Neither the listener nor network delivery controls
execution. Separate decision and cancellation endpoints update application
state and wake the waiting task.

Approval decisions are serialized, checked against the pending artifact and
revision, and idempotent for the same response. Rejection, expiry, and
cancellation have explicit application statuses. They do not alter the core
outcome enums: the gate still returns a real value or an operation error, and
concurrent execute stops launches and drains active work after a failure.
Cancellation instead drops the active async plan future; effects completed before
it remain, with no rollback.

Run records, decisions, and events are lost on server restart. Fixture workspaces
remain for inspection and future runs receive unique IDs. This prototype adds
no persistence or resume facility, distributed execution, retries, executable-plan
serialization, or runtime branching to rehearse. It has one local
operator, not a production authentication or authorization model. The runtime
crate remains independent of Axum, Tokio channels, HTTP, and browser code.

## GitHub reporting adapter

The reusable `rehearse-github` workspace crate observes `ProgressListener` events
without changing policy, scheduling, value storage, or execution outcomes. It is
initially non-published and depends on the runtime with macros disabled and serde
enabled. GitHub-specific formatting and file output stay outside the runtime.

An explicit caller-owned reporter combines the canonical Describe graph with
declaration-ordered runtime outcomes and per-node timings. It writes versioned
event and report JSON, the unchanged canonical graph JSON, and a Markdown summary
to a fresh report directory. The caller finalizes it on success or failure and
may append the summary to `GITHUB_STEP_SUMMARY`. Report artifacts contain metadata
and diagnostics, not operation outputs, executable plans, or resume checkpoints.
Reporting I/O failures remain separate from the original workflow error.

Live messages escape GitHub command data, and summary cells escape Markdown/HTML.
Concurrent events use node IDs rather than overlapping log groups. Never-started
nodes remain distinct from skipped, blocked, and failed operations. No GitHub API
credentials, network transport, or async runtime are needed by the adapter.

The CLI opts into reporting explicitly, with reports outside its managed lab so
dry-run fixture preservation still holds. A dedicated manual-only Actions
workflow builds once and drives the actual CLI for Rust and Spring Boot fixtures.
Its Python standard-library driver verifies static graph output, unchanged dry-run
fixtures, successful release, and an expected verification failure preventing
gateway publication. The same driver runs locally. Reports are retained even when
an assertion fails. Server HTTP and browser E2E are outside this first workflow.

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
- Display implementations size their columns from the current rows while
  preserving a readable minimum width.
- `Plan::to_mermaid()` renders static node metadata and explicit value
  dependency edges for docs and CLI output.
- `Plan::describe_graph()` returns versioned canonical metadata with explicit
  execution structure; its `to_mermaid()` renders sequence, fork, and join control
  edges separately from extra value edges. Existing table JSON remains unchanged.

## Progress listeners

- `ProgressListener<E>` receives borrowed progress events for describe,
  dry-run, and execute traversals.
- Existing describe, dry-run, and execute methods use `NoopProgress` internally.
  Listener variants expose the same behavior with observation hooks.
- Progress outcomes borrow operation errors so listener support does not add
  `Clone` or formatting requirements to the common error type `E`.
- Progress events do not participate in dry-run policy decisions, dependency
  checks, value storage, operation execution, or error conversion.
- Concurrent interpreters invoke listeners from one coordinator, with interleaved
  node events and one plan finish after active work drains on execute failure.

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
- The macro accepts up to eight non-context parameters, matching the runtime
  tuple input implementations.

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
  borrowing, branching on, or transforming them is rejected, including inside
  operation arguments. Only direct handle arguments are accepted.
- Validation builds a small statement representation before emitting code and
  tracks lexical shadowing. Macro arguments containing active handle names are
  conservatively rejected, including literal words that could be format captures.
  Arbitrary external macro expansion is outside this syntactic validation.
- Generated local builder and context identifiers use mixed-site spans.
- Runtime control-flow nodes and arbitrary Rust control-flow lowering are
  not part of the current macro surface.

## Packaging

- The library crates inherit workspace version, edition, license, and Rust 1.85 minimum
  compiler metadata. Stable Rust runs the dev tooling and diagnostic snapshots.
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

- `scripts/publish-local.sh` invokes the standard-library-only Python 3.11+
  helper `scripts/publish_local.py`.
- A checkout-specific marker identifies an owned registry parent. Only its
  `generated/` child is recreated. Unmarked nonempty directories, symlink paths,
  repository/home/root paths, and ancestors are rejected before cleanup.
- Workspace versions, dependency requirements, renames, target predicates,
  feature flags, and MSRV come from `cargo metadata`. Index records follow
  <https://doc.rust-lang.org/cargo/reference/registry-index.html>.
- The script stages only workspace manifests, the two published crate directories,
  the README, and license, restricting the staged workspace to those two members.
  It parses the runtime manifest to replace the local macro path
  with a registry dependency, without changing the source manifest.
- Both archives are built and verified by Cargo, then indexed in dependency
  order in a file-backed git registry. Real `cargo publish` is not involved.
- Four independent consumer binaries exercise manual and macro construction,
  feature isolation, and serde round trips. Their dependency trees must resolve
  rehearse packages from this local registry. `REHEARSE_CONSUMER_TOOLCHAIN`
  selects the consumer toolchain; CI uses `1.85.0`.

## Optional serialization

- The runtime crate exposes an optional `serde` feature.
- With that feature enabled, public static descriptions, dry-run reports,
  status/action enums, node ids, operation metadata, and execute/dry-run errors
  derive serde serialization support for CLIs and automation.

Serialization does not include executable plans, value stores, or private builder identities.

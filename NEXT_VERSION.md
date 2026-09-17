# Review and next-version plan

Review baseline: workspace 0.2.0, commit `68bbaf2`, reviewed on 2026-09-17.
The working tree already contained a modified `Cargo.lock`; the original review
used that lockfile. The review below is historical; the approved work has now
been implemented for the unpublished 0.3.0 workspace.

## Implementation status

- Completed plan ownership, static construction validation, `try_finish`, and
  the sealed input boundary.
- Completed pipeline argument validation, scope tracking, and generated-name
  hygiene, with compile-pass/fail coverage.
- Completed structured construction and runtime errors, source chains, borrowed
  listener propagation, and serialization tests.
- Added feature-matrix tests/docs, a manual API doctest, Linux/macOS CI, and
  Rust 1.85 metadata and consumer checks.
- Replaced local registry cleanup with guarded, checkout-owned generated
  directories; versions/dependencies now come from Cargo metadata. Added Python
  tests and four packaged consumer configurations.
- Prepared 0.3.0 manifests, changelog, and `MIGRATION.md`; publishing/tagging remain
  separate release actions.

Post-implementation verification passed locally on macOS:

- Formatting, clippy with warnings denied, tests, and strict rustdoc in default,
  manual-only, serde-only, and all-feature configurations.
- 63 all-feature test functions including the doctest; the two macro harnesses
  additionally exercise 39 compile-pass/fail fixtures.
- Six Python release-tooling tests and two successful local registry runs,
  exercising guarded reuse and all four consumer configurations on Rust 1.85.0.
- Read-after-write, configure-vscode dry-run, seeded rollout dry-run, and deploy
  help checks. Linux/macOS CI is configured; remote CI was not run here.

`MIGRATION.md`, `DESIGN.md`, and `SEMANTICS.md` describe the implemented API.
The verification section at the end records the original review, including its
then-failing configurations, rather than the post-implementation results.

## Recommendation

Make 0.3.0 a focused release that validates plan construction and closes gaps in
the macro contract. Keep the ordered plan, explicit impact declarations, typed
values, separate execute/dry-run traversals, and static describe behavior.

The current architecture fits the problem. The strongest reason for a redesign
is the boundary between builders, value handles, and dependency metadata. A
handle can currently refer to the wrong plan without an error. Fix that boundary
before adding more ways to compose plans.

Use 0.3.0 for changes to public errors and the input extension surface. Cargo
treats a change from 0.2 to 0.3 as incompatible; adding enum variants or changing
trait contracts can break downstream users. See the
[Cargo compatibility guidance](https://doc.rust-lang.org/cargo/reference/semver.html).

## Findings, in priority order

### 1. P1: Foreign value handles can silently select unrelated data

Locations: `crates/rehearse/src/plan/value.rs:38`,
`crates/rehearse/src/plan/builder.rs:37`, and
`crates/rehearse/src/plan/store.rs:68`.

`Value<T>` contains a node index and type marker. Each builder allocates indices
from zero, and the store resolves values using only that index. Neither `add`
nor `finish` checks which builder produced a handle.

Confirmed with isolated runtime probes:

- Builder A produces `100_u32` at node 0. Builder B produces `7_u32` at node 0.
  An operation in B consuming A's handle receives **7**, without an error.
- Finishing B with A's handle also returns **7**.
- Finishing an empty builder with A's handle produces a dry-run report accepted
  by `require_complete()`, while execution fails to resolve the output.

The first two cases are silent correctness failures. For operations that commit
changes, the wrong input can reach the operation body. Type checks cannot catch
the collision when the two producers have the same output type.

### 2. P2: Step arguments bypass the pipeline's value-use restrictions

Locations: `crates/rehearse-macros/src/pipeline/validate.rs:126` and
`crates/rehearse-macros/src/pipeline/lower.rs:33`.

Ordinary statements receive value-use validation, but `step!` expressions only
check for nested steps, `?`, and an outer function-call shape. The validator does
not check transformations of step handles inside arguments.

This compiles today:

```rust,ignore
let value = step!(write_number())?;
let output = step!(read_number(value.node().index() as u32))?;
Ok(output)
```

The probe confirmed that the second operation has no dependency edges, receives
literal zero, and executes during dry-run while the write is skipped. The
runtime follows the recorded inputs correctly; the macro accepts syntax that
its documented contract says it rejects.

The name-only `HashSet` and the ordinary-statement visitor also deserve tests
for aliases, function calls, opaque macro arguments, and lexical shadowing.
Those additional cases were inspected in source, not exhaustively reproduced.

### 3. P2: The optional-macros configuration lacks working verification

Locations: `crates/rehearse/tests/pipeline_macro_runtime.rs:1`,
`crates/rehearse/src/lib.rs:8`, and `.github/workflows/ci.yml:26`.

The runtime library builds with `--no-default-features`. However:

- Integration tests fail because macro tests import disabled macros
  unconditionally.
- Strict rustdoc fails because the crate-level docs link to macro names that
  are unavailable with the feature disabled.
- CI only tests all features on stable Linux, so neither failure is detected.

This is a test/documentation configuration defect, not evidence that the manual
runtime API fails to build.

### 4. P2: Internal errors lose their structure before reaching callers

Locations: `crates/rehearse/src/operation.rs:77`,
`crates/rehearse/src/error.rs`, and `crates/rehearse/src/runner/execute.rs`.

`ResolveInputError` is structured, but operation resolution immediately converts
it to `String`. Missing dependencies and final-output failures are also formatted
inside the runner. Callers cannot reliably distinguish these conditions without
parsing messages, and error sources are lost for internal failures. This also
conflicts with the contributor instruction to stringify only for display or
diagnostics.

### 5. P2: Local registry cleanup trusts an arbitrary override directory

Locations: `scripts/publish-local.sh:5` and `scripts/publish-local.sh:168`.

`LOCAL_REGISTRY_DIR` controls an unconditional recursive deletion. A mistaken
override can delete an existing directory unrelated to the generated registry.
The default path is appropriate, but the public override needs ownership checks.
This was established by source inspection; no destructive probe was run.

The script also hardcodes the workspace version and registry dependency entries,
and changes a manifest using an exact text substitution. Those are maintenance
risks when preparing the next version, rather than current resolution failures.

## Proposed design

### Validate ownership before a plan can run

- Give each builder a private, non-reused identity. Carry it in `Value<T>` while
  preserving `Copy` without a `T: Copy` bound.
- Preserve public `NodeId` as a plan-local index for reports and diagrams. Do
  not expose process-local builder identities in serialized reports.
- Retain ownership and expected output type in internal dependency references.
  Adding identity only to `Value<T>` is insufficient: today's
  `OperationInputs::dependencies()` discards everything except `NodeId`.
- Validate every dependency and the selected final output: same builder,
  existing producer, correct type, and producer preceding consumer. Validation
  must inspect recorded metadata only, without resolving inputs or invoking
  executors.
- Add `PlanBuilder::try_finish(...) -> Result<Plan<...>, PlanBuildError>`.
  Retain `finish(...)` as a documented, caller-tracked panic convenience for
  invalid construction. Existing valid pipeline signatures remain unchanged.
  Both paths must produce only validated plans.
- Keep checked downcasts as runtime defense. A valid plan's stores remain fresh
  for each run, and describe remains independent of contexts and stores.

Recommended 0.3 input boundary: seal the currently documented-as-internal
`OperationInputs` implementation surface and move store plumbing behind private
traits. Keep the supported `()`, single input, and two-to-eight-input tuples.
Document the breaking change for users implementing this public trait today.
Custom input adapters should be a separately specified extension if consumer
requirements justify them.

### Enforce one explicit macro grammar

Parse the pipeline into ordinary statements, bound steps, ignored steps, and a
final output before emitting builder calls. Apply the same handle-use rules to
ordinary statements and operation arguments.

Allow a step-produced handle as a direct operation argument or final output.
Reject transformations, aliases, borrows, and inspection of handles elsewhere.
Track scopes and rebinding explicitly; preserve existing repeated step bindings
such as `let state = step!(next(state))?`. Define conservative diagnostics for
handles inside opaque macro tokens instead of claiming to understand arbitrary
macro expansion. Preserve ordinary plan-time Rust that does not use handles.

Retain the existing parse/validate/lower module split. This needs a small
validated representation, not a general Rust compiler frontend. Include
generated-name collision and renamed-dependency cases in the diagnostic suite.

### Preserve error data through the runtime

Introduce structured construction and invariant errors with node identifiers
and relevant dependency/type details. Carry them through execute errors,
dry-run outcomes, and borrowed progress outcomes. Preserve the user's operation
error `E` and its source chain without adding `Clone` or `Display` bounds.

Format errors in `Display` and console adapters. Document changes to public enum
variants and JSON forms, and test serialization round trips. Resolve the
current five-outcome description versus the extra internal-error variant in
`SEMANTICS.md` explicitly.

Keep execute and dry-run as separate traversal loops. Their failure and policy
semantics are intentionally different. Share small metadata or dependency
helpers only where doing so improves clarity.

## Delivery sequence

| Change | Scope | Acceptance criteria |
|---|---|---|
| 1. Validate plans | Builder identity, internal dependency references, input boundary, `PlanBuildError`, `try_finish` | Foreign inputs and final outputs are rejected before any body runs; same-type collisions cannot return local values; ordinary plans remain reusable; update `DESIGN.md` and construction semantics. |
| 2. Tighten pipeline validation | Shared grammar validation and binding tracking | The confirmed argument-transformation example fails with a targeted diagnostic; aliases, scopes, rebinding, generated names, and renamed dependencies have coverage; valid existing pipelines still compile. |
| 3. Preserve runtime errors | Structured invariant errors and report/progress propagation | Callers can match error kinds without string parsing; original operation errors remain available; listener ordering and dry-run continuation remain unchanged. |
| 4. Verify supported configurations | Feature gates, rustdoc, consumer checks, MSRV | Default, no-default, serde-only, and all-feature configurations have working tests/docs; macro-only examples have `required-features`; choose and test an explicit minimum Rust version. |
| 5. Harden release tooling | Safe cleanup, metadata-derived version/dependency entries, consumer fixtures | Unknown/nonempty override directories are refused; only owned generated content is cleaned; packaging works after a version change; local consumers cover manual and macro APIs. |
| 6. Prepare 0.3.0 | Migration notes, changelog, package metadata, examples | Explain input-trait and error changes; distinguish report serialization from executable-plan persistence; pass the release gates and local registry smoke test. |

Changes 1-3 define the release. Changes 4-5 can be developed independently once
the intended public API is written down. Change 6 follows all five.

For cleanup, prefer a dedicated generated child directory with an ownership
marker and validated paths. Guard against repository/home/root targets and
symlinks before deleting anything. Derive packaging metadata from Cargo rather
than maintaining a parallel list in shell. Test the cleanup logic in disposable
directories.

Choose the MSRV from builds of both the manual runtime and macro configurations,
including their dependency requirements. Do not infer it from the Rust edition.
Declare it in package metadata and test it in CI; the
[Cargo rust-version documentation](https://doc.rust-lang.org/cargo/reference/rust-version.html)
describes the supported declaration and compatibility considerations.

## Semantic release gates

In addition to the repository's format, clippy, and test commands, require:

- Builder ownership and final-output validation tests with body-call counters.
- Policy-before-dependency tests for both skipped and denied nodes with missing
  inputs; independent work after failed, skipped, denied, and blocked nodes.
- Execute fail-fast and repeated-run isolation tests across the representation
  change; concurrent runs of one plan using separate contexts/stores.
- Macro compile-pass and compile-fail cases for the validated grammar.
- Runtime tests with default features disabled, both with and without serde.
  Check strict rustdoc in those configurations too.
- JSON round trips for all outcome/error variants, including non-Clone errors
  where the runtime promises not to require cloning.
- At least one independently compiled consumer exercising each supported
  feature configuration and the declared MSRV.
- The read-after-write example and local registry smoke test. Publish example
  checks should use help/dry-run as appropriate; publishing is a separate step.

## Later work

After the construction contract is sound, evaluate synchronous `#[operation]`
support and an explicit per-operation error adapter against real consumer code.
Both address visible API limitations without changing execution semantics.

Keep borrowed/non-Clone values, plan composition, configurable runner builders,
and performance changes for demonstrated needs. Benchmark before replacing the
store or boxed futures. Runtime branching, retries, rollback, persistent plans,
and parallel graph execution need separate designs under the repository rules.

No new runtime dependency or feature is required just to repair ownership and
macro validation.

## Verification performed for this review

| Check | Result |
|---|---|
| `cargo fmt --all --check` | Passed. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed. |
| `cargo test --workspace --all-features --locked` | Passed: 49 integration test functions, including the macro UI harnesses. Both crates currently have zero doctests. |
| `cargo check -p rehearse --no-default-features --lib --locked` | Passed. |
| `cargo test -p rehearse --no-default-features --tests --locked` | Failed on unconditional macro imports/use. |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p rehearse --no-default-features --no-deps --locked` | Failed on the crate-level macro links. |
| `cargo run -p rehearse --example read_after_write --locked` | Passed; 4 executed, 2 skipped, 1 blocked, 0 failed. |
| Four isolated runtime probes | Confirmed foreign-input collision, foreign-output collision, invalid empty-plan discrepancy, and step-argument validation bypass. |

The probes and failure logs are under the ignored directory
`target/review-probes-efw7zbc5/`. Run them with
`cargo test --offline --manifest-path target/review-probes-efw7zbc5/Cargo.toml`.
Their assertions describe the current defects; implementation work should turn
these scenarios into regression tests requiring rejection of invalid input.

This review used Rust 1.93.0 on the current macOS workspace. It did not establish
an MSRV, rerun the local registry smoke script, test every supported platform,
or publish anything. Runtime and macro source files were not changed.

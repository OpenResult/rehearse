# Changelog

All notable changes to this project are documented here.

## Unreleased

- Add the reusable, initially non-published `rehearse-github` reporting adapter
  with live Actions notices/errors, timing and outcome summaries, canonical graph
  JSON, event JSONL, and final reports, including never-started operations.
- Add opt-in CLI reporting and a manual-only Workflow Example E2E workflow for
  Rust and Spring Boot fixtures, with selectable scenarios and retained reports.
- Add canonical `describe_graph()` metadata with versioned JSON, explicit
  sequence/fork/join structure, application revisions, annotations, and structural
  Mermaid rendering while preserving existing table descriptions.
- Add manual bounded parallel groups and opt-in concurrent execute/dry-run
  interpreters. Preserve sequential defaults; stop new launches and drain active
  operations after execute failure, retaining structured outcomes.
- Replace the office filing CLI with `rehearse-workflow-example`, a local CI/CD
  fixture lab for Rust and Spring Boot services with CI, release, promotion,
  audit, and maintenance plans.
- Demonstrate all six impacts, custom dry-run policy, strict completeness,
  controlled failures, progress, Mermaid, and structured JSON output without
  requiring external build tools or services.

- Add the local `rehearse-workflow-server` browser example with owned SSE
  progress events, reconnect snapshots, typed production approval, rejection,
  expiry, cancellation, and isolated Rust/Spring Boot fixture environments.
- Draw its canonical graph before execution, run independent dev/staging lanes,
  and require both verifications before asking for production approval.

## 0.4.0 - 2026-11-21

- Update direct and transitive dependencies, including Tokio, Clap, serde_json,
  and Trybuild.
- Upgrade the macro parser to Syn 3 while preserving match-guard scope and
  step-value validation, with additional compile-pass/fail regression coverage.
- Preserve the public API and Rust 1.85 library minimum supported version.

## 0.3.0 - 2026-09-17

- Validate builder ownership, producer order, and output types before returning
  plans; add `PlanBuilder::try_finish` and `PlanBuildError`.
- Seal `OperationInputs` and make value-store plumbing private.
- Preserve structured `InvariantError` and `ValueError` data through execution,
  reports, progress listeners, and error source chains.
- Reject pipeline handle transformations, aliases, and opaque macro uses;
  respect lexical shadowing and isolate generated local names.
- Fix testing and rustdoc with macros disabled; add four-configuration CI on
  Linux and macOS and declare/test Rust 1.85 as the library MSRV.
- Guard local registry cleanup with ownership markers and derive packaging
  metadata from Cargo; verify four packaged consumer configurations.
- Document breaking API and serialized error changes in `MIGRATION.md`.
- Add the non-published `rehearse-filing-example` workspace CLI with sample office
  documents, static descriptions, dry-run destination checks, and filing without
  overwriting existing destinations. Keep its dependencies isolated from library
  feature checks and release packaging.

## 0.2.0 - 2026-06-23

- Added progress listeners for describe, dry-run, and execute traversals.
- Added the seeded `conditional_rollout` example.
- Added `ConsoleProgress`, `Plan::to_mermaid()`, `Operation::sync`, and
  `DryRunReport::require_complete()`.
- Added optional `serde` support for public description, report, status, and
  error types.
- Expanded operation input support from three to eight non-context inputs.
- Improved describe/report display with dynamic columns and dependency names
  for blocked dry-run nodes.
- Standardized example CLI parsing with `clap`.

## 0.1.1 - 2026-06-22

- Added `Plan::describe_execution()` for execute-mode static plan rendering
  without a dry-run action column.

## 0.1.0 - 2026-06-22

- Added the `rehearse` runtime crate with ordered typed plans, manual
  `PlanBuilder` construction, static description, execute, dry-run, and
  structured reports.
- Added `SafeDryRun` with run/skip/deny policy handling, dependency blocking,
  no fabricated outputs, and deterministic ASCII report rendering.
- Added the `rehearse-macros` crate and default `macros` feature with
  `#[operation]`, `#[pipeline]`, and `step!`.
- Added compile-fail and runtime tests for operation and pipeline macro
  diagnostics.
- Added compiled examples for read-after-write dry-run behavior, VS Code
  settings configuration, and guarded crates.io publishing.
- Added local file-backed registry smoke testing through
  `scripts/publish-local.sh`.

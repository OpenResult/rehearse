# Changelog

All notable changes to this project are documented here.

## Unreleased

### 0.3.0 preparation

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

The workspace is prepared for 0.3.0; this entry does not claim publication.

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

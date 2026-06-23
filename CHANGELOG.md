# Changelog

All notable changes to this project are documented here.

## Unreleased

No changes yet.

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

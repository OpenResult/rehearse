# Changelog

All notable changes to this project are documented here.

## Unreleased

- No changes yet.

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

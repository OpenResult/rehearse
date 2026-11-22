# rehearse-github

A reusable GitHub Actions reporting adapter for `rehearse`. This workspace crate
is Unreleased and is not published to crates.io yet. Use a path dependency from
this checkout.

The adapter implements `ProgressListener`: it observes the existing sequential
and concurrent interpreters. It does not schedule operations, alter dry-run
policy, upload data, or call the GitHub API. It has no HTTP or async-runtime
dependency. The application keeps the original operation error and handles
reporting errors separately.

```rust,ignore
use rehearse_github::{GitHubReporter, ReportOptions, ReportOutcome};

let options = ReportOptions::new("target/reports/release-1")
    .with_github_summary()
    .with_metadata("commit", "application-supplied-build-identity");
let mut reporter = GitHubReporter::new(plan.describe_graph(), options)?;
let result = plan.execute_with_listener(&context, &mut reporter).await;
let outcome = if result.is_ok() {
    ReportOutcome::Succeeded
} else {
    ReportOutcome::Failed
};
let reporting_result = reporter.finish(outcome);
// Handle both results. In particular, do not replace an operation error with
// an unrelated failure to write a report.
```

For a dry-run, pass `Incomplete` when policy skips/denies work or unavailable
values block it. A strict application using `require_complete` may instead pass
`Failed`. Node outcomes stay unchanged, showing why the application rejected the
run. Passing `Succeeded` never downgrades an observed failure or turns unfinished
work into a completed node.

## Outputs

One reporter observes one plan traversal. Its output directory must be new or
empty. Paths containing symlinks or parent traversal are refused; report files
use create-new semantics and never overwrite existing content. Use distinct
directories for separate scenarios or runs. The optional GitHub summary path
must be outside the report directory.

| File | Content |
| --- | --- |
| `graph.json` | Canonical `PlanGraphDescription`, written before traversal |
| `events.jsonl` | Schema-version-1 owned events, flushed in observation order |
| `report.json` | Schema-version-1 final outcome and declaration-ordered node rows |
| `summary.md` | Markdown outcome, metadata, timing, and node table |

Reports include plan-local node IDs, plan revision, interpretation, application
metadata, status, timing, diagnostics, and unavailable dependency IDs. They do
not contain literal input values, stored outputs, or executable bodies. Failure
diagnostics are formatted at this presentation boundary; operation errors need
`Display` here but do not need `Clone` or `Serialize`.

Parallel node events may interleave. Each live log line includes its node ID;
the adapter avoids log groups that would incorrectly nest overlapping work.
After concurrent failure, finalization follows the runner's drain and reports
the actual outcomes of active peers. Unlaunched nodes remain `not_started`.
Dropping a reporter without calling `finish` leaves partial event artifacts;
it cannot produce a complete summary for a process terminated by the runner.

## GitHub Actions

Live output uses escaped `::notice` and `::error` commands. The default writer is
stdout; use `GitHubReporter::with_writer(graph, options, std::io::stderr())` to
keep machine-readable stdout separate. Set `.with_live_log(false)` to disable
workflow-command output without disabling artifacts.

`.with_github_summary()` explicitly reads `GITHUB_STEP_SUMMARY`, if present, and
appends the completed Markdown summary during `finish`. Local runs always write
`summary.md`, and can set a test destination with `.with_summary_path(path)`.
The adapter escapes command data and Markdown metadata independently.

GitHub displays the summary after the Actions step finishes. Upload the local
report directory from a later artifact step that also runs after failures. The
adapter does not require a GitHub token or write permissions. It does not add
nodes to GitHub's native job graph or provide an interactive browser view.

The workflow example exposes this integration with `--github-report DIR`. See
the repository's manual **Workflow Example E2E** Actions workflow for a complete
fixture-only run.

# rehearse-workflow-example

A local CI/CD lab for learning `rehearse` with Rust and Java Spring Boot
workflows. It replaces the office filing example.

The main release path is:

```text
build → test → deploy → verify deployment → publish to API gateway
```

Each stage is an explicit operation with a declared impact and typed inputs.
The CLI lets you describe the static plan, rehearse it without committing changes,
or execute it against local fixture files. Five workflows show different parts
of the library rather than adding a workflow engine to it.

The lab uses deterministic fixtures only. It does not run Cargo, Maven, Java,
Docker, HTTP servers, cloud deployments, or real API gateway publication. The
Rust and Spring Boot command labels show where those integrations would go.
You need Rust to run this CLI, but no Java installation or external credentials.

## Start with a release

Run these commands from the repository root:

```bash
# Create a new lab with fixtures for both stacks.
cargo run -p rehearse-workflow-example -- --init

# Inspect the plan without reading the lab or running an operation body.
cargo run -p rehearse-workflow-example -- --describe

# Rehearse the Rust release. This is the default mode.
cargo run -p rehearse-workflow-example

# Execute the same workflow, with operation progress.
cargo run -p rehearse-workflow-example -- --execute --progress

# Try the equivalent Spring Boot release.
cargo run -p rehearse-workflow-example -- --stack spring-boot --execute --progress
```

The default directory is `target/workflow-lab`, the default stack is `rust`,
and the default workflow is `release`. Release and CI examples use revision
`demo-1` unless you supply `--revision`. Promotion uses the existing `seed`
revision by default.

`--init` creates a new lab and refuses an existing root. Use a different directory
for a fresh exercise:

```bash
cargo run -p rehearse-workflow-example -- target/workflow-second --init
cargo run -p rehearse-workflow-example -- target/workflow-second --execute
```

Choose a new revision when trying a different build or deployment in the same
lab. The lab refuses conflicting contents instead of silently replacing an
existing revision:

```bash
cargo run -p rehearse-workflow-example -- --revision demo-2 --execute
```

## What changes locally

Initialization seeds source fixtures, an existing tested artifact named `seed`,
previous and obsolete deployments, and a gateway route pointing at the previous
deployment for each stack. This makes promotion, audit, and cleanup useful
without first executing a release.

```text
target/workflow-lab/
├── sources/
│   ├── rust/          # source.rs and fixture.json
│   └── spring-boot/   # DemoApplication.java and fixture.json
├── artifacts/         # build receipts, by stack and revision
├── test-reports/      # test receipts, by stack and revision
├── deployments/       # deployed fixture records, by stack and revision
├── gateway/           # current local route for each stack
└── legacy/            # local legacy-hook receipts, created during execute
```

These files represent the example's managed domain state. They are not saved
plans, execution checkpoints, or engine persistence.

| Stack | Illustrative build command | Illustrative test command | Local result |
|---|---|---|---|
| Rust | `cargo build --release` | `cargo test` | Artifact and test-report fixtures, then a deployment and gateway route |
| Spring Boot | `mvn -B package -DskipTests` | `mvn -B test` | Equivalent fixture receipts for a Spring Boot service |

The command labels are data in the fixtures, never shell commands. Verification
reads the recorded deployment and fixture health expectation; it does not probe
a running application. Publishing the gateway updates a local route record.

## Choose a workflow

Use `--workflow ci|release|promote|audit|maintenance` with either stack.

| Workflow | Operations to explore | Default dry-run behavior | Main lesson |
|---|---|---|---|
| `ci` | Read and validate source, build, test, verify the test report, observe the gateway | Source validation runs; build/test writes are skipped; receipt-dependent checks block | Even local build tools produce managed output |
| `release` | Build → test → deploy → verify → publish gateway, with preflight and independent observation | Writes are skipped; deployment verification blocks; independent observation still runs | Order and typed value dependencies are separate |
| `promote` | Read an existing tested artifact → deploy → verify → publish gateway | Existing artifact checks run; mutations skip; downstream verification blocks | Rehearse promotion using actual existing evidence |
| `audit` | Authenticate, read and validate fixtures, inspect existing state | All operations can run with the safe policy | A complete non-committing plan; manual builder frontend |
| `maintenance` | Delete inactive deployments, invoke a legacy hook, verify its receipt, observe the gateway | Delete skips, opaque hook is denied, dependent verification blocks, independent observation runs | Delete, Opaque, and distinct incomplete outcomes |

The plans are built before their interpretation is selected. Choosing a workflow
selects a static ordered plan; it does not introduce runtime branching. Most
workflows use `#[operation]`, `#[pipeline]`, and `step!`. Audit uses
`Operation::sync`, `PlanBuilder`, typed `Value<T>` handles, and `try_finish`.

### CI without deployment

```bash
cargo run -p rehearse-workflow-example -- --workflow ci --describe
cargo run -p rehearse-workflow-example -- --workflow ci --stack spring-boot --dry-run
cargo run -p rehearse-workflow-example -- --workflow ci --revision ci-1 --execute
```

Build and test are `Write` operations because their fixture artifacts and
reports persist in the lab. Real Cargo and Maven commands also write artifacts,
reports, and caches, so they should not be declared `Pure` just because they
happen before deployment.

### Promote an existing artifact

```bash
cargo run -p rehearse-workflow-example -- --workflow promote --dry-run
cargo run -p rehearse-workflow-example -- --workflow promote --execute
cargo run -p rehearse-workflow-example -- --stack spring-boot --workflow promote --execute
```

Promotion reads the seeded tested artifact. It does not manufacture a successful
build result when a build was skipped. You can also promote a revision previously
produced by an executed CI workflow:

```bash
cargo run -p rehearse-workflow-example -- --workflow ci --revision candidate-1 --execute
cargo run -p rehearse-workflow-example -- --workflow promote --revision candidate-1 --execute
```

### Audit and strict completeness

```bash
cargo run -p rehearse-workflow-example -- --workflow audit --require-complete
cargo run -p rehearse-workflow-example -- --stack spring-boot --workflow audit --format json
```

Audit demonstrates a genuinely complete dry-run. `--require-complete` makes any
skipped, denied, or blocked operation cause a failing exit status, as well as
ordinary failures. It is useful when CI requires proof that every planned check
actually ran.

The same flag deliberately fails for a default release dry-run because release
writes must be skipped:

```bash
cargo run -p rehearse-workflow-example -- --workflow release --require-complete
```

### Maintenance and opaque work

```bash
cargo run -p rehearse-workflow-example -- --workflow maintenance --describe
cargo run -p rehearse-workflow-example -- --workflow maintenance --dry-run
cargo run -p rehearse-workflow-example -- --workflow maintenance --execute --progress
```

Cleanup removes deployments other than the revision referenced by the gateway, including the seeded obsolete deployment. The legacy hook is deliberately
classified `Opaque`, so safe dry-run denies it. In execute mode its implementation
writes only a local fixture receipt. The example never invokes an arbitrary
legacy script.

## Impacts, policies, and outcomes

| Impact | Example operation | Safe dry-run action |
|---|---|---|
| `Pure` | Validate already-read source metadata or assemble a summary | Run |
| `Session` | Create an in-memory example authentication session | Run |
| `Read` | Read source, test evidence, deployment health, or gateway state | Run |
| `Write` | Persist a build, test report, deployment, or gateway route | Skip |
| `Delete` | Remove an obsolete local deployment | Skip |
| `Opaque` | Invoke the deliberately unclassified local legacy hook | Deny |

The default `safe` policy is `SafeDryRun`. The `no-session` policy denies
`Session` operations and otherwise uses the same decisions. Try it against
static descriptions and audit:

```bash
cargo run -p rehearse-workflow-example -- --describe --policy no-session
cargo run -p rehearse-workflow-example -- --workflow audit --policy no-session --progress
```

Policy applies only to dry-run and its description. Execute always visits every
operation in order, so the CLI rejects `--execute --policy no-session` rather
than implying the custom policy protects execution.

The reports distinguish:

- **Executed:** the operation body ran successfully and produced a real value.
- **Skipped:** policy chose Skip; no operation body ran and no value exists.
- **Denied:** policy chose Deny; no operation body ran and no value exists.
- **Blocked:** policy allowed the operation, but an explicit input value was
  unavailable.
- **Failed:** an allowed operation ran and returned an error; no value exists.

Dry-run checks policy **before** resolving dependencies. A gateway publication
can therefore be reported as skipped even when its verification input is
unavailable. A read that needs that same unavailable value is blocked instead.
The library never substitutes a placeholder artifact or deployment receipt.

Each workflow includes a later independent gateway observation. Its availability
comes from its own inputs, not from whether earlier operations succeeded. A
skipped, denied, blocked, or failed node does not automatically block the rest
of the plan.

Ordinary dry-run exits successfully when it has only expected incomplete
outcomes. A failed operation returns a failing exit status after the report has
visited independent remaining work. `--require-complete` also rejects incomplete
reports. Defensive internal invariant errors are covered by the library's tests;
valid example plans do not deliberately manufacture them.

## Exercise failures

`--fail-at` injects an error in an operation body. Valid stages are
`authenticate`, `read-source`, `validate`, `build`, `test`, `deploy`, `verify`,
`gateway`, `observe`, `cleanup`, and `legacy`. A stage must actually run to fail;
a skipped build cannot fail during safe dry-run.

Use a fresh directory for these exercises:

```bash
cargo run -p rehearse-workflow-example -- target/workflow-failures --init

# Dry-run reports a read failure and still observes independent gateway state.
# This command is expected to exit unsuccessfully.
cargo run -p rehearse-workflow-example -- target/workflow-failures --fail-at read-source --progress

# Execute stops when deployment verification fails. Gateway publication does not run.
# Build, test, and deployment files already created remain in the lab.
cargo run -p rehearse-workflow-example -- target/workflow-failures --revision broken-1 --execute --fail-at verify --progress

# Try a successful release under a new revision.
cargo run -p rehearse-workflow-example -- target/workflow-failures --revision fixed-1 --execute --progress
```

Execute is fail-fast: later operations, including the independent observation,
are not invoked after the first error. There is no rollback, retry, or hidden
recovery. Read the local files to see which writes committed before the failure.

## Inspect plans and structured output

Static describe and Mermaid do not require an initialized lab and do not invoke
operation bodies:

```bash
cargo run -p rehearse-workflow-example -- target/not-created --describe
cargo run -p rehearse-workflow-example -- target/not-created --workflow release --mermaid
cargo run -p rehearse-workflow-example --quiet -- --workflow release --describe --json
```

JSON is available for descriptions, dry-run reports, and execution summaries.
Use `--json` (shorthand for `--format json`) when a later tool needs structured
data instead of terminal text. Choose either spelling, rather than combining
them. `--progress` uses rehearse's `ConsoleProgress` listener and is supported
only with text output, so progress lines cannot corrupt JSON.

```bash
cargo run -p rehearse-workflow-example --quiet -- --workflow audit --format json
cargo run -p rehearse-workflow-example --quiet -- --revision json-1 --execute --format json
```

`--describe --json` exports the canonical `PlanGraphDescription`, schema version
1: plan name/revision, node IDs and positions, declared impacts, static
annotations, value dependencies, and execution steps. Sequence and explicit
parallel groups are separate from value edges, so a value fan-out does not imply
concurrent execution. Text descriptions retain their policy-action table.
Mermaid uses the same canonical structure, with solid control arrows and dotted
additional value dependencies. Static exports contain no operation bodies,
literal input values, or runtime results. Dry-run reports retain their outcome
JSON; executable plans are not serialized.

## GitHub reporting and CLI E2E

The checkout's `rehearse-github` adapter produces live operation notices and a
completed-run report. Enable it explicitly with a fresh report directory outside
the fixture lab:

```bash
cargo run -p rehearse-workflow-example -- target/github-workflow-lab --init
cargo run -p rehearse-workflow-example -- target/github-workflow-lab \
  --execute --stack rust --github-report target/github-workflow-report
```

The adapter writes `graph.json`, `events.jsonl`, `report.json`, and `summary.md`.
Its node rows retain declaration order, timings, and actual outcomes, including
work never started after execute failure. Skipped, denied, and blocked dry-run
nodes remain distinct. Reports contain metadata and diagnostics, not operation
bodies or stored output values. Report directories must be new or empty; rerun
with a different path to preserve earlier evidence.

When `GITHUB_STEP_SUMMARY` is set, the adapter also appends the Markdown summary
to that file. GitHub displays it after the Actions step completes. Live progress
commands go to stderr, so `--github-report` can be combined with `--json` while
stdout remains valid JSON. Use it instead of `--progress`; static describe,
Mermaid, and fixture initialization do not accept the reporting option. The lab
must be separate from both the report directory and the Actions summary path.

An operation failure keeps its original exit status even if report finalization
also fails; the reporting error is printed separately. A reporting failure after
an otherwise successful workflow produces a nonzero CLI exit. Strict dry-run
completeness failure is recorded as a failed overall run while retaining each
node's skipped or blocked outcome.

The **Workflow Example E2E** Actions workflow is manual-only. In the repository's
Actions tab, select it and choose **Run workflow**, then select a fixture stack
(`all`, `rust`, or `spring-boot`) and scenario (`suite`, `dry-run`, `release`, or
`failure`). It runs the actual CLI, verifies fixture files and generated reports,
and uploads evidence even when an assertion fails. The injected verification
failure is an expected test: the E2E job passes when it confirms that deployment
remains but gateway publication never happens. No Java, Maven, server, browser,
cloud credentials, or real deployment is required.

After building the binary once, run the same E2E driver locally:

```bash
cargo build -p rehearse-workflow-example --bin rehearse-workflow-example --locked
python3 scripts/workflow_e2e.py --stack all --scenario suite
```

The manual workflow becomes available in GitHub after its YAML file is present
on the repository's default branch. This example does not publish a live graph
inside GitHub's native job diagram; it reports operation progress within the job.

## Browser server example

Run the local dashboard to preview the complete graph before execution, stream
progress through parallel dev and staging lanes, then answer an explicit
production approval question after both verifications succeed:

```bash
cargo run -p rehearse-workflow-example --bin rehearse-workflow-server
```

Open the printed loopback URL. The [server guide](SERVER.md) covers approval,
rejection, timeout, cancellation, reconnect, HTTP endpoints, and the in-memory
lifetime of runs. It uses the same Rust and Spring Boot fixture approach and
requires no external deployment service.

The browser is buildless HTML, CSS, and vanilla JavaScript embedded into the
Axum binary. It sends commands with `fetch` and receives owned progress snapshots
over Server-Sent Events. The server owns each task and its in-memory event history
independently of browser connections. A run's canonical graph supplies static
structure; node IDs and plan revisions bind live status overlays to that graph.

## Boundaries

This example is for local experimentation with explicit operation impact,
static composition, interpretation, and dependency semantics. Fixture command
labels and local gateway records do not claim that a Rust or Spring Boot
application was compiled, tested, started, or exposed over HTTP. Adding real
service integrations would require operation bodies that honor the same declared
impact contract, along with explicit process and environment management.

The CLI workflows remain sequential. The server explicitly opts into bounded
parallel groups through the manual builder; after a failure it stops launching
work and drains already-running operations. Neither example adds distributed
execution, runtime branching, retries, rollback, persistent workflow state, or
automatic detection of mutations. The core library remains runtime-agnostic;
these applications use Tokio to drive async operations.

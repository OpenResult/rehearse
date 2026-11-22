# Local workflow server

This example serves a browser dashboard for a static rehearse release plan. It
streams progress, waits for a production decision, and continues only when the
local operator approves the exact tested artifact.

Everything deployed is a local fixture. The server does not invoke Cargo,
Maven, Java, Docker, a cloud API, or a real API gateway.

## Start it

From the repository root:

```bash
cargo run -p rehearse-workflow-example --bin rehearse-workflow-server
```

Open the printed URL, normally **http://127.0.0.1:3000**. The server creates and
owns `target/workflow-server`; no separate CLI `--init` is needed. It refuses
an unrelated nonempty directory or a symlink in its managed fixture namespace.

Choose another directory, port, decision timeout, or pacing delay with:

```bash
cargo run -p rehearse-workflow-example --bin rehearse-workflow-server -- \
  target/workflow-server-second \
  --bind 127.0.0.1:3001 \
  --approval-timeout-secs 30 \
  --step-delay-ms 200
```

The decision timeout defaults to 120 seconds and accepts 1–3600 seconds. The
pacing delay defaults to 350 milliseconds and accepts 0–2000 milliseconds.
`--bind 127.0.0.1:0` chooses a free port. Bind addresses must be loopback.

## Try the release flow

Select Rust or Spring Boot. The dashboard draws the static plan before execution.
Start an execute run to follow this structure:

```text
authenticate → read source → build → test
  → fork (maximum 2 operations in flight)
      ├─ deploy dev → verify dev
      └─ deploy staging → verify staging
  → join both verified receipts → request production approval
  → deploy production → verify production → publish API gateway
  → observe gateway → summarize
```

At the approval question, dev and staging have completed. Production remains
pending. Inspect the revision and SHA-256 digest before choosing **Approve** or
**Reject**.

The server uses an explicit bounded parallel group. Each lane preserves its
deploy-before-verify order and both consume the same tested candidate. Approval
cannot start until both real verification outputs are available. Ordinary
rehearse `execute` and the CLI workflows remain sequential; this server selects
the opt-in concurrent interpreter.

- **Approve:** the gate produces a typed production permit. Production consumes
  it, rechecks the tested bytes and non-production evidence, deploys, verifies,
  and publishes the local gateway route.
- **Reject:** execution ends with a `rejected` application status. Production
  deployment and publication are not invoked.
- **Wait:** the question expires, yielding `timed_out`; production remains
  untouched.
- **Cancel:** the run ends as `cancelled`. Fixture writes that already completed
  remain available for inspection. Cancellation drops the active run future;
  it does not drain operations or roll back a deployment.

Each execute run has a separate workspace and generated revision. Its Rust or
Spring Boot bundle is identical across the dev, staging, and production labs.
The deployment adapter materializes that bundle and fixture test evidence in
its destination; it does not execute a compiler or test tool.

## Rehearse and inject failures

A dry-run traverses the same static plan with a server-specific policy:

- `SafeDryRun` handles ordinary impacts.
- The interactive approval operation is explicitly skipped.
- Build, test, deployment, and gateway writes are skipped.
- Receipt-dependent reads block; the independent gateway observation still runs.

No approval question opens and no placeholder permit is created. Dry-runs read
an initialized preview workspace; interpretation does not write that workspace
or create an execute-run directory. The normal result is `incomplete`, matching
rehearse's skipped and blocked outcomes.

Use the failure selector to exercise build, test, dev verification, staging
verification, or production verification errors. Failures occur only if the selected operation
body actually runs. For example, an injected build failure does not occur in a
dry-run because policy skips build. A production verification failure occurs
after production deployment and prevents gateway publication; its deployment
file remains in the workspace.

On the first observed execute failure, the concurrent runner stops launching new
operations and drains those already in flight. If dev verification fails while
staging verification is active, staging still reports its actual outcome; the
production gate and later work remain unstarted. This differs from explicit
cancellation, which drops the run future immediately. Completed effects remain
in either case.

## Web architecture

The dashboard uses plain HTML, CSS, and vanilla JavaScript, embedded in the Rust
binary with `include_str!` and served by Axum. There is no frontend build step or
external asset service. Browser commands use `fetch`; progress uses native
`EventSource` and Server-Sent Events.

Axum handles plan previews, run creation, approvals, cancellation, and event
streams. An in-memory run registry owns Tokio execution tasks, snapshots, decision
channels, and bounded event history. A synchronous rehearse progress listener
copies borrowed events into owned JSON; HTTP delivery does not drive execution.
The fixture adapter writes local artifacts and environment records. The core
library has no HTTP, browser, or Tokio dependency.

`GET /api/plan` returns `Plan::describe_graph()` as canonical schema-version-1
JSON. It records node metadata, value dependencies, static annotations, an
application plan revision, and explicit sequence/parallel execution steps.
Annotations supply labels, environments, and approval kinds without parsing
operation names. The browser draws that structure before execution; it does not
infer parallelism from missing data edges. Graph export never invokes operation
bodies or reads context, stores, or literal input values.

## Progress and reconnect

The server owns each task independently of HTTP requests. Refreshing the page,
closing a tab, or disconnecting an event stream does not cancel the run. Select
the existing run after reconnecting to recover its current state and pending
question. Decisions can also be sent through the HTTP API.

Every server event has a per-run sequence number, timestamp, kind, message,
`run_id`, `plan_revision`, optional `node_id`, and a full owned run snapshot.
The snapshot contains the canonical `graph` and separate `nodes` status overlays
matched to graph node IDs. The browser combines static metadata and runtime
status using run ID, plan revision, and node ID; multiple nodes may be running.
The synchronous rehearse listener copies borrowed node metadata and error
diagnostics into these owned events. A separate SSE response
streams them; slow clients never make an operation wait for network delivery.

Each run retains the latest 256 events in memory. SSE accepts `Last-Event-ID`,
or `?after=<sequence>`, and replays later events. If the cursor predates retained
history, a `reset` event supplies the current full snapshot. The browser applies
snapshots in sequence order. `GET /api/runs/{id}` is also available for explicit
resynchronization.

## HTTP API

The dashboard uses these local endpoints:

| Method and path | Purpose |
|---|---|
| `GET /api/plan` | Canonical versioned graph with execution structure and value edges; no operation bodies run |
| `POST /api/runs` | Start a server-owned dry-run or execute run; returns `202` and its snapshot |
| `GET /api/runs` | List retained runs |
| `GET /api/runs/{id}` | Current full snapshot |
| `GET /api/runs/{id}/events` | SSE `progress` and `reset` events |
| `POST /api/runs/{id}/approvals/production` | Approve or reject the pending production question |
| `POST /api/runs/{id}/cancel` | Cancel; JSON body `{}` |

Start an execute run:

```bash
curl -sS http://127.0.0.1:3000/api/runs \
  -H 'Content-Type: application/json' \
  -d '{"stack":"rust","mode":"execute"}'
```

Use `"stack":"spring-boot"` for the other fixture. `mode` defaults to
`"dry-run"` if omitted. The optional `fail_at` values are `"build"`, `"test"`,
`"verify-dev"`, `"verify-staging"`, and `"verify-production"`.

Copy the returned run ID into the following URLs:

```bash
curl -sS http://127.0.0.1:3000/api/runs/RUN_ID
curl -N http://127.0.0.1:3000/api/runs/RUN_ID/events
```

Once the snapshot has `status: "awaiting_approval"`, copy the exact
`approval.subject.artifact_digest` and `approval.subject.revision` into the
request. The following values are placeholders:

```bash
curl -sS http://127.0.0.1:3000/api/runs/RUN_ID/approvals/production \
  -H 'Content-Type: application/json' \
  -d '{"decision":"approve","artifact_digest":"sha256:EXACT_DIGEST","revision":"EXACT_REVISION"}'
```

Use `"decision":"reject"` to reject. A matching repeated decision is
idempotent; a conflicting, stale, expired, or artifact-mismatched decision
returns `409`. The permit also binds the run ID, gate ID, and production target.
An approval cannot silently authorize a changed bundle after a rebuild.

Cancel with:

```bash
curl -sS http://127.0.0.1:3000/api/runs/RUN_ID/cancel \
  -H 'Content-Type: application/json' -d '{}'
```

State-changing requests use JSON. Browser requests must originate from the
local dashboard; other origins are rejected. This is a single-local-operator
demo, with no user accounts or production authorization system.

## Files and lifetime

```text
target/workflow-server/
├── .rehearse-workflow-server
├── preview/
│   ├── dev/              # initialized read-only dry-run fixtures
│   ├── staging/
│   └── production/
└── runs/
    └── RUN_ID/
        ├── dev/          # sources, artifacts, test reports, deployments, gateway
        ├── staging/
        └── production/
```

Run snapshots, pending approvals, decisions, and event history live only in
memory. Restarting the server forgets them. Fixture workspaces remain on disk
for inspection, and later runs receive new IDs rather than resuming old work.
The server retains at most 32 runs per process, including finished runs. Restart
it to begin another session; an owned directory can be reused.

The server is an application layer around rehearse's explicit parallel groups.
It does not add durable checkpoints, executable-plan serialization, retries,
rollback, or runtime branching. Its `awaiting_approval`, `rejected`, `timed_out`, and
`cancelled` states describe the server interaction. The core still observes an
async gate operation producing a real permit or failing. Concurrent execute
stops new launches at the first observed error and drains active operations.

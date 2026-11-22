# Describe graphs and explicit concurrent execution

This design adds metadata-only graph descriptions and bounded, explicitly declared
parallel regions. Existing `execute()` and `dry_run()` behavior remains sequential.
The extension does not add distributed workers, retries, rollback, durable state,
executable-plan serialization, automatic effect analysis, or runtime branching.

## Static structure

A plan retains its flat declaration-ordered nodes and builder-owned `Value<T>`
handles. It additionally records a top-level sequence of node steps and parallel
groups. A group contains named branches, each an ordered list of ordinary nodes,
and a nonzero maximum number of in-flight operations. Groups do not nest in the
first API. The manual builder declares groups; the pipeline macro grammar stays
unchanged until that API has been exercised.

Group branches may consume outputs from before the group, or earlier outputs
from their own branch. They cannot consume sibling branch outputs. Such a
reference is a structured construction error even if its producer has a lower
node index. Nodes after a group may consume any of its outputs. Every ordinary
ownership, type, final-output, and producer-order check remains in force.

All operations outside groups remain ordered. A group is a barrier: the following
step cannot start until the group settles successfully. Missing value edges do
not grant permission to reorder work. Declared impacts describe effects and do
not establish resource independence; authors must explicitly choose independent
branches and manage shared external resources in operation implementations.

## Interpretations

`execute()` visits every node in flat declaration order and retains its current
strict fail-fast contract. An opt-in concurrent interpreter honors declared groups.
It runs ready branch heads, bounded by the group's limit; each branch preserves
its own sequence. It does not infer additional parallelism across group boundaries.
The scheduler polls ordinary futures and does not require a particular runtime,
spawn tasks, or imply CPU parallelism.

On the first observed execute failure, the concurrent interpreter stops starting
new operations. It drains already-running operations and collects their actual
outcomes before returning. No following group or sequential step starts. The
structured concurrent error retains the original operation/invariant errors,
completed node IDs, and never-started node IDs. Failure order is observation order;
completed/never-started lists use declaration order. Committed effects remain.
Dropping an operation future is not rollback, and a hanging operation can keep a
drain pending. Application cancellation is an independent, explicitly chosen act.

The opt-in concurrent dry-run interpretation uses the same declared structure but
retains policy-before-dependency resolution and continuation. Skip, deny, block,
and failure produce no value. Later independent allowed checks continue within
branches and after groups. Final report rows remain in declaration order even
when progress events and operation completion interleave. Existing sequential
dry-run methods remain available unchanged.

Input resolution is separated from the lifetime of running futures. Each ready
operation owns its resolved inputs before it is polled, allowing the coordinator
to store completed outputs while other operations remain in flight. Stores remain
private and per-run, with checked downcasts and no unsafe code. The listener is
called by the coordinator; it is not a scheduler or a control hook.

## Canonical Describe graph

An additive `describe_graph()` export supplies one owned graph DTO shared by CLI
JSON, server previews, and structural Mermaid rendering. The existing table
`PlanDescription` and its serialized row format remain supported.

Schema version 1 records plan name, optional application-supplied plan revision,
default sequential interpretation, node IDs/positions/names/impacts, explicit
value dependencies, application annotations, and the declared execution steps.
Sequence and parallel structure are distinct from value dependencies. A value
fan-out in an ordinary sequential plan must not be rendered as automatic
concurrency. A concurrent run may execute only the groups present in this graph.

Applications may attach static labels, environments, and approval-gate hints as
string annotations. The runtime does not hardcode CI/CD environment semantics.
Graph export never invokes bodies, resolves/clones inputs, touches a context or
store, or exports literal input values. It is metadata, not executable code.

Node IDs are plan-local. Applications bind run events and visual overlays using
run ID, application plan revision, and node ID. The supplied revision must identify
the relevant application code/build and configuration; a metadata hash cannot
identify opaque operation bodies. Approval subjects, artifact digests, deadlines,
decisions, outputs, statuses, and errors remain runtime data.

## Server demonstration

The local server declares independent dev and staging deploy/verify branches
that consume one tested candidate. A barrier joins both verified receipts before
requesting production approval. Production requires the real approval permit.
The same canonical graph draws these lanes before execution and supplies node
identity to the live overlay. Rehearsal skips the interactive gate and preserves
all ordinary safe-policy decisions. The server explicitly chooses the concurrent
interpretations; existing sequential CLI workflows keep their behavior.

## Verification

Required coverage includes static construction/description with no input cloning
or context access; serial fallback; group limits and genuine async overlap;
sequential branch order; barriers and fan-in; sibling dependency rejection;
stop-starting plus draining after failure; multiple structured errors without
`E: Clone`/`Display`; deterministic dry-run reports and independent continuation;
policy before dependencies; canonical JSON/annotations/revisions; and a production
gate that cannot open before both environment verifications succeed.

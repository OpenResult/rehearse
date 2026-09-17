# Office filing example

A small office inbox, a set of filing rules, and a reasonable amount of paperwork.
This standalone CLI demonstrates `rehearse` with real filesystem reads and moves.
It is a workspace member with `publish = false`.

## Try it

From the repository root:

```bash
# Create seven sample documents in a new directory.
cargo run -p rehearse-filing-example -- --seed-demo

# Describe the static plan. This works even before the inbox exists.
cargo run -p rehearse-filing-example -- --describe

# Assess the inbox and check destinations without changing files.
cargo run -p rehearse-filing-example

# File the documents and verify the result.
cargo run -p rehearse-filing-example -- --execute
```

The default office directory is `target/filing-office`, relative to your current
directory. Supply a positional path to use another location:

```bash
cargo run -p rehearse-filing-example -- /tmp/filing-office --seed-demo
cargo run -p rehearse-filing-example -- /tmp/filing-office --dry-run
cargo run -p rehearse-filing-example -- /tmp/filing-office --execute
```

`--seed-demo` requires a new directory whose parent exists. It refuses to reseed
an existing directory. Samples are plain-text placeholders, including those with
`.pdf` and `.docx` extensions. `--seed-demo`, `--describe`, `--dry-run`, and
`--execute` are mutually exclusive. Omitting a mode selects dry-run.

## Filing rules

Only files directly inside `inbox/` are assessed, in filename order. Matching is
case-sensitive and based on the filename prefix; contents and extensions do not
affect classification. Names are preserved.

| Filename | Destination under the office directory |
|---|---|
| `invoice-*` | `accounts/invoices/` |
| `expense-*` | `accounts/expenses/` |
| `minutes-*` | `administration/minutes/` |
| `report-*` | `management/reports/` |
| Anything else | `pending-review/` |

The sample inbox includes two invoices, an expense file, minutes, a report,
`document-final-FINAL.docx`, and `meeting-about-reducing-meetings.txt`.
The last two require manual review. They are moved to `pending-review/` during
execution, retaining their original names and contents.

## What the plan demonstrates

The operations and their value dependencies are declared in
[`src/filing.rs`](src/filing.rs). Filename classification happens inside a pure
operation. The plan always has seven steps; it does not create runtime graph
branches or a node for each document.

| Operation | Impact | Default dry-run |
|---|---|---|
| Scan inbox | Read | Runs |
| Determine filing locations | Pure | Runs |
| Check destinations | Read | Runs; reports existing-file conflicts |
| Create destination folders | Write | Skipped |
| Move documents | Write | Skipped |
| Check remaining inbox | Read | Runs independently of the moves |
| Verify filed documents | Read | Blocked; needs the move receipt |

A successful rehearsal prints the proposed destinations, followed by:

```text
7 documents assessed; 2 require manual review.
Inbox check: 7 documents remaining.
...
Dry-run incomplete: 4 executed, 2 skipped, 0 denied, 1 blocked, 0 failed.
```

Incomplete is expected: no move receipt exists until execution. The CLI uses
`require_no_failures()` so an ordinary dry-run exits successfully. A real read or
validation failure produces a nonzero exit status, while independent dry-run
work continues. Execute stops at the first error.

## Filesystem behavior

- Describe performs no filesystem access. Dry-run creates no directories and
  changes no document contents or locations.
- Execution checks the whole batch for existing destinations before creating
  folders or moving documents. Existing files, directories, and dangling
  symlinks at a destination are never replaced.
- A move creates a [hard link](https://doc.rust-lang.org/std/fs/fn.hard_link.html)
  at the destination, then removes the inbox entry. Creating the link fails if
  the destination already exists, including a conflict that appears after the
  earlier check. Both locations must be on a filesystem that supports hard links,
  and on the same filesystem.
- Subdirectories of `inbox/` are left alone. Symlinks and special files inside
  the inbox are rejected, as are symlinks used for the office, inbox, or filing
  folders. Use a local office directory that no other process changes during
  the run; the checks are not a security boundary against concurrent changes.
- The batch is not transactional. If execution fails, earlier moves remain.
  If removing an inbox entry fails after linking, both entries remain and the
  error reports that condition. There is no automatic rollback.
- Verification checks destination file types and sizes and confirms that the
  source entries are gone. It does not hash contents or guarantee crash durability.

The example uses synchronous filesystem calls inside async operation bodies,
on a single-thread Tokio runtime, to keep the filing workflow readable.

Run its filesystem and CLI integration tests with:

```bash
cargo test -p rehearse-filing-example
```

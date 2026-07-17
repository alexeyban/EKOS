---
name: memory
description: >-
  Global cross-project memory for Claude sessions, backed by the EKOS
  knowledge ledger. Use this at the START of any substantial task to restore
  context by searching the ledger instead of re-reading files, and DURING any
  session where you learn something worth keeping — a hard-won lesson, an
  effective approach, a decision with rationale, new material the user shared,
  or a completed piece of work. Triggers include: the user asking "have we/I
  done this before?", "how did I solve X?", starting work in any project the current
  workspace root, finishing significant work, being corrected,
  or discovering something non-obvious. This applies in EVERY project, not
  just EKOS — the memory is shared between all projects and sessions.
---

# EKOS as global memory

The EKOS ledger already contains a compiled index of all projects —
every file, README, CLAUDE.md, devlog, SQL schema, and git contributor —
plus the `memory/` notes written by previous sessions.

Treat it as your long-term memory:

**Read from it before re-deriving context, write to it whenever this
session produces knowledge a future session would otherwise need to
re-learn.**

## Workspace variables

Prefer variables instead of hardcoded paths:

```bash
WORKSPACE_ROOT="${WORKSPACE_ROOT:-.}"
MEMORY_DIR="${MEMORY_DIR:-$WORKSPACE_ROOT/memory}"
EKOS_ROOT="${EKOS_ROOT:-$WORKSPACE_ROOT/EKOS}"
EKOS_BIN="${EKOS_BIN:-$EKOS_ROOT/ekos/target/release/ekos}"
LEDGER_DB="${LEDGER_DB:-$WORKSPACE_ROOT/.ekos/ledger/ledger.db}"
```

## Reading: restore context from the ledger, not from files

Re-reading whole files burns context on content you don't need.
Search first, read second, and only read what the search points at.

1. Search by keywords:

```bash
ekos_search "<2-3 topic keywords>"
```

Searches names and content excerpts, ranked by relevance.

Use keywords, not questions:

Good:

```text
stale CKM
fabric deployment
dbt snapshots
```

Bad:

```text
have I seen a stale CKM before
```

Prefer 2–3 specific terms over one common word.

2. To list memory notes directly, use their markers via EKL:

```sql
FIND Object WHERE name CONTAINS '--lesson--'
```

Other markers:

* `--decision--`
* `--approach--`
* `--session--`
* `--reading--`

These notes are distilled by previous sessions and usually provide
the highest signal per token.

3. READMEs, CLAUDE.md files, devlogs, and other project artifacts are
   also indexed. Search for them directly:

```bash
ekos_search "devlog"
ekos_search "readme"
ekos_search "claude"
```

4. When filenames are insufficient, open the actual file returned by
   the search.

The ledger is the index.

Source files are the truth.

Never answer from filenames alone when content matters.

# Writing: the capture cycle

Capture knowledge whenever:

* you learned something non-obvious
* an approach proved effective
* an approach failed in an instructive way
* a decision was made with rationale
* the user shared important new information
* substantial work was completed

Write one file per fact under:

```bash
$MEMORY_DIR
```

## Naming convention

The filename is the retrieval key.

Use:

```text
<scope>--<type>--<keywords>.md
```

Where:

* scope = `global` or project name
* type =

  * `lesson`
  * `decision`
  * `approach`
  * `session`
  * `reading`

Examples:

```text
global--lesson--fabric-capacity-pause.md
ekos--decision--binary-ledger-format.md
customerA--session--migration-phase2-summary.md
```

## Body

A few lines are enough:

* what happened
* why it matters
* how to apply it
* references to code, files, links, tickets, or commits

## Before creating a note

Search first:

```bash
ekos_search "<keywords>"
```

Update existing notes instead of creating duplicates.

Delete or amend notes that are no longer correct.

Do not duplicate information already stored in:

* code
* README files
* CLAUDE.md
* devlogs

Memory notes should distill knowledge, not copy source material.

# Refresh: make notes searchable asynchronously

New notes are invisible until the ledger is rebuilt.

After writing notes, refresh asynchronously.

Batch several notes together.

Never block the session waiting for completion.

```bash
cd "$WORKSPACE_ROOT" && nohup sh -c '
"$EKOS_BIN" build &&
"$EKOS_BIN" recover &&
"$EKOS_BIN" compile &&
"$EKOS_BIN" commit
' >> .ekos/refresh.log 2>&1 &
```

The pipeline is incremental.

Unchanged projects are skipped through fingerprints, so refreshes
typically complete within seconds.

A note written in the current session is already available in the
current context.

Refreshing exists for future sessions only.

There is never a reason to wait for it.

# Division of labor

* This skill provides estate-wide shared memory across all projects.
* The `ekos-knowledge` skill provides retrieval mechanics, EKL
  examples, impact analysis, and advanced ledger querying.
* Symbol-level code questions should still use native file tools
  and LSP capabilities.

The ledger answers:

* what exists
* where it exists
* what was previously learned about it

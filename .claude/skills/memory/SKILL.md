---
name: memory
description: >-
  Global cross-project memory for Claude sessions, backed by the EKOS
  knowledge ledger. Use this at the START of any substantial task to restore
  context by searching the ledger instead of re-reading files, and DURING any
  session where you learn something worth keeping — a hard-won lesson, an
  effective approach, a decision with rationale, new material the user shared,
  or a completed piece of work. Triggers include: the user asking "have we/I
  done this before?", "how did I solve X?", starting work in any project under
  /home/legion/PycharmProjects, finishing significant work, being corrected,
  or discovering something non-obvious. This applies in EVERY project, not
  just EKOS — the memory is shared between all projects and sessions.
---

# EKOS as global memory

The EKOS ledger already contains a compiled index of all 44 projects — every
file, README, CLAUDE.md, devlog, SQL schema, and git contributor — plus the
`memory/` notes written by previous sessions. Treat it as your long-term
memory: **read from it before re-deriving context, write to it whenever this
session produces knowledge a future session would otherwise re-earn.**

## Reading: restore context from the ledger, not from files

Re-reading whole files burns context on content you don't need. Search first,
read second, and only what the search pointed at:

1. `ekos_search "<2–3 topic keywords>"` — finds files and notes by name
   across every project. **Use multiple terms** (`"mcp stdio jsonrpc"`, not
   `"mcp"`): results cap at 20, and single common words get drowned by
   project files before memory notes surface.
2. To list memory notes directly, use their name markers via EKL:
   `FIND Object WHERE name CONTAINS '--lesson--'` (likewise `--decision--`,
   `--approach--`, `--session--`, `--reading--`, or a `<project>--` prefix).
   Notes are distilled by past sessions — highest signal per token, read
   these first.
3. `readme`, `claude.md`, `devlog` files of any project are in the ledger too —
   search `ekos_search "devlog"` or `"<project> readme"` to locate them.
4. **When the located names aren't enough, open the actual file** with Read
   at the path the search returned — the ledger is the index, source files
   are the truth. Never answer from a filename alone when content matters.

## Writing: the capture cycle

When any of these happen, capture it — don't wait to be asked:

- you learned something non-obvious (a gotcha, an SDK quirk, a root cause)
- an approach proved effective (or clearly failed — record that too)
- a decision was made with rationale worth remembering
- the user shared or you read new material worth keeping
- a substantial piece of work completed (session summary: what + where + why)

Write one file per fact to `/home/legion/PycharmProjects/memory/`:

- **Filename carries the search terms** — ledger full-text search covers
  names, not file contents, so the slug is the retrieval key:
  `<scope>--<type>--<keyword-rich-slug>.md`
  (scope: `global` or project name; type: `lesson` | `decision` | `approach`
  | `session` | `reading`).
- **Body** (a few lines is enough): the fact, why it matters, how to apply
  it, and paths/links to the full source.
- **Search before writing** (`ekos_search` on your keywords) — update an
  existing note rather than creating a near-duplicate; delete notes proven
  wrong.

Don't duplicate what's already compiled: devlogs, READMEs, CLAUDE.md files
and code are in the ledger — a note should *distill or point*, not copy.

## Refresh: make it searchable, asynchronously

New notes are invisible to the ledger until recompiled. After writing (batch
several notes into one refresh; end of task is a good moment), fire and
forget — never block the session on it:

```bash
cd /home/legion/PycharmProjects && nohup sh -c \
  'EKOS=EKOS/ekos/target/release/ekos; $EKOS build && $EKOS recover && $EKOS compile && $EKOS commit' \
  >> .ekos/refresh.log 2>&1 &
```

The pipeline is incremental (unchanged projects skip via fingerprints), so a
refresh after a few notes takes ~seconds of real work. A note you just wrote
this session is already in your context — the refresh is for *future*
sessions, so there is never a reason to wait for it.

## Division of labor

- This skill = the estate-wide memory shared between all projects and models.
- The `ekos-knowledge` skill covers the query tools in depth (EKL cheat
  sheet, impact analysis, diffs) — lean on it for retrieval mechanics.
- Symbol-level questions about code you have open stay with your native
  file/LSP tools; the ledger answers *what exists, where, and what we learned
  about it*.

# Devlog 198 — Memory: retire the estate skill, make staleness reach the answer (RFC 0151)

**Date:** 2026-09-21
**PRs:** none (local `main`, `[skip ci]`)
**Branch:** main

---

## Summary
Three overlapping memory systems existed; only one was alive. Retired the broken estate `memory`
skill to a routing stub, rewrote the `session-memory` skill, made RFC 0151's staleness marker
actually reach the model's answer, and re-ran the eval at 121 notes. The staleness fix is
confirmed (0 of 24 answers flagged before → 5 of 6 after). The correctness win is real but belongs
to *scoped retrieval*: an unscoped session-start brief scored identically to having no memory.

---

## The estate `memory` skill was broken, and it misled a live session

Its `WORKSPACE_ROOT` loop stopped at the first ancestor holding `ekos.toml` — the EKOS repo itself
— so `MEMORY_DIR` resolved to a directory that does not exist, and a session reported "no memory
notes exist" while 9 notes sat one level up at `/home/legion/PycharmProjects/memory/`, behind an
`ekos.toml` the loop can never reach. Three further defects: `ekos_search "…"` in bash fences ×5
(it is MCP-only — no binary, no subcommand), `LEDGER_DB` on the v2 SQLite path when the workspace
is the v3 fact engine, and a refresh pipeline missing `resolve`.

Retired to a routing stub rather than deleted, because `/memory` is invoked by name. The stub's
description is narrowed to explicit invocation so it stops competing for automatic attention. The
9 legacy notes are left on disk untouched.

## Making the staleness marker reach the answer — `session/src/read.rs`

The 2026-09-21 live eval found the `[CHANGED]` marker produced a staleness flag in **0 of 24**
answers; the apparent 0.67 "stale-served" score came entirely from `NONE` refusals. Four changes:

| change | why |
|---|---|
| directives moved **above** the `untrusted` envelope, and made imperative | the envelope described the marker but never said what to do with it. Imperatives inside an "inert data" region would also hand a note author a shape to imitate |
| directive scoped to the marker, plus "never refuse over a marker" | both observed failure modes: ignore it, or refuse and lose the fact |
| print the verdict only for `CHANGED`/`ORPHANED`, the tier only for `T1` | every line carried `[T0 unconfirmed agent note] [FRESH]`; the one label needing action was buried in noise |
| scope overlap ranked above verdict; scope matches on basename too | a note about the file you are editing lost to any unrelated fresh note. `--scope-from-git` emits paths while anchors are object names, so exact equality left the key at 0 |

Also fixed a pre-existing bug: the truncation line and pending block were appended *after* the
budget check, so a brief overran its own `budget_tokens`. Both are now accounted for before the
loop, and the truncation line reports how many hidden notes were `CHANGED`.

## Eval at 121 notes

| condition (400-token budget, haiku) | correct | stale served |
|---|---|---|
| model-written summary, same budget | 0.16 | refused all |
| brief, no scope | 0.16 | refused all |
| brief, scoped | 1.00 | 1.00 |
| brief, scoped, anchors changed | 1.00 | 0.17 |

`docs/evals/session-continuity-scale-2026-09-21.md`.

---

## Knowledge Captured

- **A directive is not a description.** Telling the model what a marker *means* changed nothing (0
  of 24). Telling it what to *do* — answer, say it may be stale, never refuse over a marker —
  moved it to 5 of 6. The same applies to any label injected into context.
- **Labels on every line destroy the one label that matters.** `[T0 unconfirmed] [FRESH]` on all
  lines made `[CHANGED]` invisible. Print only departures from the default and state the default
  once.
- **The win was scope, not memory.** An unscoped brief at 121 notes scored 0.16 — identical to no
  memory. A 400-token brief holds ~12 of 121 notes and without a scope cannot know which 12. What
  the eval validates is scoped retrieval, not a session-start brief. Do not let the headline
  number hide that.
- **Two self-inflicted eval bugs, both of which flattered the feature.** (1) A 572-call run was
  discarded: the fixture keyed the note template on the note index instead of the row, so all 3
  notes per table shared a template with different values — every question had 3 contradictory
  answers and any answer listing them all scored correct. (2) The earlier grader counted
  "unconfirmed"/"unverified" as staleness flags, words the `T0` label made the model emit
  regardless. Check whether a metric can be satisfied by something other than the effect being
  measured.
- **Budget-match the conditions.** The 7-note run gave compaction ~56 tokens and the EKOS brief
  ~317 and still only tied — a worse result than the table conveyed. An unmatched budget makes a
  comparison meaningless in whichever direction it points.
- **A fixture of arbitrary numbers is adversarial to summarisation.** 121 unique 4-digit values
  are maximally incompressible and individually required. Real notes are redundant. The result is
  real for that regime and must be quoted with it.
- **Enabling a feature in `ekos.toml` is not enough to expose its MCP tools.** The server runs
  `target/release/ekos` per `~/.claude.json`; that binary predated RFC 0151 and answered
  `unrecognized subcommand 'session'`. Config + `cargo build --release` + reconnect.
- **Skill edits do not take effect until a new session** — descriptions are loaded at startup.

## Files Changed
| File | Change summary |
|---|---|
| `.claude/skills/memory/SKILL.md` | retired to a routing stub |
| `.claude/skills/session-memory/SKILL.md` | rewritten: pushier description, commit step, write triggers, good/bad examples |
| `ekos/crates/session/src/read.rs` | directive envelope, ranking, label rendering, budget + truncation honesty |
| `ekos/crates/session/src/eval.rs` | `poisoned_leak` re-expressed off the tier string |
| `ekos/crates/session/tests/pipeline.rs` | 2 new tests; envelope + budget assertions retuned |
| `ekos/crates/cli/src/commands/mcp.rs` | tool descriptions match the new labels and ranking |
| `ekos.toml` | `[session-memory] enabled = true` |
| `demo/session-memory/live_eval.py` | budget-matched, 121 notes, scoped condition, refusal/over-hedge metrics, fixture fix |
| `demo/session-memory/transcript.txt` | regenerated |
| `docs/evals/session-continuity-scale-2026-09-21.{md,json}` | new report + raw answers |
| `docs/session-memory.md`, `docs/session-memory-comms-checklist.md`, `ekos/docs/rfcs/0151-…` | updated |

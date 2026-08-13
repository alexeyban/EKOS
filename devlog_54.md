# Devlog 54 — RFC 0054: Event Store closure, Simulation Replay, and a real ledger bug fix

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Ninth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
RFC 0052 (Conflict Resolution), RFC 0053 (Virtual Social Environment), and now RFC 0054 — the
user's chosen path: close Phase 12 (Event Store) honestly, then build Phase 13 (Simulation Replay)
on top of it. Phase 12 turned out to be almost entirely already satisfied by the existing
`ActionExecuted` event shape; the one real gap (`observed_by`) closed as a derived query over
existing `Knows` edges, not new storage. Phase 13's `Replay` needed exactly one new mechanism (a
durable, per-ledger event log) to answer a question nothing could answer before: "what happened in
round N," after the process that ran it has exited. Writing `Replay`'s own test then surfaced a
real, pre-existing correctness bug in the SQLite and fact-engine ledger backends alike —
`relationships_at` silently returned the *current* version of a relationship instead of the
historically correct one whenever that relationship had been updated more than once. Fixed at the
root in both backends, not worked around.

---

## RFC 0054 — Event Store (Phase 12) and Simulation Replay (Phase 13)

### Problem / motivation

Checked before designing, same discipline as the prior eight RFCs:

- The source document's own Phase 12 example event (`id`, `round`, `timestamp`, `actor`, `action`,
  `target`, `content`, `observed_by`) compared field-by-field against `execute_action`'s existing
  `Custom("ActionExecuted")` payload (RFC 0050/0052/0053) — matched on everything except
  `observed_by`.
- `observed_by` isn't missing *data* — every observer already gets a `Knows` edge pointing at the
  event (RFC 0050's `observers_for`/`append_knows`). Baking a redundant list into the event's own
  payload would duplicate what the edges already represent — the same "derive, don't duplicate"
  call RFC 0053 made for `PostedIn`.
- Phase 13's real prerequisite, checked concretely rather than assumed: `RoundResult` exists only
  in memory, gone the moment the process that produced it exits. There was no way to reconstruct
  "what happened in round N" from a ledger alone after the fact — that gap, not `observed_by`, was
  Phase 12's one genuinely missing piece, and Phase 13 depends on it directly.
- `KnowledgeStore` still has no bulk "every event" query (the same finding RFC 0053 made for
  channels) — so the durable log needed the same kind of relationship index RFC 0053 built for
  `PostedIn`, generalized to every executed action.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0054-event-store-and-replay.md` |
| `SimulationLog` indexing (`Custom("LoggedIn")`) for every executed action | `crates/simulation/src/simulation.rs` |
| `observed_by` | Same file |
| `Replay` (`open`/`rounds`/`events_in_round`/`jump_to`/`inspect_agent`/`inspect_graph`/`observed_by`) | `crates/simulation/src/replay.rs` |
| `ekos replay <scenario.yaml> [--round N]` | `crates/cli/src/commands/replay.rs` |
| **`relationships_at` multi-version fix** | `crates/ledger/src/lib.rs`, `crates/ledger/src/fact_ledger.rs` |

**Deliberately not built**, per the RFC's own Non-goals: an interactive stepping session
(`start`/`pause`/`next round` as literal REPL commands — `Replay`'s methods are the stateless
primitives such a UI would be built from, not the UI itself); `observed_by` baked into the event
payload; metrics, turning-point detection, or report generation (Phases 14-16 — real, separate,
unscoped forks); video/report rendering of a replay.

### The bug `Replay`'s own test found, in both ledger backends

Writing `inspect_graph`'s historical-reconstruction test — using `FormAlliance`'s `Trusts` bump,
updated across two separate rounds — produced a wrong answer on the first run: round 0's own
pre-round snapshot showed nothing at all for the relationship it should have shown at 0.5. Traced
to `Ledger::relationships_at` (SQLite): unlike `object_at`, which correctly scans every version of
an object and picks the latest one at-or-before the query time, `relationships_at` joined straight
against `current_relationships` — a pointer table that only ever tracks the *current* version of
each relationship id. A relationship updated after the query time was excluded entirely, rather
than falling back to the version that actually existed at that point. This was a **known, already-
documented limitation** (RFC 0011, referenced in RFC 0047's own devlog as a standing gap) — but
Replay is the first feature in this continuation whose entire value proposition depends on getting
exactly this right, so it surfaced immediately instead of staying latent.

`FactLedger`'s own `relationships_at` had the identical bug, independently implemented — and
arguably worse: it checked visibility against a relationship's *latest* transaction, then, if
visible, reconstructed state at `TxId(u64::MAX)` (always current), regardless of the requested
`at`. Its own doc comment said "kept for parity" with the SQLite limitation — a deliberate choice
at the time, now revisited because Replay needs the actual guarantee, not just backend parity.

Both fixed to mirror `object_at`'s already-correct pattern: find the *set* of relationship ids
ever involving the queried entity (stable across versions, since a relationship's `from`/`to`
never change — only its properties do), then for each one, independently reconstruct its state at
the query cut. Regression tests added directly to `ekos-ledger` (not just exercised indirectly
through `Replay`), since every caller of `relationships_at`/`build_world`/`agent_observation` — not
just this RFC — benefits from the fix. Full ledger and workspace test suites confirmed no other
behavior regressed (88 ledger tests, 91 workspace-wide `test result: ok` blocks, all clean).

### `jump_to`'s real semantics — and why my own first test assumed the wrong one

A second, smaller finding while building the same test: `jump_to(round)` resolves to that round's
*pre-round* snapshot (`occurred_at`, captured before the round's own actions execute — RFC 0050's
own invariant that no agent's decision can see another's same-round action), not a *post-round* one.
My own first draft of the historical-reconstruction test assumed the opposite and failed for a
different reason before the ledger bug was even found. Once corrected, this turned out to be the
more useful semantic anyway — "what did agents actually see deciding this round" — and required no
implementation change, only a corrected test and a doc-comment clarification on `jump_to`/
`inspect_agent`/`inspect_graph` so a future reader doesn't make the same assumption.

### Decisions (alternatives considered, why this choice)

- **Baking `observed_by` into the event payload at creation time** — rejected; duplicates the
  `Knows` edges that already represent exactly this fact, with a real staleness risk for no
  benefit.
- **A per-round marker object instead of one flat per-ledger log** — rejected; `payload["round"]`
  already identifies which round an event belongs to; a second index expressing the same
  information would be redundant. `Replay::rounds()`/`events_in_round()` filter the one flat log
  client-side.
- **An interactive replay REPL in this RFC** — rejected; would be built on primitives that didn't
  exist before this RFC, with no concrete UI consumer yet to validate the design against — the same
  "designed but not exercised" risk this session has avoided eight times running.
- **Leaving `relationships_at`'s known limitation as-is and working around it in `Replay` alone** —
  rejected once the actual scope of the bug was understood: it affects every caller of
  `build_world`/`agent_observation` with an `at` parameter, not just `Replay`. A workaround
  localized to this RFC would have left the same wrong answer available to every other caller.

---

## Knowledge Captured

- **A "known, documented limitation" is still a bug — it just hasn't met the feature that needs it
  fixed yet.** RFC 0011's `relationships_at` gap sat undisturbed through RFC 0047-0053 because
  nothing in those RFCs' own tests happened to query a multiply-updated relationship at a
  historical point in time. The first feature whose core purpose requires exactly that (`Replay`)
  found it immediately — worth remembering that "documented and accepted" isn't the same as "safe
  to keep relying on the surrounding correct-looking behavior."
- **When two backends implement the "same" query independently, check both for the same class of
  bug, not just the one that failed first.** `FactLedger`'s version wasn't merely parallel to
  SQLite's — its own doc comment said so explicitly ("kept for parity") — but it had gone further
  wrong (reconstructing at `TxId(u64::MAX)` unconditionally) than a first glance at "does the same
  thing" would suggest.
- **A single relationship id's `from`/`to` endpoints are a reliable historical index even when its
  *properties* have many versions** — this is what made the fix straightforward: enumerate ids from
  the stable pointer table, then resolve each one's state independently and correctly, rather than
  trying to make the pointer-table join itself time-aware.
- **A round's "pre-round" vs. "post-round" snapshot is a real semantic choice, not an obvious
  default** — worth stating explicitly in any future point-in-time query API, since both are
  legitimate answers to "state at round N" and only one of them is what `Simulation::run_round`
  itself actually reasons about.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0054-event-store-and-replay.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/ledger/src/lib.rs` | **Real bug fix**: `relationships_at` rewritten for true multi-version reconstruction; 1 new regression test |
| `ekos/crates/ledger/src/fact_ledger.rs` | Same fix, `FactLedger`; 1 new regression test |
| `ekos/crates/simulation/src/simulation.rs` | `find_log_object`/`find_or_create_log`; `observed_by`; log indexing wired into `execute_action`; 2 new tests |
| `ekos/crates/simulation/src/replay.rs` | New: `Replay`, `ReplayError`; 4 unit tests |
| `ekos/crates/simulation/src/lib.rs` | `pub mod replay;` + re-exports |
| `ekos/crates/simulation/tests/replay_fixture.rs` | New: multi-round simulate-then-replay, both ledger backends |
| `ekos/crates/cli/src/commands/simulate.rs` | `scenario_ledger_path` made `pub(crate)`, shared with `replay` |
| `ekos/crates/cli/src/commands/replay.rs` | New CLI command |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod replay;` |
| `ekos/crates/cli/src/bin/ekos.rs` | New `Commands::Replay` variant + dispatch |

## Still open (tracked, not silently dropped)

- **Whether to continue further** — `world.sources` document ingestion remains the one named,
  still-unscoped fork from several devlogs back; Phase 14+ (Metrics, Turning Point Detection,
  Report Generation, and beyond) haven't been scoped at all.
- **No interactive replay session** — `Replay`'s primitives exist; a stepping UI on top of them is
  real, deferred work.
- **No scenario-YAML seeding of forum/channel state ahead of round 0** — unrelated gap, carried
  over from RFC 0053.

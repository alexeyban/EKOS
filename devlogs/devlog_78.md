# Devlog 78 — RFC 0068 Increment 7: closing Data Architecture's cross-referencing gaps

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Seventh increment, closing the four concrete follow-ons RFC 0074 (Increment 6) explicitly
surfaced rather than papered over. Shipped real code for two (Table↔TransformNode linking, Data
Domains); for the other two (Ownership, Lifecycle), investigation found and corrected a real
factual error in RFC 0074's own text, and turned a vague "not yet computed" into a precise,
concretely-scoped blocker. Data Quality was checked and confirmed genuinely out of reach without
Phase 3 runtime telemetry, not under-investigated.

---

## RFC 0075 — Data Architecture Cross-Referencing

### Table↔TransformNode linking

New `ekos-semantic::data_lineage::link_transform_nodes_to_tables`, run from `commit.rs` the same
way `commit_rollups` (RFC 0044) already runs a whole-graph pass right before the final ledger
write. Matches `TransformNode` Source/Sink nodes to real `Table`/`Dataset` objects by
`object_name`, case-insensitively, **only when the match is unambiguous** — exactly one table with
that normalized name. Two schemas both having a `customers` table is real and common; guessing
would fabricate a false lineage edge, so ambiguous or absent matches are skipped, not guessed at.
New relationship kinds `ReadsFrom`/`WritesTo` get **deterministic ids from the start**
(`reads_writes_kir_id`, matching RFC 0072's `depends_on_kir_id` pattern) — this shape has the exact
"boolean fact per pair" property RFC 0072 proved safe to dedupe this way, so there was no reason to
ship it with a random id and rediscover the same duplication bug a third time.

Live-verified: this repo's own real ledger has zero SQL content, and neither committed fixture has
transformation SQL (only DDL), so built a small disposable fixture (2 tables, 2 views) and ran the
real pipeline. Real result: `Data lineage links: 3` on first commit, `Relationships written: 0` (no
"Data lineage links" line at all) on a second commit against unchanged input — confirmed
idempotent, not accumulating duplicates.

Found a second real bug while wiring this into `docs-gen`: the "has any transformation been
compiled" check was keyed on `FeedsInto`-edge presence, which is false for a real, legitimate
single-node transformation (a bare `SELECT * FROM x`, one `Source` node, no downstream step, hence
zero `FeedsInto` edges). Fixed to check for compiled `TransformNode` objects directly. Caught by
this increment's own new test fixture — a lone Source node — failing an assertion it should have
passed.

### Data Domains

`Table`/`Dataset` names already carry a real schema qualifier whenever the source DDL wrote one
(`sales.orders`). `data_domains_section` groups by that qualifier — zero new extraction, just
reusing structure already in the compiled name. Both this repo's own committed fixtures use
unqualified names, so the honest-empty-state path is what's live-verified against real fixture
data; the grouping logic itself is unit-tested against synthetic qualified names.

### A real correction: RFC 0074's Ownership text was wrong

Investigating whether Ownership was closable this increment, re-read `git_analyzer.rs` directly
rather than trusting RFC 0074's own earlier description. Found `git_analyzer.rs` never emits
`ObjectKind::File` objects at all, and its only `OwnedBy` edge connects a **commit event** to the
**contributor who authored it** — never a file, never a table. RFC 0074 had stated `OwnedBy` edges
land "onto observed `File` objects" — factually wrong, a real conflation of two unrelated real
primitives (`OwnedBy` exists; `File` objects exist; they aren't connected) into one incorrect
sentence. Corrected in the rendered Data Architecture text and TODO.md, named explicitly as a
correction rather than silently fixed — a wrong claim about what's actually compiled is precisely
the failure mode this project's whole evidence-traceability discipline exists to prevent, so it
gets called out the same way a code bug would.

The corrected, precise blocker: Ownership and Lifecycle both need a `Table`→`File` link that
doesn't exist yet, and Ownership additionally needs `git_analyzer.rs` to derive real per-file
ownership (it only has commit-event-level data today) — two concrete, scoped pieces of real future
work, recorded as such, not implemented this increment.

### Data Quality — confirmed, not assumed, out of reach

Checked whether DDL-level `NOT NULL`/constraint metadata could stand in for a data-quality signal.
Deliberately didn't use it: a structural constraint is a stated rule, not a measurement of actual
data — RFC 0068 §26 itself draws exactly this requirement-vs-observation distinction. Genuinely
needs runtime data profiling, explicitly RFC 0068 §63 Phase 3 scope. Confirmed out of reach by
checking, not left unchecked.

---

## Knowledge Captured

- **Deterministic ids should be the default for any new relationship-emitting site with the
  "boolean fact per pair" shape, not something retrofitted after a bug report.** RFC 0072 spent a
  whole increment finding and fixing this after two independent live occurrences; this increment's
  new `ReadsFrom`/`WritesTo` edges got it right immediately because the pattern was already
  identified and named. Worth checking this shape explicitly for the other 134 `KirRelationship::
  new()` call sites RFC 0072 left untouched, not just waiting for the next live occurrence.
- **A summary of code behavior is a claim, and claims need to be checked against the actual code
  before being built on, not just repeated.** RFC 0074's Ownership text was wrong not because
  anyone fabricated anything, but because two separate real facts (`OwnedBy` exists;
  `git_analyzer.rs` observes commits and authors) got compressed into a plausible-sounding but
  incorrect sentence about what's *connected*. Investigating a "close this gap" request is the
  right moment to re-verify prior claims, not just extend them.
- **An honest-empty-state branch is exactly where an off-by-a-concept bug hides.** The
  `is_feeds_into`-only gate for "any transformation compiled" had been in place since RFC 0071 and
  passed every test written against it — because every fixture used happened to have 2+ connected
  nodes. A one-node fixture, deliberately written to test the *new* linking feature, exposed a
  pre-existing gap in an *older* feature as a side effect of exercising a genuinely different edge
  case.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0075-data-architecture-cross-referencing.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Status note for this increment |
| `ekos/crates/semantic/src/data_lineage.rs` | New module: `link_transform_nodes_to_tables`; 8 tests |
| `ekos/crates/semantic/src/lib.rs` | `pub mod data_lineage;` |
| `ekos/crates/cli/src/commands/commit.rs` | `commit_data_lineage`, wired into `run()` |
| `ekos/crates/docs-gen/src/lib.rs` | Read/write counts per store; `has_transform_nodes` bug fix; `data_domains_section`; corrected Ownership text; sharpened Lifecycle/Data Quality text; 9 new tests |
| `TODO.md` | Follow-on items closed/re-scoped with the corrected blocker |
| `devlogs/devlog_78.md` | This file |

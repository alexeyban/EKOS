# Devlog 73 — RFC 0068 Increment 2: Component View + Technology Inventory, and a real ledger bug

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Second increment of continuous, automatic build-out against RFC 0068. Resolved the Crate↔File
design question Increment 1 (RFC 0069) deliberately deferred, by reusing existing RFC 0044
infrastructure instead of building anything new. Live verification surfaced a real, previously
unknown ledger-layer bug — relationship ids aren't deterministic, so identical relationships
accumulate as real duplicates across repeated commits — fixed locally where this increment
touches it, tracked honestly as separate, larger work where it doesn't.

---

## RFC 0070 — Component View + Technology Inventory

### What was built

| Component | What it does |
|---|---|
| `render_component_view` (`docs-gen`) | Links each `Crate` to its matching `Rollup` subsystem page, by exact directory match |
| `## Technology Inventory` | Promoted from `## Technologies`; each technology now links to its own real detail page |
| Deduplicated `dependents` list | Local fix for the real relationship-duplication bug found live |

### The design question, resolved by finding existing infrastructure

Increment 1 found `Crate` and `File`/`RustSymbol` objects aren't linked in the graph and deferred
Component View rather than rush an answer. This increment found the real answer already existed:
RFC 0044's `synthesize_rollups` (crate-level directory grouping, already producing a `Rollup` per
crate-sized subtree with real member counts) uses the exact same path convention
`CrateTopologyAnalyzerPass` uses for `Crate.path`. Confirmed against this repo's own real compiled
data before relying on it — `ekos ekl "FIND Object WHERE name CONTAINS 'ekos-kir'"` showed a real
`Rollup` named literally `"ekos/crates/kir"`, matching what `Crate.path` for the same crate would
be. Zero new extraction needed; `render_component_view` just matches by exact name equality.

### A real bug, found live, not invented to fill a feature slot

Live-verifying Technology Inventory against this repo's own real ledger showed something wrong
immediately: `serde_json`'s "used by" list repeated the same ~30 crate names 3-4 times in a row.
Read the code rather than guessed: `KirRelationship::new()` (used by essentially every
relationship-emitting pass, including `crate_topology_analyzer.rs`) mints a fresh random `KirId`
every call — unlike `KirObject`, where most emitting passes (including this same analyzer, for its
`Crate`/`Technology` objects) set a deterministic id. `append_relationship`'s `(id,
content_signature)` versioning — the exact mechanism that already correctly dedupes identical
`KirObject` re-writes across repeated `recover` runs (confirmed by reading `append_versioned`
during RFC 0069's drift work) — keys on `rel.id`. A random id means a logically identical
`DependsOn` edge, re-derived by a later `recover`/`commit`, never matches what's already in the
ledger: a real, unbounded duplicate every time. This repo's own ledger has been recommitted many
times this session (Phase 1, Phase 1 verification, RFC 0067's several `investigate` runs) — the
duplication in the real output is direct, visible proof, not a hypothetical.

**Handled honestly, not swept under either extreme.** Two bad options were available: silently
ship a view that surfaces obviously wrong-looking duplicated data (dishonest to the reader), or
quietly expand this increment into an unplanned, unscoped audit of every `KirRelationship::new()`
call site across every recovery-crate analyzer pass (real scope creep, and a data-model decision —
change what "the same relationship" means at the ledger layer — that deserves its own RFC, not a
rushed fix buried inside a docs-gen rendering increment). Did the third thing: fixed the one view
this increment actually touches (dedup the `dependents` list, with a real regression test
reproducing the non-deterministic-id shape directly), and wrote the real root cause down as its
own tracked TODO.md item with both real fix options named, neither picked yet — because picking
between "give every relationship a deterministic id" and "change the ledger's dedup key" has real
downstream implications (the latter, for instance, touches `relationship_history`'s RFC 0047
point-in-time semantics) that deserve their own investigation, not a decision made in passing.

---

## Live verification — no new pipeline run needed

Same pattern as Increment 1: this repo's own real, already-committed ledger already had everything
needed to verify both new views for real. `ekos docs generate --layout curated` rendered:

- A real `## Component View` — all 44 real crates, most linked to their real subsystem page with
  real member-file counts (`ekos-kir` → 2 member files, `ekos-ledger` → 9, etc.).
- A real `## Technology Inventory` — every real external dependency, linked to its own detail
  page, "used by" lists now genuinely deduplicated (`serde_json` — 33 real distinct crates, not
  132 duplicated entries).

---

## Knowledge Captured

- **The answer to "how do I link these two kinds of objects" is sometimes "they're already
  linkable, by a convention two different passes both already happen to follow" — check before
  building anything new.** Confirmed via real compiled data, not just reading source in isolation:
  a `Rollup` named `"ekos/crates/kir"` and a `Crate` with `path = "ekos/crates/kir"` in the *same*
  real ledger is stronger evidence than reading both functions' path-derivation logic and assuming
  they'd agree.
- **A live-verification step that surfaces something wrong is doing its job — the response is to
  fix what's in scope and honestly track what isn't, not to expand scope to "fix everything" or
  shrink it to "ignore what I found."** The relationship-duplication bug is real, was previously
  unknown, and is bigger than this increment; recording it precisely (root cause, what's mitigated,
  what's still exposed, real fix options with their own tradeoffs) is what makes it findable and
  actionable next time, instead of being quietly forgotten the moment this session ends.
- **The exact mechanism that makes an append-only ledger safe for objects (deterministic ids +
  content-signature versioning) silently doesn't apply to relationships unless something explicitly
  gives them a deterministic id too** — worth remembering for any *new* relationship-emitting code:
  match the `Crate`/`Technology`/`Claim`/`ArchitectureGap` pattern (explicit deterministic id), not
  the bare `KirRelationship::new()` default.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0070-component-view-and-technology-inventory.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Increment 2 status note |
| `ekos/crates/docs-gen/src/lib.rs` | New `## Component View` + `render_component_view`; `## Technologies` → `## Technology Inventory` with links + dedup; 4 new/updated tests |
| `TODO.md` | RFC 0068 §61 MVP items ticked off; new relationship-id-non-determinism item tracked; next increment scoped |
| `devlogs/devlog_73.md` | This file |

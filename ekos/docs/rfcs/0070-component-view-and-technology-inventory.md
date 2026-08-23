# RFC 0070 — Component View + Technology Inventory

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

**Increment 2** of RFC 0068's continuous build-out (Increment 1: RFC 0069, System Context +
documentation drift). This increment: Basic Component View (RFC 0068 §18) and a dedicated
Technology Inventory view (§61 MVP), the two items explicitly deferred from Increment 1 pending a
real design decision.

## Design

### The Crate↔File design question, resolved

Increment 1 found `Custom("Crate")` (RFC 0042) and `File`/`RustSymbol` (RFC 0041) objects aren't
linked in the graph — `Contains` edges run File→RustSymbol, never Crate→File — and deferred
Component View rather than rush a shallow answer. Investigating further this increment found the
real answer already exists: RFC 0044's `synthesize_rollups` already groups `File` objects by
directory prefix at depth 3 (`ekos/crates/kir/src/lib.rs` → `ekos/crates/kir` — crate-level,
matching a Cargo workspace's real structure), producing a `Rollup` object whose `name` is exactly
that directory path. `CrateTopologyAnalyzerPass` computes `Crate.path` the same way (manifest
directory relative to wherever `recover` was invoked from). Both use the identical path
convention — confirmed against this repo's own real compiled data (`Crate` named `ekos-kir` with
`path = "ekos/crates/kir"`, a real `Rollup` named exactly `"ekos/crates/kir"`) before relying on
the match, not assumed.

So: no new extraction, no new relationship. `docs-gen::render_component_view` matches each `Crate`
to a `Rollup` by exact `rollup.name == crate.path` and links through to that rollup's existing
detail page (real member-file count, boundary relationships — RFC 0044's existing output). A crate
with no matching rollup (RFC 0044's own ≥2-member threshold means many real small crates
legitimately have none) is silently skipped, not reported as a gap — that would be inventing
signal that isn't there.

### Technology Inventory

Promoted the existing `## Technologies` bullet list (Phase 1) to `## Technology Inventory`,
framed against RFC 0068 §61's own naming, with each technology now linked to its own real detail
page (`Technology` was already an `is_entity_page_kind`, just never linked from here).

## A real bug found live, fixed where it surfaces, tracked where it doesn't

Live-verifying Technology Inventory against this repo's own real, already-committed ledger
surfaced genuinely duplicated "used by" lists — the same ~30 crate names repeated 3-4 times in a
row for widely-used dependencies like `serde_json`. Root cause, confirmed by reading the code, not
guessed: `KirRelationship::new()` mints a fresh random `KirId` every call (unlike `KirObject`,
which most emitting passes give a deterministic id). `append_relationship`'s `(id,
content_signature)` versioning (the same mechanism that correctly deduplicates identical `KirObject`
re-writes, RFC 0015) keys on `rel.id` — so a logically identical `DependsOn` edge re-derived by a
later `recover`/`commit` never matches the id of the one already in the ledger, and a real,
unbounded duplicate accumulates every time the same data is recommitted. This repo's own ledger has
been recommitted many times this session (Phase 1, Phase 1 verification, RFC 0067's several
`investigate` runs) — real, visible proof of the bug, not a contrived scenario.

**Fixed where this increment surfaces it**: `render_architecture`'s Technology Inventory now
deduplicates the `dependents` list by name before rendering (sort + dedup) — a real regression test
(`architecture_technology_inventory_deduplicates_repeated_dependent_relationships`) reproduces the
non-deterministic-id shape directly and proves the view stays honest regardless.

**Deliberately not fixed everywhere**: the root cause (`KirRelationship::new()`'s non-deterministic
ids) is a ledger/commit-layer concern affecting `all_relationships()` broadly — the Crate topology
Mermaid diagram, MCP tools, EKL queries, and any other relationship-reading code likely share the
same exposure to varying degrees, and fixing it properly means auditing every `KirRelationship::new()`
call site across every recovery-crate analyzer pass (or changing `append_relationship`'s dedup key
itself) — real, separate, RFC-sized work, not something to fold into a docs-gen rendering
increment. Logged in TODO.md as its own tracked item, not silently absorbed into this one.

## Testing

- `docs-gen`: real Crate+Rollup fixtures link correctly; a crate with no matching rollup is
  silently skipped, not reported; Technology Inventory links to real detail pages; the
  duplicate-relationship-id reproduction test.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real data — no new pipeline run needed** (same as Increment 1): `ekos docs generate
  --layout curated` against this repo's own real, already-committed ledger rendered a real
  `## Component View` (44 crates, each linked to its real subsystem page with real member counts)
  and a real, now-deduplicated `## Technology Inventory`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0070-component-view-and-technology-inventory.md` | This RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Increment 2 status note |
| `ekos/crates/docs-gen/src/lib.rs` | New `## Component View` + `render_component_view`; `## Technologies` → `## Technology Inventory` with links + dedup; 4 new/updated tests |
| `TODO.md` | RFC 0068 §61 MVP items ticked off; new relationship-id-non-determinism item tracked; next increment scoped |

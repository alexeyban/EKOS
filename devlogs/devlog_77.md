# Devlog 77 — RFC 0068 Increment 6: Data Architecture view, opening §62 Phase 2

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Sixth increment of continuous build-out against RFC 0068, and the first into §62 Phase 2 (all of
§61's MVP shipped in Increments 1-5). Investigated three candidate Phase 2 starting points against
the real codebase before picking one, shipped a real `## Data Architecture` section in
`Architecture.md` reusing already-compiled SQL/Transformation-IR data, and fixed a real stale
cross-reference bug found along the way.

---

## RFC 0074 — Data Architecture View

### The investigation

TODO.md's own next-step note named three candidates: Data Architecture (existing SQL extractors),
Human Review (extend RFC 0029's identity-review pattern), or a new Terraform/Kubernetes/OpenAPI
extractor for Deployment Architecture. Read RFC 0068 §62's actual item list rather than TODO.md's
paraphrase (which had two section numbers swapped) and noticed `SQL` and `Data Architecture` sit
adjacent in it — read as the Phase-2 SQL work feeding the Phase-2 Data Architecture view. Checked
what §22 (Data Architecture) actually asks for against the real codebase:

- Data Stores/Schemas/Tables — real, already compiled (`sql_analyzer.rs`).
- Data Flows/Transformations/Lineage — real, already compiled (RFC 0027 Transformation IR,
  already rendered in `SequenceDiagrams.md`).
- Data Domains — checked `sql_analyzer.rs`'s actual `Table` object construction: no domain/schema
  property exists. No signal to build on.
- Ownership — checked: `RelationshipKind::OwnedBy` is real, but only `git_analyzer.rs` emits it,
  onto `File` objects, never onto `Table`/`Dataset`. No compiled edge connects a data store to an
  owner.
- Lifecycle, Data Quality — no existing signal at all.

Checked Terraform/Kubernetes/OpenAPI too: zero existing analyzers for any of those formats anywhere
in `plugins/`/`recovery/` — genuinely new extraction, a bigger and riskier increment than one with
real compiled data already behind half its dimensions. Chose Data Architecture.

### What shipped

`render_data_architecture`: real Data Stores (every `Table`/`Dataset`, sorted, each with a real
foreign-key edge count) and real Transformations/Lineage (link-through to `SequenceDiagrams.md`'s
existing Data-Flow Sequences, the same link-through precedent Runtime View set in RFC 0071). The
four dimensions with no real signal — Data Domains, Ownership, Lifecycle, Data Quality — each say
explicitly why, naming the actual missing primitive (e.g. Ownership names `OwnedBy`/
`git_analyzer.rs` specifically, not a vague "not implemented").

Found one real, concrete gap while designing the Transformations & Lineage link-through:
`TransformNode` source/sink nodes carry table names as *properties*, not a relationship edge to the
real compiled `Table` object they read/write — so Data Stores and Transformations can't yet be
cross-referenced to each other. Documented explicitly in the rendered section and in TODO.md rather
than silently assumed connected.

### A real bug fixed along the way

Reading `components_cross_reference` (existing link-text-by-kind lookup) before adding a new entry
found it still pointed `Technology` at `` `## Technologies` `` — the exact heading RFC 0070 renamed
to `` `## Technology Inventory` `` two increments ago. Unpinned by any test, so it had gone
unnoticed. One-line fix, regression test added, folded into this increment rather than a separate
RFC (too small on its own, found only because this increment's own reading touched the surrounding
code).

### Live verification

This repo's own real ledger has zero `Table`/`TransformNode` objects — a Rust codebase observing
itself never produces SQL/ETL content — so it only exercises the honest-empty path. Reused this
repo's own already-committed integration-test fixture (`tests/fixtures/ecommerce.sql`, the same
schema `ecommerce_pipeline_end_to_end` already exercises) instead of writing a new synthetic one:
ran the real pipeline against it in a disposable workspace, then generated curated docs. Real
output: 6 real tables (`categories`, `customers`, `order_items`, `orders`, `payments`, `products`),
each with a correct real foreign-key count, honest gaps for the four uncomputed dimensions.

---

## Knowledge Captured

- **Reading the source standard's own text beats trusting an earlier paraphrase of it** — TODO.md's
  own summary of §62 had two section numbers swapped; re-reading RFC 0068 directly surfaced the
  `SQL`/`Data Architecture` adjacency that made this increment's scope choice obvious. Tracking
  documents drift from source material even within one session; the source is still the source.
- **"Real compiled data already exists for N of M dimensions" is a legitimate, checkable way to
  pick between several roadmap candidates** — not a vague preference, a concrete audit (grep for
  the relevant `ObjectKind`/`RelationshipKind` construction sites, read what properties they
  actually attach) done before committing to a design, the same discipline every increment since
  RFC 0069 has used.
- **A cross-reference/link-text bug can hide indefinitely with no test pinning it** — found only
  because unrelated work happened to read the same function. Worth remembering: a rename (RFC
  0070's `## Technologies` → `## Technology Inventory`) needs a grep for every *other* place that
  names the same string, not just the definition site.
- **Not every real object relationship reaches every real object** — `Table` (SQL DDL) and
  `TransformNode` (Transformation IR) are both real, both correctly compiled, and still not linked
  to each other. Two correct, independently-shipped features can still leave a real integration gap
  between them; RFC 0068's own breadth makes this kind of gap likely to keep surfacing as more views
  get built, worth watching for deliberately rather than assuming "compiled" implies "connected."

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0074-data-architecture-view.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Status note for this increment |
| `ekos/crates/docs-gen/src/lib.rs` | `render_data_architecture`; new `## Data Architecture` section in `render_architecture`; `components_cross_reference` stale-link fix; 6 new tests |
| `TODO.md` | §62 Phase 2 Data Architecture item marked done; next-step pointer updated |
| `devlogs/devlog_77.md` | This file |

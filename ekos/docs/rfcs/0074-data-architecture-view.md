# RFC 0074 — Data Architecture View

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0068 §61 (the full MVP) shipped in Increments 1-5 (RFC 0069-0073). TODO.md's own "Next step"
note framed Increment 6 as opening §62 Phase 2, listing three candidate starting points to
investigate first rather than assuming one: (a) Data Architecture (§22) via the existing SQL
extractors, (b) Human Review extending RFC 0029's `ekos_identity_review` pattern, (c) a new
Terraform/Kubernetes/OpenAPI extractor for Deployment Architecture (§21). This RFC documents that
investigation and the resulting choice: Data Architecture.

## Investigation before design

Read RFC 0068 §62's own item list directly (not TODO.md's paraphrase, which had misnumbered two
sections): `Terraform / Kubernetes / OpenAPI / SQL / Data Architecture / Deployment Architecture /
Security Architecture / Quality Architecture / Architecture Diff / Architecture Drift / Human
Review / ADR generation / MCP`. `SQL` and `Data Architecture` are adjacent in that list — read as
"the SQL-based extraction Phase 2 still needs, feeding the Data Architecture view Phase 2 still
needs" — and EKOS already has real, shipped SQL extraction (`sql_analyzer.rs` for DDL,
`sql_transform_analyzer.rs` for SELECT/VIEW/procedures into the Transformation IR, RFC 0027)
predating RFC 0068 entirely. Checked what §22 actually asks for against what's already compiled:

- **Data Stores, Schemas, Tables, Entities** — real, already compiled: `ObjectKind::Table`/
  `Dataset` objects from `sql_analyzer.rs`.
- **Data Flows, Transformations, Lineage** — real, already compiled: `Custom("TransformNode")` +
  `Custom("FeedsInto")` (RFC 0027 Transformation IR), already rendered as "Data-Flow Sequences" in
  `SequenceDiagrams.md`.
- **Data Domains** — checked `sql_analyzer.rs`'s `KirObject::new(&table_name, ObjectKind::Table)`
  construction directly: no schema/database/domain property is attached. No real signal exists to
  group by; not built.
- **Ownership** — checked: `RelationshipKind::OwnedBy` exists and is real, but only
  `git_analyzer.rs` emits it, onto observed `File` objects — never onto `Table`/`Dataset` objects,
  which come from SQL DDL parsing, not git-history analysis. No edge connects a compiled data store
  to an owner today.
- **Lifecycle, Data Quality** — checked for any existing property/relationship carrying this; none
  exists.

Also checked Terraform/Kubernetes/OpenAPI (candidate (c)): none of `ekos/plugins/` or
`crates/recovery/` has any existing parser for those formats — genuinely new extraction work with
no existing analyzer to extend, a larger and riskier increment than the option with real compiled
data already sitting behind five of §22's eleven dimensions.

**Chose Data Architecture**: the most real, already-compiled data of the three candidates, and the
same "reuse existing compiled data, extend only what's missing" judgment call every increment since
RFC 0069 has made.

## Design

New `render_data_architecture(objects, relationships) -> String` in `docs-gen`, called from
`render_architecture` in a new `## Data Architecture` section placed after `## Runtime View` (RFC
0068 §20) and before `## Open Questions` — matching the source standard's own §20 → §22 numeric
adjacency (§21 Deployment Architecture is skipped, not yet built).

- **Data Stores**: every compiled `Table`/`Dataset`, sorted by name, each with its real
  `ForeignKey`-edge count (a real, cheap-to-compute connectivity signal). Listed by name only, not
  linked to a per-object page — `Table`/`Dataset` aren't `is_entity_page_kind`, so no curated
  per-object page exists for them; linking would produce a dangling reference under `--layout
  curated`.
- **Transformations & Lineage**: link-through to `SequenceDiagrams.md`'s existing "Data-Flow
  Sequences" section (`Custom("FeedsInto")` presence check) — the exact same link-through precedent
  Runtime View (RFC 0071) already established for the same reason (don't duplicate real content
  that already renders correctly elsewhere). Includes an explicit note that `TransformNode`
  source/sink nodes carry table names as *properties*, not a relationship edge to a real `Table`
  object — a genuine gap in cross-referencing this session found while designing this view, tracked
  as real follow-on work rather than silently assumed away.
- **Data Domains / Ownership / Lifecycle / Data Quality**: each an explicit `_not yet computed —
  <real reason>_` line, the same honest-gap convention `render_architecture_summary` (RFC 0071)
  established — Ownership's reason names the concrete existing primitive (`OwnedBy`,
  `git_analyzer.rs`) that doesn't yet reach data objects, not a vague "not implemented."

## A real bug fixed along the way

While reading `components_cross_reference` (the `## Components` section's per-kind link text) to
understand existing cross-reference conventions before adding a new one, found it still pointed
`Technology` at `` `## Technologies` `` — the heading RFC 0070 renamed to `` `## Technology
Inventory` `` two increments ago. A real stale link in `Architecture.md`'s own `## Components`
section, unpinned by any existing test. Fixed as a one-line change alongside this increment (not a
separate RFC — too small, found only because this increment's own investigation touched the
surrounding code), with a regression test added.

## What this does and doesn't cover

**Covers**: real Data Stores + real Transformations/Lineage link-through, for every workspace that
already runs `sql_analyzer`/`sql_transform_analyzer` or Pentaho/Python transformation recovery.

**Does not cover**: Data Domains, Ownership, Lifecycle, Data Quality for data objects — each
requires either new extraction (a domain/schema property, an owner-linking relationship) or a
design decision this RFC didn't need to make to ship real value from what's already compiled.
Tracked as explicit follow-on items in TODO.md, not silently dropped.

## Testing

- `docs-gen` unit tests: `render_data_architecture` with real Table/ForeignKey fixtures (data-store
  listing + FK counts); with real TransformNode/FeedsInto fixtures (link-through); on an empty
  ledger (every honest-gap line present, no dangling `SequenceDiagrams.md` link). `render_architecture`
  integration test confirming the section heading, §22 citation, and real table name appear.
  Regression test for the `## Technology Inventory` cross-reference fix.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end**: this repo's own committed ledger has zero `Table`/`TransformNode`
  objects (a Rust codebase, no SQL/ETL content of its own), so it only exercises the honest-empty
  path — not sufficient alone. Reused this repo's own real, already-committed integration-test
  fixture (`tests/fixtures/ecommerce.sql`, the same schema `ecommerce_pipeline_end_to_end` already
  exercises) instead of writing a new one: ran the real pipeline (`init → build → recover → resolve
  → compile → commit`) against it in a disposable workspace, then `ekos docs generate --layout
  curated`. Real output: 6 real tables (`categories`, `customers`, `order_items`, `orders`,
  `payments`, `products`), each with a real, correct foreign-key count (1-3 edges), honest gaps for
  the four uncomputed dimensions.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0074-data-architecture-view.md` | This RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Status note for this increment |
| `ekos/crates/docs-gen/src/lib.rs` | `render_data_architecture`; wired into `render_architecture`'s new `## Data Architecture` section; `components_cross_reference`'s stale `Technology` link text fixed; 6 new tests |
| `TODO.md` | §62 Phase 2 Data Architecture item marked done; next-step pointer updated |
| `devlogs/devlog_77.md` | This increment's devlog |

# Devlog 36 — RFC 0037 Phase 1: curated documentation set (README/Architecture/API/SequenceDiagrams)

**Date:** 2026-08-07
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Implemented Phase 1 of RFC 0037: `ekos docs generate --layout curated`, a second output shape
alongside RFC 0035's existing per-object multi-page default (`--layout objects`, unchanged). The
curated layout renders exactly four fixed-name Markdown files — `README.md`, `Architecture.md`,
`API.md`, `SequenceDiagrams.md` — the documentation set a developer actually expects from a
project, built from the same compiled `ekos-docs-gen` reads for the per-object pages. No new
extraction pass: `README.md` pulls real object-kind counts and real git-derived contributors;
`Architecture.md` pulls real `dependency_analyzer` technology edges, the existing ER diagram, and
one small Mermaid graph per structural relationship kind (deliberately excluding pipeline-internal
`FeedsInto` wiring, which would make the diagram unreadable — a real Pentaho workspace has 86
`TransformNode`s); `API.md` honestly lists file-level symbol names harvested by a text scan,
caveated as not a parsed API spec, since no analyzer compiles real `Api`/`Service` objects;
`SequenceDiagrams.md` renders one Mermaid `sequenceDiagram` per compiled Transformation IR
pipeline from real `FeedsInto` step order, explicitly labeled as a data-flow sequence, not a code
call sequence, since no analyzer compiles `RelationshipKind::Calls` data. Verified end-to-end
against a real compiled SQL schema.

---

## RFC 0037 Phase 1 — four curated renderers + `--layout curated`

### Problem / motivation

The user asked for EKOS's existing "generate documentation from code" feature to produce the
classic four-artifact documentation set instead of (or alongside) many per-object pages:
`Architecture.md`, `API.md`, `README`, sequence diagrams. Before designing anything, the real
feasibility ceiling was investigated: grepping the whole `crates/`/`plugins/` tree confirmed
`RelationshipKind::Calls` and `ObjectKind::Api`/`ObjectKind::Service` are never constructed by any
analyzer — there is no real call-graph or API-surface data in the ledger today. This directly
shaped the design: rather than fabricate data that doesn't exist, each document renders exactly
what real analyzers produce, with an honest empty-state placeholder wherever the data doesn't
exist, matching the project's established "Unmapped is a citizen, not a failure" discipline.

Two scoping decisions were confirmed with the user before implementation:
1. **Additive, not a replacement** — `--layout curated` is a new opt-in flag; `--layout objects`
   (today's default) is unchanged, so nothing already tested or already described in the
   `generated-documentation.html` deck breaks.
2. **Sequence diagrams render from Transformation IR `FeedsInto` chains**, the only real *ordered*
   flow data that exists, explicitly labeled as a data-flow sequence rather than presented as a
   code call sequence that doesn't exist.

### What was built

| Component | Location |
|---|---|
| Four new renderers | `ekos/crates/docs-gen/src/lib.rs` — `render_readme`, `render_architecture`, `render_api`, `render_sequence_diagrams` |
| CLI wiring | `ekos/crates/cli/src/commands/docs.rs` — `Layout` enum, `generate_curated` |
| Clap flag | `ekos/crates/cli/src/bin/ekos.rs` — `--layout objects\|curated` on `DocsCommands::Generate` |
| RFC | `ekos/docs/rfcs/0037-curated-documentation-set.md` (new) |

Added to the same `ekos-docs-gen` crate rather than a new sibling crate (unlike RFC 0036's
`ekos-dbt-gen`) — this is a different document *shape* over the same compiled-object data
`docs-gen` already reads, not a different output *type* the way dbt SQL was. Reuses
`render_er_diagram`, `mermaid_node_id`/`mermaid_escape_label`/`mermaid_arrow`, and
`render_index_page`'s kind-grouping idiom directly rather than duplicating them.

### Implementation details worth remembering

- **`Architecture.md`'s `## Dependency Graph` splits by relationship kind into separate small
  Mermaid graphs**, explicitly excluding `Custom("FeedsInto")` edges — pipeline-internal step
  wiring belongs in `SequenceDiagrams.md`, and inlining 86 `TransformNode`s' worth of edges into
  one graph (a real number from RFC 0035's own Pentaho test) would make it unreadable. This is
  this RFC's pragmatic answer to RFC 0035's still-open "diagram size" question: split by
  relationship *purpose*, not by trying to fit everything into one graph.
- **`SequenceDiagrams.md` groups `TransformNode` objects by origin** using the trailing
  `:{index}` stripped from `object.name` (`transform_ir.rs::lower_to_kir` names every node
  `{source_path}:{index}`) — no new data needed, purely string-splitting an id scheme that already
  exists.
- **`API.md`'s empty-state placeholder correctly triggered on the real smoke-test fixture** — the
  scratch workspace only had a `schema.sql` file, no source files with harvested `symbols`, so
  `API.md` honestly rendered "_No API surface data compiled._" rather than an empty-looking but
  technically-non-placeholder page.
- **`Format`/`Layout` are independent flags**: `--layout curated --format html` errors clearly
  ("HTML curated output is an open question in RFC 0037, not yet implemented") rather than
  silently ignoring `--format` — same "no valid degraded mode, fail clearly" pattern RFC 0035
  Phase 5's `--prose` provider-selection already established.

### Testing

12 new unit tests in `ekos-docs-gen` (50 total, up from 38) — one real-data test and one
honest-empty-state test per renderer, plus a two-origin `SequenceDiagrams.md` test (two pipelines
must render as two separate `sequenceDiagram` blocks, not merged) and a regression test confirming
`Architecture.md`'s dependency graph excludes `FeedsInto` edges. 3 new CLI-level tests in
`commands/docs.rs` (20 total, up from 17): `--layout curated` writes exactly the four named files
and nothing else, `--layout curated --format html` errors clearly, and `Layout::parse` accepts/
rejects correctly. All 6 pre-existing `docs.rs` tests pass unmodified — the additive-flag decision
held. `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` both clean.

Real end-to-end smoke test against the same compiled-SQL scratch workspace used for RFC 0036's
testing (2 tables + a real `CREATE VIEW ... JOIN ... WHERE ... GROUP BY`): real `Architecture.md`
ER diagram + `ForeignKey` dependency graph, real `SequenceDiagrams.md` showing the actual
Join→Join→Filter→Aggregate→Sink chain, and the honest `API.md` placeholder described above. No
bugs found this time (unlike RFC 0035/0036's real-data tests) — likely because this RFC's design
was scoped defensively around real feasibility limits from the start, rather than assuming data
shapes and discovering gaps afterward.

### Decisions (alternatives considered, why this choice)

- **Same crate, not a new one** — `ekos-dbt-gen` was split out because dbt SQL is a genuinely
  different output type with different structural rules (`ref()`/`source()` semantics); this RFC's
  four documents are the same "render compiled KIR objects as Markdown text" job `ekos-docs-gen`
  already does, just a different shape, so staying in one crate avoids duplicating
  `html_escape`/grouping/Mermaid helpers.
- **Honest empty-state placeholders over fabricating content** — same reasoning as every prior
  phase; an "API.md" that invents endpoints from file names, or a "SequenceDiagrams.md" that
  presents data-flow order as a code call sequence, would violate the evidence-first ethos harder
  than a plain "no data compiled" sentence does.
- **Markdown-only for this phase** — HTML curated output is real additional work (four more
  renderers) with no new design questions to resolve; deferred to Phase 3 rather than doubling
  this phase's scope for content that's structurally identical to the Markdown output.

---

## Knowledge Captured

- **No real call-graph or API-surface data exists anywhere in EKOS today** — confirmed by
  exhaustive grep, not assumed. Any future feature that wants either (a proper `API.md`, a real
  code-call sequence diagram) needs a new analyzer first; this is real unscoped work, not a gap in
  the rendering layer.
- **`dependency_analyzer.rs` is a fixed 5-technology substring-match table** (PostgreSQL/MySQL/
  MongoDB/Redis/Kafka), not an import/package parser — `Architecture.md`'s "Technologies" section
  is only as complete as that fixed list; a real project using an unlisted technology renders
  nothing for it, correctly, rather than guessing.
- **`git_analyzer.rs`'s `Person.commit_count` is real, already-aggregated data** — no additional
  ranking/aggregation logic was needed to build README's "Contributors" section beyond a sort.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0037-curated-documentation-set.md` | new — full RFC with Implementation Plan |
| `ekos/crates/docs-gen/src/lib.rs` | `+render_readme`, `+render_architecture`, `+render_api`, `+render_sequence_diagrams`, `+is_feeds_into`, `+count_by_kind`, `+render_relationship_kind_graph`, `+transform_node_origin`, 12 new tests |
| `ekos/crates/cli/src/commands/docs.rs` | `+Layout` enum, `+generate_curated`, `--layout` threading, 3 new tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `+--layout` arg on `DocsCommands::Generate` |

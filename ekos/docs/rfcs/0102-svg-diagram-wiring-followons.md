# RFC 0102 — `render_graph_svg` wiring follow-ons (RFC 0068 §61)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0073/RFC 0083 Phase 4 shipped a generic, dependency-free, deterministic `render_graph_svg`/
`layer_nodes` primitive and used it for System Context, System Decomposition, and Crate &
Workspace Topology. RFC 0068 §61's own follow-on list named three remaining wiring sites this RFC
closes: per-object neighborhood diagrams (`--layout objects`), the per-relationship-kind
`## Dependency Graph` subsections, and the `erDiagram` family. `sequenceDiagram` is confirmed still
open (Non-goal, below) — not attempted here.

One item on that follow-on list turned out to already be resolved: *"`layer_nodes` doesn't wrap
wide layers within one row"*. Re-reading the actual code (`layer_nodes`/`wrap_layer_into_rows`/
`MAX_NODES_PER_ROW`) found this was already fixed by RFC 0084 (`devlog_87`, 2026-08-24) — the
stale backlog note in `TODO.md` was never updated after that RFC shipped. Corrected here rather
than re-implemented.

## Design

Every new function below follows the exact shape RFC 0073/0083 already established: a small
`(nodes, edges) -> IdGraph` extraction function mirroring an existing Mermaid renderer's own
node/edge selection, plus a `pub fn render_*_svg(..) -> Option<RenderedPage>` that feeds that into
`render_graph_svg` unmodified, `None` under the identical "nothing real to draw" condition the
Mermaid renderer already falls back to text for. No change to `render_graph_svg`/`layer_nodes`
themselves — the primitive was already generic enough.

### Per-object neighborhood SVG (`--layout objects`)

`object_neighborhood_graph` extracts `render_mermaid_graph`'s same 1-hop neighborhood (center
object + every object at the other end of a relationship) into plain `(id, label)`/`(from, to)`
pairs — deliberately dropping edge *kind* labels and arrow style (dashed for `CoupledWith`), a
Mermaid-only concern `render_graph_svg` has no field for. `render_object_neighborhood_svg` wraps
this, `None` when `relationships.is_empty()` (the same check `build_object_page_model` already
uses to skip the Mermaid diagram). Wired into `docs.rs::generate`'s existing per-object loop,
writing `<kind>-<name>.svg` alongside each `.md`/`.html` page.

### Per-relationship-kind Dependency Graph SVG (`--layout curated`)

`render_architecture`'s own `## Dependency Graph` loop already groups relationships by kind and
decides, per kind, "real diagram" vs. "omitted, too large" (`MAX_GRAPH_EDGES = 20`, found live
against a real Pentaho+PDF workspace where `Contains` alone produced 74 edges). The one real
design decision here: **the SVG writer must never independently re-derive that same eligible-kind
filter**, or it risks drifting from the Markdown page's own decision — the exact "logic duplicated
across two spots, one drifts" shape this codebase has hit repeatedly (`DefaultResolver`'s
kind-exclusion list, the two ledger backends' indexed-content field lists, both named in
`CLAUDE.md`). `MAX_GRAPH_EDGES` is hoisted to module scope and a new `dependency_graph_groups`
function factors the exact grouping/filtering logic out, called both by `render_architecture`'s own
loop (unchanged behavior) and by `generate_curated`'s new SVG-writing loop — one source of truth,
not two copies. `render_relationship_kind_graph_svg` (mirrors `relationship_kind_ids_graph`, the
SVG-shaped counterpart to `render_relationship_kind_graph`) writes `dependency-graph-<kind>.svg`;
the Markdown gets a `[<kind> Dependency Graph diagram (SVG)](...)` link right after each kind's
Mermaid block, matching the existing crate-topology/system-context link convention.

### Whole-workspace ER diagram SVG (`--layout objects`)

`er_diagram_graph` mirrors `render_er_diagram`'s own filter exactly (only `ForeignKey` edges
strictly between two objects in `tables`, deduplicated by `(from, to)` pair). `render_er_diagram_svg`
wraps it, wired into `docs.rs::generate`'s existing `er-diagram.md`/`.html` writing, alongside a new
`er-diagram.svg` and a second `index.md`/`index.html` diagrams-list entry.

`render_graph_svg`'s plain box-and-arrow layout is a real, honest simplification of `erDiagram`'s
crow's-foot notation — every table and every `ForeignKey` edge is real and present, just without
cardinality glyphs. Stated explicitly, not silently glossed over.

## Non-goals

- **`sequenceDiagram` SVG.** A sequence diagram is fundamentally a different shape — participant
  lanes over a time axis, not a layered DAG `layer_nodes`/`render_graph_svg` can lay out. Forcing it
  through the existing primitive would misrepresent the diagram rather than simplify its notation
  (unlike the ER diagram case above, where the simplification is honest). Needs its own real layout
  primitive — left as a clearly scoped future increment, not silently dropped.
- **Cardinality glyphs on the ER diagram SVG.** `render_graph_svg` draws a plain arrow for every
  edge; real crow's-foot notation would need a new marker/label convention specific to this one
  caller. Not attempted — the plain arrow already correctly conveys which tables relate to which.

## Verification

9 new `ekos-docs-gen` unit tests (per-object neighborhood: none-with-no-relationships, real
center+neighbor render, unresolvable-neighbor-by-id; dependency-graph: link-within-cap,
no-link-when-oversized, `dependency_graph_groups` excludes `FeedsInto`/oversized kinds, direct
`render_relationship_kind_graph_svg` render + empty case; ER diagram: real render, none-with-no-FKs,
excludes-outside-table-set), 3 new `ekos` (CLI) integration tests (`--layout objects` per-object SVG
written and contains real neighbor labels; `--layout curated` Dependency Graph SVG written and
linked from `Architecture.md`; ER diagram SVG written and linked from `index.md`), full workspace
gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`),
`tests/integration` 3/3.

Live-verified against a real scratch workspace (3 real tables — `customers`, `orders`,
`order_items` — 2 real compiled `ForeignKey` relationships, through the real
`init`/`build`/`recover`/`resolve`/`compile`/`commit` pipeline): `docs generate --layout objects`
wrote `table-customers.svg`/`table-orders.svg`/`table-order-items.svg` (3 neighborhood SVGs, one per
table with a real relationship) plus `er-diagram.svg`; `docs generate --layout curated` wrote
`dependency-graph-foreignkey.svg`, and `Architecture.md` contains the real link text
`[ForeignKey Dependency Graph diagram (SVG)](dependency-graph-foreignkey.svg)`. Every generated SVG
confirmed to start with `<svg ` and contain the real table names as `<text>` content, not placeholder
or fabricated labels.

## Known follow-up, found live during verification (not this RFC's scope)

Running the real self-analysis ledger at the repo root (`/home/legion/PycharmProjects/EKOS/.ekos`)
through `docs generate` failed with `Schema error: 'An index exists but the schema does not match.'`
— a real, previously-undiscovered consequence of RFC 0101 (same session, shipped earlier) adding a
new `memory_path` field to `SearchIndex`'s tantivy schema with no migration path for an
already-built on-disk index. `SearchIndex::open_impl` calls `Index::open_or_create`, which validates
the on-disk schema against the code's current schema and errors rather than transparently upgrading.
Every pre-existing `FactLedger` workspace built before RFC 0101 is affected, not just this repo's
own. Not fixed here — this RFC's scope is `docs-gen` SVG wiring, and rebuilding/migrating a real
production ledger's search index needs its own explicit decision (rebuild vs. a real schema-version
migration), flagged to the user rather than acted on unilaterally. Tracked in `TODO.md`.

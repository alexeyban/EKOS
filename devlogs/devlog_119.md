# Devlog 119 — RFC 0102: closing out RFC 0068 §61's `render_graph_svg` follow-on list

**Date:** 2026-08-26
**PRs:** RFC 0102
**Branch:** main (direct)

---

## Summary

RFC 0068's own §61 follow-on list named three remaining `render_graph_svg` wiring sites after
RFC 0073/0083 Phase 4 shipped the primitive for System Context, System Decomposition, and Crate &
Workspace Topology: per-object neighborhood diagrams, the per-relationship-kind Dependency Graph,
and the `erDiagram` family. All three are now wired. `sequenceDiagram` stays a named, deliberate
Non-goal — a genuinely different diagram shape, not a trimmed corner. One item on the same list
(`layer_nodes` wide-layer wrapping) turned out to already be shipped by RFC 0084 two days earlier;
`TODO.md`'s stale note claiming otherwise is corrected in this same session.

---

## RFC 0102 — `render_graph_svg` wiring follow-ons

### Problem / motivation

`docs/GAP_ANALYSIS.md`-derived tracking in `TODO.md` listed four concrete `## §61 MVP —
remaining pieces` items after the six-item MVP itself shipped. Picking this up as the next
increment of RFC 0068's explicit "build the whole thing, only sequence it" instruction.

### What was built

| Component | Change |
|---|---|
| `object_neighborhood_graph` / `render_object_neighborhood_svg` | Per-object 1-hop neighborhood as a standalone SVG, `--layout objects` |
| `dependency_graph_groups` / `relationship_kind_ids_graph` / `render_relationship_kind_graph_svg` | Per-relationship-kind Dependency Graph SVG, `--layout curated` |
| `er_diagram_graph` / `render_er_diagram_svg` | Whole-workspace ER diagram SVG, `--layout objects` |
| `TODO.md` correction | `layer_nodes` wrap-within-row item marked done (RFC 0084, already shipped) instead of left stale |

### Implementation details worth remembering

- **The Dependency Graph SVG needed a real anti-duplication decision, not just a new function.**
  `render_architecture`'s own Markdown loop already decides per relationship-kind whether to draw
  a real diagram or fall back to an "omitted, too large" sample list (`MAX_GRAPH_EDGES = 20`, a cap
  found live against a real Pentaho+PDF workspace where `Contains` alone produced 74 edges). A
  naive SVG writer re-deriving that same by-kind grouping and cap independently would risk drifting
  from the Markdown's own decision — exactly the "logic duplicated across two spots, one drifts"
  shape this codebase has hit repeatedly enough that `CLAUDE.md` calls it out by name for identity
  resolution and the two ledger backends. Fixed by hoisting `MAX_GRAPH_EDGES` to module scope and
  factoring the grouping/filter logic into one `dependency_graph_groups` function both the Markdown
  loop and the new SVG-writing loop call — one source of truth instead of two copies that could
  silently disagree about which kinds get a real diagram.
- **Every new node/edge extraction function deliberately drops edge *kind* labels and arrow
  style**, matching the precedent RFC 0073's `system_context_graph` already set: those are a
  Mermaid-only concern (`-.->` for `CoupledWith`, `|kind|` edge labels) that `render_graph_svg`'s
  plain `(id, label)`/`(from, to)` shape has no field for. Not a missing feature — a deliberate,
  matching simplification, same as every sibling SVG this project already ships.
- **A stale backlog note gets found and corrected by re-reading the actual code, not by assumption.**
  `TODO.md` still listed `layer_nodes` wide-layer wrapping as an open item; the actual code
  (`wrap_layer_into_rows`, `MAX_NODES_PER_ROW = 8`, a passing test named exactly for this case) shows
  RFC 0084 shipped it two days before this session's read of the backlog. A user-pasted status
  summary earlier in this session repeated the same stale claim — checking the real code before
  trusting a status document (even one that looks authoritative) caught this before any wasted
  re-implementation effort.

### Decisions (alternatives considered, why this choice)

- **`sequenceDiagram` SVG deliberately not attempted, named as a real Non-goal.** A sequence diagram
  is fundamentally a different shape from every other diagram this primitive draws — participant
  lanes over a time axis, not a layered DAG. Forcing it through `layer_nodes`/`render_graph_svg`
  would misrepresent the diagram, not just simplify its notation (unlike the ER diagram case, where
  dropping crow's-foot cardinality glyphs is an honest simplification, not a misrepresentation).
  Needs its own real layout primitive — left as a clearly scoped future increment per RFC 0068's own
  "sequence it, don't cut it" instruction, not silently dropped from the list.

---

## Knowledge Captured

- **A user-pasted "current state" summary can itself be stale — verify against the actual code
  before treating it as ground truth, even when it reads as authoritative.** The layer-wrapping item
  in this session's own status paste was already fixed two RFCs earlier; the fix would have been a
  wasted re-implementation (or worse, a second, drifted copy of the same fix) if taken at face value
  instead of grepped and read first.
- **RFC 0101 (same session, shipped just before this RFC) has a real, previously-undiscovered
  production consequence**: adding a new `memory_path` field to `SearchIndex`'s tantivy schema with
  no migration path breaks `Index::open_or_create` against any already-built on-disk `FactLedger`
  search index (`Schema error: 'An index exists but the schema does not match.'`) — confirmed live
  against this repo's own real self-analysis ledger at the repo root. Every pre-existing `FactLedger`
  workspace is affected, not just this one. Deliberately not fixed as part of this RFC (out of
  scope, and rebuilding/migrating a real production ledger's search index is a decision for the
  user, not something to act on unilaterally) — flagged here and in `TODO.md` instead.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/docs-gen/src/lib.rs` | `object_neighborhood_graph`/`render_object_neighborhood_svg`; `dependency_graph_groups`/`relationship_kind_ids_graph`/`render_relationship_kind_graph_svg`; `er_diagram_graph`/`render_er_diagram_svg`; `MAX_GRAPH_EDGES` hoisted to module scope; 9 new tests |
| `ekos/crates/cli/src/commands/docs.rs` | Wired all three into `generate`/`generate_curated`; 3 new/extended tests |
| `ekos/docs/rfcs/0102-svg-diagram-wiring-followons.md` | New RFC |
| `TODO.md` | §61 follow-on list updated: three items done, `layer_nodes` wrap item corrected as already-done (RFC 0084), `sequenceDiagram` SVG named as a real Non-goal; RFC 0101 schema-break finding tracked |

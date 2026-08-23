# RFC 0084 — Diagram-Quality Fixes (Row-Wrapping, Crate Topology SVG, Honest Component View)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Phase 4 of the "Deep Source Decomposition + Production-Grade Architecture Diagrams" plan — the
plan's own already-tracked finding (RFC 0073): a real layer with many nodes renders as one
unreadably wide row (46 nodes → an 8296×190px single row for EKOS's own self-dogfooded System
Context diagram, the concrete example that motivated the whole plan). Two smaller, real,
immediately valuable fixes bundled with it: a standalone SVG for `## Crate & Workspace Topology`
(one of the sections RFC 0073 itself explicitly deferred), and making `## Component View`'s
unmatched-crate skip honest instead of silent.

## Design

**Row-wrapping** (`crates/docs-gen/src/lib.rs`): `layer_nodes` (Kahn's-algorithm topological
layering) is unchanged — it's still the correct DAG semantics. New `wrap_layer_into_rows`
chunks any topological layer wider than `MAX_NODES_PER_ROW` (8) into multiple *visual* rows,
tagging each with whether it starts a new topological layer. `render_graph_svg` now stacks visual
rows rather than topological layers directly: a new-layer boundary keeps the existing
`SVG_LAYER_GAP`, a wrap-continuation row gets a smaller `SVG_ROW_GAP` (16px) so it visually reads
as "more of the same row," not a new DAG level. Width is now sized from the widest *row*, not the
widest *layer* — the whole point of wrapping.

**Crate & Workspace Topology SVG**: new `crate_topology_graph`/`render_crate_topology_svg`,
mirroring `system_context_graph`/`render_system_context_svg`'s exact shape — real `Crate`→`Crate`
`DependsOn` edges only, `None` (no file written) when there are none, reusing `render_graph_svg`
completely unmodified. Linked from `## Crate & Workspace Topology` the same way System
Context/System Decomposition already link their own SVGs.

**Honest Component View**: `render_component_view` now tracks which crates had no matching
`Rollup` (not just which did), and reports them by name and count below the linked list — real,
not fabricated (RFC 0044's 2-member threshold is a legitimate reason to have none), but no longer
invisible. The one previous case that printed a blanket "no rollup at all" message now only fires
when there are truly zero crates *and* zero matches; any real crate list with at least one
unmatched entry gets the honest per-crate note instead.

## Scope — what this does and doesn't cover

**Covers**: the three items above, all bundled because they're small, real, and each improves an
existing diagram/section immediately without new extraction.

**Does not cover** (explicitly deferred, not silently dropped): standalone SVGs for per-object
neighborhood diagrams (`render_mermaid_graph`'s callers — one per compiled object, thousands of
them for a real project) and `render_api`'s per-relationship-kind graphs (`Contains`/`Calls`/…,
already Mermaid-only). Both were in the plan's original Phase 4 scope but are a different shape of
work — many small per-object/per-kind SVGs rather than one whole-workspace SVG per section — with
much lower marginal readability payoff than the three items shipped here (individual object pages
already show their own 1-hop Mermaid diagram; the unreadable-diagram complaint that started this
plan was specifically about whole-workspace views). Left for a future increment if real usage
shows it's actually needed, not assumed necessary.

## Testing

- 1 new test: `render_graph_svg_wraps_a_layer_wider_than_max_nodes_per_row` — a 12-node
  single-layer graph must produce 3 distinct row y-positions (root row + two wrapped child rows,
  8+4) and size its width from the 8-node row, not the unwrapped 12-node layer.
- 2 new tests: `render_crate_topology_svg_renders_real_internal_crate_dependencies`,
  `render_crate_topology_svg_is_none_with_no_internal_dependencies`.
- 2 updated/new tests for Component View: the old
  `architecture_component_view_silently_skips_a_crate_with_no_matching_rollup` renamed to
  `architecture_component_view_honestly_reports_a_crate_with_no_matching_rollup` and asserts the
  crate is now named in the output; new
  `architecture_component_view_reports_no_crates_at_all_when_none_are_compiled` covers the
  genuinely-zero-crates case the old blanket message still correctly serves.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end** against two real workspaces:
  - The real analytics project (Elixir/Phoenix, no `Cargo.toml` anywhere) correctly has neither a
    `system-context.svg` nor a `crate-topology.svg` — honest `None`, no `Crate` objects exist to
    build either graph from.
  - EKOS's own self-dogfooded ledger (this repo, already committed from an earlier session) is
    the real Rust workspace that actually exercises all three fixes: `system-context.svg` now
    renders its real 46 technology nodes as a multi-row 1488×470px diagram instead of the
    previously-reported unreadable single-row 8296×190px; a new `crate-topology.svg` renders 44
    real crates across multiple rows (1488×1182px); `Architecture.md`'s Component View now
    honestly names `ekos-benchmark, ekos-integration-tests` as the 2 real crates with no matching
    rollup, instead of silently omitting them.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0084-diagram-quality-fixes.md` | This RFC |
| `ekos/crates/docs-gen/src/lib.rs` | `wrap_layer_into_rows`, `render_graph_svg` row-stacking; `crate_topology_graph`/`render_crate_topology_svg`; honest `render_component_view`; 5 new/updated tests |
| `ekos/crates/cli/src/commands/docs.rs` | `crate-topology.svg` conditional write |
| `TODO.md` | Phase 4 of the decomposition plan marked done (with its own honest scope note) |
| `devlogs/devlog_87.md` | This increment's devlog |

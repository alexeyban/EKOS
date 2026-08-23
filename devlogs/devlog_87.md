# Devlog 87 — Diagram-quality fixes (RFC 0084), Phase 4 of the docs quality plan

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Fourth phase of the source-decomposition plan — three bundled, real, immediately-visible fixes to
existing diagrams rather than new extraction: System Context's row-wrapping bug (the concrete
example that motivated the whole plan — a real 46-node diagram rendering as one unreadable
8296×190px row), a standalone SVG for Crate & Workspace Topology, and an honest (not silent)
Component View for crates with no matching rollup.

## RFC 0084

Row-wrapping kept `layer_nodes`'s topological DAG layering completely unchanged and added a
second, purely visual pass (`wrap_layer_into_rows`) that chunks any layer wider than 8 nodes into
multiple rows, using a smaller gap for a wrap-continuation than for an actual new DAG layer so the
two read differently. `render_graph_svg`'s width now comes from the widest *row*, not the widest
*layer* — the actual fix, since the old code sized the whole SVG off the single largest layer
regardless of how many nodes were crammed into its one row.

Crate & Workspace Topology's SVG reused `system_context_graph`/`render_system_context_svg`'s
shape almost verbatim — same `None`-on-empty contract, same `render_graph_svg` primitive, no new
rendering logic needed, just a different real relationship set (`Crate`→`Crate` `DependsOn`
instead of `Crate`→`Technology`).

Component View's fix was the smallest of the three but the one most directly requested by the
plan's own text ("report a real, honest count of unmatched containers instead of only ever
showing successes or an all-or-nothing empty state") — tracked which crates had no rollup match
alongside which did, instead of only ever tracking the successes.

## Live verification

The real analytics project (Elixir/Phoenix) has zero `Cargo.toml` files anywhere, so it correctly
has neither a System Context nor a Crate Topology diagram at all — confirmed via `ekl` that zero
`Custom("Crate")` objects exist for that project, so `None` is the honest, correct output, not a
bug. The three fixes actually needed a Rust workspace to exercise, so verification used EKOS's own
already-committed self-dogfooded ledger (this repo) instead: `system-context.svg`'s real 46
technology nodes now render as a multi-row 1488×470px diagram (previously a single unreadable
8296×190px row); a new `crate-topology.svg` renders 44 real crates across multiple rows;
`Architecture.md` now names `ekos-benchmark, ekos-integration-tests` explicitly as the 2 real
crates with no matching rollup, where the old code silently dropped them from the page with no
trace at all.

## Knowledge Captured

- **A generic diagram renderer's readability fix doesn't always need new data or a new diagram
  type** — the row-wrapping bug was purely a layout defect in code that already had all the real
  data it needed (`layer_nodes`'s topological assignment was already correct); the actual bug was
  treating "one DAG layer" and "one visual row" as always the same thing, which breaks the moment
  a layer gets wide. Worth checking whether a diagram-quality complaint is a data gap or a layout
  bug before reaching for a new extractor — this one turned out to be layout only.
- **Not every real project exercises every diagram** — the analytics project's total absence of
  `Crate` objects (not a Rust project at all) meant this phase's own fixes needed a *different*
  real workspace (EKOS's own self-dogfooded ledger) to verify against. Worth keeping more than one
  real live-verification target in mind rather than assuming the one project used for earlier
  phases covers every feature.
- **"Silently skips X" in an existing doc comment is worth treating as a standing finding, not
  just documentation** — `render_component_view`'s own doc comment already named the exact defect
  this phase fixed (`"A crate with no matching rollup is silently skipped, not reported as
  missing"`) well before this phase started; the fix was reading what the code already admitted
  to, not discovering something new.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0084-diagram-quality-fixes.md` | New RFC |
| `ekos/crates/docs-gen/src/lib.rs` | Row-wrapping; crate topology SVG; honest Component View; 5 new/updated tests |
| `ekos/crates/cli/src/commands/docs.rs` | `crate-topology.svg` conditional write |
| `TODO.md` | Phase 4 marked done |
| `devlogs/devlog_87.md` | This file |

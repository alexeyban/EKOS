# Devlog 76 — RFC 0068 Increment 5: System Context SVG artifact

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Fifth increment of continuous build-out against RFC 0068. Closes out the last open RFC 0068 §61
MVP item: `docs-gen` now produces a real, standalone SVG artifact (not just Mermaid-in-Markdown)
for the System Context diagram, via a new generic, dependency-free, deterministic graph-to-SVG
renderer. All six RFC 0068 §61 MVP view items are now shipped.

---

## RFC 0073 — System Context SVG Artifact

### Problem / motivation

Every diagram `docs-gen` produces is Mermaid fenced code inside a Markdown file — real, useful, but
not a standalone artifact a non-Markdown consumer (an image viewer, a slide deck, a static site
that doesn't render Mermaid) can use directly. RFC 0068 §61 named this as the one remaining MVP
gap after Increments 1-3 shipped the other five view items.

### Why one diagram, not four

`docs-gen` builds `graph TD` diagrams from three different call sites, each with its own inline
node/edge string assembly rather than a shared structured form. Generalizing to all of them in one
increment meant either refactoring all three (a real, broader change than "add one SVG artifact"
asked for) or parsing the already-rendered Mermaid text back apart (fragile — couples correctness
to today's exact string format). Scoped to System Context instead: its node/edge data was cleanly
extractable into a small shared helper (`system_context_graph`) reused by both the existing Mermaid
text renderer and the new SVG renderer, so the two can't silently drift on which technologies
qualify. The new SVG primitive itself takes plain `(id, label)`/`(from, to)` tuples — no `KirId`,
no Mermaid syntax — so wiring it into the other three diagram producers is real, concretely scoped
follow-on work, not a redesign.

### Why hand-rolled SVG, not `mmdc`/a Mermaid-rendering crate

This project's own conventions (pure functions, zero `unsafe` without an RFC, no global mutable
state, reproducible builds) rule out shelling out to `mmdc`/Puppeteer — a Node.js + bundled headless
Chromium dependency is exactly the kind of large, non-reproducible external dependency this
project's Coding Rules exist to avoid — and no mature pure-Rust Mermaid-to-SVG renderer exists to
pull in instead. A small deterministic renderer over data `docs-gen` already computes is the same
choice this crate already made for everything except the opt-in `--prose` path: no LLM, no
interpretation, no external process.

### Design

- `layer_nodes`: Kahn's-algorithm topological levels (BFS layers), ties broken by node id for
  determinism. Nodes in a cycle never reach in-degree 0 through the main loop — appended as one
  final sorted layer instead of being dropped, proven by a dedicated cycle test (2 nodes, mutual
  edges → both still render).
- `render_graph_svg`: fixed-size boxes, each layer horizontally centered, straight edges with a
  shared arrowhead marker, XML-escaped labels (`svg_escape`, deliberately separate from
  `mermaid_escape_label` — different syntax, different escaping rules). Canvas size computed from
  the actual widest layer and layer count.
- `render_system_context_svg`: reuses `system_context_graph`, `None` under the exact same
  honest-empty condition the text renderer already had — no SVG file written for nothing.
- Wired into `generate_curated` (writes `system-context.svg` conditionally) and
  `render_architecture` (links it from `## System Context` only when real data backs it).

### Live verification

Reused this repo's own already-committed ledger — no new pipeline run needed. `ekos docs generate
--layout curated` produced a real `system-context.svg`: 46 `<rect>` (1 System node + 45 real
technologies), 45 `<line>` edges, confirmed well-formed XML via `xml.etree.ElementTree.parse`, and
confirmed `Architecture.md` links to it. Found and recorded a real, honest limitation in the same
pass: with 45 technologies the diagram lays out as one very wide row (8296×190px) rather than
wrapping — correct and valid, just wide. Tracked as explicit follow-on work in RFC 0073/TODO.md
rather than silently fixed or silently ignored.

---

## Knowledge Captured

- **A generic rendering primitive is worth factoring out even when only one caller needs it yet** —
  `render_graph_svg` takes plain string tuples specifically so it has zero coupling to this
  session's one integration (System Context), making the next three integrations (per-object
  neighborhood, Crate & Workspace Topology, per-kind Dependency Graph subsections) a wiring task
  against an already-tested primitive, not a redesign.
- **Deterministic layout needs an explicit tie-break, and an explicit answer for cycles** — a
  topological-layering algorithm is only reproducible if equal-rank nodes are ordered by something
  stable (here: node id), and only total if it has a defined behavior for inputs that never
  naturally reach in-degree 0. Untested, this is exactly the kind of thing that "works" on every
  hand-checked example and then silently drops a node the first time a real graph has a cycle.
- **Live verification against this repo's own already-committed data found a real, worth-recording
  scaling limitation** (wide unwrapped rows at 45 nodes) that no unit test with a handful of
  synthetic nodes would have surfaced — the same "verify against real data, not just fixtures"
  discipline this session has followed throughout, catching something concrete rather than
  hypothetical.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0073-system-context-svg-artifact.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Status note for this increment |
| `ekos/crates/docs-gen/src/lib.rs` | `system_context_graph`, `render_system_context_svg`, `render_graph_svg`, `layer_nodes`, `svg_escape`; System Context section links the SVG conditionally; 8 new tests |
| `ekos/crates/cli/src/commands/docs.rs` | `generate_curated` writes `system-context.svg` conditionally; 2 new tests |
| `TODO.md` | RFC 0068 §61 MVP fully done; next-step pointer updated to §62 Phase 2 |
| `devlogs/devlog_76.md` | This file |

# RFC 0073 — System Context SVG Artifact

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0068 §61 (MVP scope) lists six documentation-view items; five shipped across Increments 1-3
(RFC 0069-0071). The sixth and last, in TODO.md's own words: "current output is Mermaid-in-Markdown
only ... isn't a standalone SVG artifact `docs-gen` produces." This RFC ships that artifact for the
first of `docs-gen`'s Mermaid `graph TD` diagrams — System Context — via a new generic, reusable
rendering primitive, closing out the last open RFC 0068 §61 MVP item.

## Scoped to one diagram, deliberately, not all of them

`docs-gen` currently produces several `graph TD` diagrams from several different call sites
(`render_mermaid_graph` — per-object neighborhood, `render_system_context`, and
`render_relationship_kind_graph` — used for `## Crate & Workspace Topology` and any per-kind
`## Dependency Graph` subsection), each with its own inline node/edge string-building rather than a
shared structured representation. Generalizing SVG output to every one of them in this increment
would mean either refactoring every call site to first build a structured `(nodes, edges)` list (a
real, broader change touching functions this session didn't design, not warranted by "produce one
SVG artifact") or writing a Mermaid-text parser to reconstruct that structure from the already-
rendered fenced blocks (fragile — couples the SVG renderer's correctness to the exact string format
three different producer functions happen to emit today).

Scoped instead to System Context: it already had exactly the node/edge data the new renderer needs,
extracted once into a shared helper (`system_context_graph`) so the existing Mermaid-text renderer
and the new SVG renderer provably can't drift apart on which technologies qualify. The generic
renderer itself (`render_graph_svg`, taking plain `(id, label)` nodes and `(from_id, to_id)` edges)
is intentionally decoupled from `KirId`/Mermaid syntax entirely, so wiring it into the other three
diagram producers is real, concretely scoped follow-on work — the same "each needs its own
extraction into structured data first" judgment call, not a blanket mechanical change — tracked in
TODO.md rather than silently narrowing what "SVG/diagram generation" means.

## Design

**Why hand-rolled SVG, not a Mermaid renderer or `mmdc`**: this project's own conventions (Rust
2024, zero `unsafe` unless RFC-justified, no global mutable state, pure functions, reproducible
builds — CLAUDE.md) rule out shelling out to `mmdc`/Puppeteer (Node.js + a bundled headless Chromium
is a large, non-reproducible, environment-dependent external dependency for a CLI/CI tool), and no
mature pure-Rust Mermaid-to-SVG renderer exists to depend on either. A small, deterministic,
dependency-free SVG renderer over `docs-gen`'s own already-computed graph data fits this crate's
existing "pure rendering over compiled data, zero external calls" ethos exactly — the same reasoning
that already ruled out an LLM in this crate for anything but the opt-in `--prose` path.

**Layout — `layer_nodes`**: Kahn's-algorithm topological levels (BFS layers): layer 0 is every node
with in-degree 0, each later layer is nodes whose predecessors are all already placed, ties within a
layer broken by node id (lexicographic) so a given graph always lays out identically — required for
a deterministic, reproducible-build-compatible renderer. Nodes that never reach in-degree 0 (a
cycle) are appended as one final sorted layer rather than dropped, so `layer_nodes` always places
every input node exactly once regardless of graph shape — proven by a dedicated cycle test, not just
assumed for the (acyclic, star-shaped) System Context case it was designed for.

**Rendering — `render_graph_svg`**: fixed-size boxes (`160×40`), each layer horizontally centered,
straight-line edges with a shared `<marker>` arrowhead, XML-escaped labels (`svg_escape`, distinct
from `mermaid_escape_label` — different escaping rules for different syntaxes). Canvas size is
computed from the widest layer and layer count — real, not estimated. Takes generic `(String, String)`
node/edge tuples, so it has zero dependency on `KirId`, `ObjectKind`, or Mermaid syntax — a reusable
primitive, not System-Context-specific.

**Wiring**: `render_system_context_svg(objects, relationships) -> Option<RenderedPage>` reuses
`system_context_graph` — `None` under the exact same "no crates, no technologies, or no real
`DependsOn` edge" condition `render_system_context`'s text fallback already used, so no SVG file is
ever written for a diagram with nothing real to show. `generate_curated`
(`crates/cli/src/commands/docs.rs`) writes it conditionally alongside the four curated files, and
`render_architecture` links to it from the `## System Context` section only when
`system_context_graph` returns `Some` — the same conditional-link pattern the Component View section
already uses for rollup pages.

## What this does and doesn't fix

**Fixes**: a real, standalone, valid SVG artifact for System Context — the one RFC 0068 §61 item
this session's own tracking called out as the last open MVP piece.

**Does not (yet)**: apply SVG output to the other three `graph TD` diagram producers
(`render_mermaid_graph`'s per-object neighborhood, `render_relationship_kind_graph`'s Crate &
Workspace Topology / Dependency Graph), or to the `erDiagram`/`sequenceDiagram` families, which use
entirely different Mermaid syntax and would need their own (still generic, still Mermaid-independent)
node/edge extraction. Left as explicit, concretely scoped follow-on work in TODO.md, not silently
dropped.

**A known real limitation, not a bug**: layers aren't wrapped — a System Context diagram with many
technologies (this repo's own real ledger has 45) renders as one very wide row rather than wrapping
into a grid. Confirmed live: the real diagram against this repo's own committed ledger is
8296×190px, well-formed and correct, just wide. Left as a real, explicitly tracked follow-on
(`layer_nodes`/`render_graph_svg` would need a max-nodes-per-row wrap rule), not fixed here since it
wasn't part of what MVP scope asked for and the current output is honest and functionally correct.

## Testing

- `docs-gen` unit tests: `render_graph_svg` on empty input (empty string, not a malformed/empty SVG
  file); label escaping + layer placement with a real root/children graph; every node placed exactly
  once even with a genuine cycle (the `layer_nodes` fallback path). `render_system_context_svg`:
  `None` without real dependency data; `Some` with a real `<svg>...</svg>` document containing the
  expected node labels and marker reference, given real Crate/Technology/`DependsOn` fixtures.
  `render_architecture`: links `system-context.svg` only when real data exists, omits the link link
  otherwise.
- `cli` (`crates/cli/src/commands/docs.rs`) integration-style tests: `generate_curated` writes
  `system-context.svg` to disk and links it from `Architecture.md` given real Crate/Technology/
  `DependsOn` ledger data; omits the file entirely (not an empty one) when there's none — the
  existing "exactly four named files and nothing else" test stays green unmodified, the same
  conditional-file pattern the entity-pages/ER-diagram code already established.
- Live, real end-to-end: reused this repo's own already-committed ledger (no new pipeline run
  needed, following this session's established preference) — `ekos docs generate --layout curated`
  produced a real `system-context.svg` (46 `<rect>`, 45 `<line>` — 1 System node + 45 real
  technologies), confirmed well-formed via `xml.etree.ElementTree.parse`, and confirmed
  `Architecture.md` links to it.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0073-system-context-svg-artifact.md` | This RFC |
| `ekos/crates/docs-gen/src/lib.rs` | `system_context_graph` (extracted shared helper), `render_system_context_svg`, `render_graph_svg`, `layer_nodes`, `svg_escape`; `render_architecture`'s System Context section links the SVG when real data exists; 8 new unit tests |
| `ekos/crates/cli/src/commands/docs.rs` | `generate_curated` writes `system-context.svg` conditionally; 2 new tests |
| `TODO.md` | RFC 0068 §61 MVP item marked done; next-step pointer updated |
| `devlogs/devlog_76.md` | This increment's devlog |

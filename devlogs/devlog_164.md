# Devlog 164 — RFC 0136: Web Console Phase 6 (graph v2)

**Date:** 2026-09-05
**Branch:** `main` (direct — local-tests-only + `[skip ci]`, per standing maintainer direction)
**RFC:** `ekos/docs/rfcs/0136-web-console-phase-6-graph-v2.md`
**Related:** continues the tech-debt paydown pass (`devlog_163`); the user then directed
continuing autonomously into RFC 0127's Phase 6 specifically.

---

## Summary

RFC 0127 §10 named Phase 6 as "neighbourhood isolation, impact mode, server-side ForceAtlas2,
PNG/glTF export" (RFC 0134 already shipped the graph-time-travel slice of Phase 6 for itself).
This devlog covers the remaining three items — all now live: neighbourhood isolation and impact
mode both proxy existing, unmodified MCP tools (`ekos_neighborhood`/`ekos_impact`, RFC 0018) the
web console simply never called before; server-side layout uses `networkx` + `fa2_modified`
(Python's real ForceAtlas2 equivalent to RFC 0127's "graphology + ForceAtlas2" line, which named a
JavaScript-only library); PNG/glTF export is entirely client-side. **The single largest finding of
this pass: zero Rust-side changes were needed anywhere.** Every piece of Phase 6 lives in `web/api`
+ `web/ui` only.

---

## PR — RFC 0136: Graph v2

### Problem / motivation

Phase 5 (RFC 0133) shipped a real, working graph view — LOD 0/1, orbit/zoom, kind filters, search
with fly-to. It stopped short of the three things RFC 0127 §9.3 called "the differentiating
screen" for the whole product (impact mode — "the visual form of the claim that currently has
none") and the two operational necessities named alongside it (isolating a neighbourhood instead of
staring at the whole graph; a layout that doesn't choke past ~2,000 nodes).

### What was built

| Component | Change |
|---|---|
| Neighbourhood isolation | `GET /workspaces/{id}/neighborhood/{object_id}?depth=1-3` proxies `ekos_neighborhood` directly. Frontend: `isolate` view-state replaces the normal `/graph` fetch with this real sub-graph (a depth slider, "back to full graph"). |
| Impact mode | `GET /workspaces/{id}/impact/{object_id}?direction=&max_hops=&kind=` proxies `ekos_impact`. Frontend: colors nodes by hop distance (a sequential scale, source outward), dims everything else, highlights edges already present in the loaded graph whose both endpoints are impacted. |
| Server-side layout | `POST /workspaces/{id}/graph/layout` — `networkx.Graph` + `fa2_modified.ForceAtlas2().forceatlas2_networkx_layout(...)`, `lru_cache`-memoized on `(sorted node ids, sorted edges)`. Frontend calls it once the fetched graph exceeds 2,000 nodes and pins the returned coordinates via the same `fx`/`fy` fields RFC 0134's own layout-freeze already uses. |
| PNG/glTF export | `graph-export.ts` — `canvas.toBlob()` for PNG; a hand-built minimal valid glTF 2.0 document (`POINTS` + `LINES` primitives, embedded base64 buffer) for glTF. Both entirely client-side, no backend call. |

### Implementation details worth remembering

- **The Rust-changes-needed count for this whole phase is zero.** `ekos_neighborhood` and
  `ekos_impact` already return exactly what the console needed (`ekos_neighborhood`: a real
  `KirGraph` with objects *and* relationships, not just an id list — no client-side edge
  reconstruction needed). Worth checking "does an existing MCP tool already do this" before
  assuming a web-console phase needs new compiler/ledger surface.
- **`ekos_impact`'s actual shape doesn't support a precise edge trace.** Its `hops` array is
  `{hop, id, name, kind, via}` — `via` is the *relationship kind* string, not the specific parent
  node. Building a byte-perfect edge-by-edge trace would need a new Rust traversal primitive
  returning parent edges. Decided not to build that primitive speculatively — node-hop coloring
  plus "highlight real loaded edges between two impacted nodes" is a defensible, honestly-scoped
  approximation (an Open Question in the RFC, not a silently-accepted gap).
- **RFC 0127's "graphology + ForceAtlas2" line named a JavaScript library from a Python backend.**
  Checked the real Python ecosystem instead of assuming a translation: `networkx` (already the
  natural graph representation) + `fa2_modified` (a maintained fork of the original `fa2` package,
  confirmed `pip`-installable via `uv add`, its exact
  `forceatlas2_networkx_layout(G, iterations=N) -> {id: (x, y)}` API verified with a live run
  *before* writing any route code, not assumed from the package name alone).
- **`ObjectKind`/`RelationshipKind`'s `Custom(String)` variant serializes untagged** — plain string
  (`"Technology"`), never `{"Custom": "Technology"}`. Confirmed by calling `ekos_state` live over a
  raw MCP request rather than trusting a serde-derive assumption about tuple-variant JSON shape;
  this is *why* `ObjectPanel.tsx`'s existing `{o.kind}` rendering already worked correctly for
  every kind without special-casing, and why the new `neighborhoodToGraph` conversion doesn't need
  to unwrap anything either.
- **glTF requires every mesh to have ≥1 primitive.** An empty `primitives: []` (the naive way to
  handle "no edges to export") is invalid glTF, not just an empty mesh — the exporter omits the
  "relationships" mesh/node/accessor/bufferView entirely when there are no edges, rather than
  emitting a technically-invalid empty one.
- **A pinned `fx`/`fy` on a node before the simulation starts is enough to skip d3-force's
  simulation for that node** — no `GraphCanvas` change needed for server-layout to "just work";
  it's the same mechanism RFC 0134's `onEngineStop` already uses to freeze a client-settled layout,
  just applied before the simulation starts instead of after.

### Decisions (alternatives considered, why this choice)

- **PNG/glTF export client-side, not server-rendered.** The graph a user wants to export is
  *the one on their screen* — their zoom, pan, filters, isolate/impact state. Reproducing that
  server-side would mean shipping the whole view-state to the backend and running a headless
  renderer for a strictly worse result than reading the pixels already there.
- **`lru_cache` over a pure function, not a persistent layout cache.** RFC 0127 §9.4 asked for
  caching "per (workspace, ledger generation, filter set)." A layout is a pure function of graph
  structure alone — an in-process memo keyed on the sorted node/edge set is exactly that cache's
  correctness boundary, and losing it on restart is a recompute cost, never a correctness one
  (same principle the search index and RFC 0134's union-fetch already rely on). A persistent,
  cross-restart cache is real, separate scope (RFC 0136 §6 Non-goals) not justified by anything
  found while building this.
- **Impact mode's own hop-map treats the source object as hop 0**, added explicitly rather than
  left absent — `ekos_impact`'s `hops` array only lists what was *reached*, so without this the
  source node itself would render as "not impacted," which reads as a bug on first look.

### Verification

99 Python tests pass (20 new: 8 pure-function layout unit tests, 8 route unit tests via the
existing `RecordingMcp` fake pattern, 4 live tests against this repo's own compiled `.ekos/`
workspace — real neighbourhood/impact/layout responses, not mocked). `ruff check`/`ruff format
--check` clean. `tsc -b --noEmit` and `vite build` both clean (this project's only frontend gate —
no test runner exists in `web/ui`). Confirmed Vite's dev server transforms and serves every new
module (`Graph.tsx`, `GraphCanvas.tsx`, `graph-export.ts`, `graph-v2.ts`) without error.

**Not done: a real browser check.** No browser tooling was available this session (`claude-in-chrome`
reported the extension isn't connected). This is disclosed rather than assumed away — the
verification above is real and thorough for what it covers (server-side correctness, type safety,
build integrity), but graph rendering, click interactions, and the two export downloads have not
been visually confirmed working end to end in an actual browser.

---

## Knowledge Captured

- **Before assuming a web-console feature needs new compiler/ledger surface, check what the
  existing MCP tool catalog already returns.** Both `ekos_neighborhood` and `ekos_impact` predate
  the web console by a long way (RFC 0018) and already returned exactly the right shape for two of
  Phase 6's four items — the "phase" work was entirely wiring, not new capability.
- **An RFC naming a specific library by name should be verified against the real target
  runtime, not assumed transferable.** RFC 0127 named "graphology" (JavaScript) for a Python
  FastAPI backend — a real mismatch that would have blocked implementation if not caught during
  design rather than mid-coding.
- **`uv add <package>` is the right way to add a web/api dependency in this repo** — updates
  `pyproject.toml` and `uv.lock` together, matches what `uv sync --frozen` in CI expects. A plain
  `pip install` inside the venv (tried first, out of habit) silently installs to the wrong
  location when the venv itself isn't the active `pip`'s target.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0136-web-console-phase-6-graph-v2.md` | New RFC |
| `web/api/pyproject.toml`, `web/api/uv.lock` | `networkx`, `fa2-modified` dependencies |
| `web/api/app/layout.py` | New — pure ForceAtlas2 layout function + `lru_cache` |
| `web/api/app/routes/graph.py` | `neighborhood`, `impact`, `graph_layout` routes |
| `web/api/app/schemas.py` | `LayoutIn`/`LayoutOut` |
| `web/api/tests/test_layout_unit.py` | New — 8 pure-function tests |
| `web/api/tests/test_graph_unit.py` | 8 new route unit tests (`RecordingMcp` fake) |
| `web/api/tests/test_graph_live.py` | 4 new live tests against this repo's own `.ekos/` |
| `web/ui/src/pages/graph-shared.ts` | `fx`/`fy` on `GNode`, `SERVER_LAYOUT_THRESHOLD`, `impactColorFor` |
| `web/ui/src/pages/graph-v2.ts` | New — neighbourhood/impact raw-KIR-shape conversions |
| `web/ui/src/pages/graph-export.ts` | New — client-side PNG/glTF export |
| `web/ui/src/pages/GraphCanvas.tsx` | `forwardRef` + `GraphCanvasHandle`, impact-mode coloring/edge highlighting |
| `web/ui/src/pages/Graph.tsx` | isolate/impact view-state, server-layout wiring, export toolbar |
| `web/ui/src/pages/ObjectPanel.tsx` | "isolate neighbourhood" / "what depends on this" / "what this depends on" actions |
| `TODO.md` | RFC 0136 item marked done; Phase 7 hardening left as its own open item |

# RFC 0136 — Web Console Phase 6: Graph v2 (neighbourhood isolation, impact mode, server-side layout, export)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-05
**Phase 6 of:** RFC 0127 (§9.3, §9.4, §10) · **builds on:** RFC 0133 (Phase 5 graph view, LOD 0/1),
RFC 0134 (Phase 6 slice: graph time-travel — the timelapse slider)
**Numbering:** assigned 2026-09-04 during the tech-debt paydown planning pass, after RFC 0134's own
"→ RFC 0135+" placeholder went stale (0135 was claimed by the core-provenance RFC the same day).
**Depends on:** `ekos_neighborhood` (RFC 0018), `ekos_impact` (RFC 0018), `ekos_graph_export`
(RFC 0127 R1/R3) — all three already shipped, unmodified by this RFC.

---

## 1. Scope

RFC 0127 §10 named Phase 6 as: "Neighbourhood isolation, impact mode, server-side ForceAtlas2,
PNG/glTF export." This RFC covers the three items RFC 0134 didn't already ship (that RFC took the
graph-time-travel slice of Phase 6 for itself, per its own "Defers" line).

**A genuine scope finding, not assumed going in:** none of this needs a single Rust-side change.
`ekos_neighborhood` and `ekos_impact` are existing, unmodified MCP tools (RFC 0018) the CLI/agent
side has used for a long time — the web console simply never called them yet. Server-side layout
and image/model export are pure console-layer (Python + browser) concerns; nothing about them
needs new data out of the compiler or ledger. This is entirely a `web/api` + `web/ui` change.

## 2. Neighbourhood isolation

**What:** clicking "isolate neighbourhood" on a selected object replaces the current graph view
with exactly that object's BFS neighbourhood at a chosen depth (1–3, a slider) — everything else
hidden, not just dimmed.

**Backend:** `GET /workspaces/{id}/neighborhood/{object_id}?depth=N` proxies `ekos_neighborhood`
directly — same pass-through philosophy `GraphOut`'s own docstring states ("kept permissive on
purpose"). `ekos_neighborhood` already returns a real sub-`KirGraph` (objects *and*
relationships, not just an id list), so the console needs no client-side edge reconstruction: the
returned relationships are the real edges to draw.

**Frontend:** a new `isolatedId: string | null` + `isolatedDepth: number` pair of view-state
fields. When set, `Graph.tsx` fetches this endpoint instead of `/graph`, and renders the returned
graph directly (mapped through the same `GNode`/`GLink` shape `graph-shared.ts` already defines —
object-level nodes, no aggregate/super-node concept applies here). A depth slider (1–3) and a
"back to full graph" button that clears `isolatedId`.

## 3. Impact mode

**What:** `ekos_impact` renders as a highlighted directed trace over the *currently loaded* graph
— per RFC 0127 §9.3, "the visual form of the claim that currently has none."

**Backend:** `GET /workspaces/{id}/impact/{object_id}?direction=dependents|dependencies&max_hops=N&kinds=K1,K2`
proxies `ekos_impact` directly.

**A real shape constraint, found reading `ekos_impact`'s actual output, not assumed:**
`ekos_impact` returns `hops: [{hop, id, name, kind, via}]` — `via` is the *relationship kind*
string that connected each hit, not the specific parent node it came from. It is a real hop-depth
node list, not an edge-path list. Building a precise edge-by-edge trace would need a new,
separate traversal primitive on the Rust side — out of scope for a console-layer RFC that found
it needs zero Rust changes everywhere else.

**Design decision:** render impact mode as **node highlighting by hop distance** (a sequential
colour scale from the source outward) over the graph already on screen, dimming everything not in
the impacted set — and additionally **highlight edges** already present in the loaded graph
whose two endpoints are *both* in the impacted set. This second part is a real, not-approximated
signal: it only lights up edges the console already has real evidence for (an edge that survived
`ekos_graph_export`'s own filters), it just doesn't attempt to reconstruct which *specific* hop
transition each edge belongs to. Precise edge-path attribution is a real, separate future
increment if it turns out to matter in practice (Open Question below) — not guessed at here.

**Frontend:** `impactId: string | null` + `impactDirection` view-state. When set, fetch the impact
route, build a `Map<nodeId, hop>` from the response, and pass it into `GraphCanvas` to drive node
colour (hop-scale) and edge highlighting (both endpoints present in the map).

## 4. Server-side ForceAtlas2 layout

**What:** RFC 0127 §9.4 named the threshold precisely: client-side force simulation only holds up
below ~2,000 nodes. Above that, the console precomputes positions server-side and returns fixed
coordinates.

**Library choice, verified live before committing:** RFC 0127's own text said "graphology +
ForceAtlas2" — graphology is a *JavaScript* graph library, not usable from the Python console
backend. Checked the real Python ecosystem instead of assuming: `networkx` (already the natural
graph representation) + **`fa2_modified`** (a maintained fork of the original `fa2` package,
`pip`-installable, no C-toolchain surprises) provides
`ForceAtlas2().forceatlas2_networkx_layout(G, iterations=N) -> {node_id: (x, y)}` — confirmed by
running it against a real small graph before writing any route code.

**Backend:** `POST /workspaces/{id}/graph/layout`, body `{"nodes": ["id1", "id2", …], "edges":
[["id1", "id2"], …]}` (ids only — the console already has full node/edge objects from the prior
`/graph` call; no need to re-fetch or re-send them). Builds a `networkx.Graph`, runs
`fa2_modified`, returns `{"positions": {"id1": [x, y], …}}`.

**Caching, matching §9.4's "cached per (workspace, ledger generation, filter set)" intent without
inventing new ledger-generation plumbing:** `functools.lru_cache` over a pure function keyed on
`(sorted tuple of node ids, sorted tuple of edge pairs)` — the layout is a deterministic function
of graph structure alone, so this key is exactly the cache's correctness boundary: two requests
with the same node/edge set always get the same (cached) answer, and any real change to the graph
(a recompile, a filter change) changes the key naturally. In-process only, no persistence across a
restart — layout is a derived, freely-recomputable artifact (the same principle the search index
and the RFC 0134 graph-time-travel union fetch already rely on), so losing the cache on restart is
a performance cost, never a correctness one.

**Frontend:** when the fetched graph's node count exceeds `SERVER_LAYOUT_THRESHOLD` (2,000, per
§9.4), `Graph.tsx` calls the layout endpoint after loading the graph and passes the returned fixed
positions into `GraphCanvas` as pinned coordinates (`fx`/`fy`, the same field `onEngineStop`
already uses to freeze the client-side layout post-simulation in RFC 0134) — the renderer skips
its own force simulation entirely when positions are supplied, matching `react-force-graph`'s own
documented behavior for pre-positioned nodes.

## 5. PNG / glTF export

**What:** let a user save the current graph view as a static image or a 3D-tool-importable model.

**Design decision:** both are **entirely client-side**, no backend involvement. `GraphCanvas`
already renders to an HTML5 `<canvas>` (`react-force-graph-2d`); the canvas element's own
`toBlob()` gives a real PNG with zero new dependencies. glTF is a minimal, hand-built valid glTF
2.0 document (`POINTS` primitive for nodes with `POSITION`/`COLOR_0` accessors, `LINES` for edges)
built directly from the same in-memory node/link data already driving the canvas — not a full
scene-graph exporter, since the source is a flat 2D point-and-line graph, not a modeled 3D scene.
Both trigger a real browser download in the deployed console (`web/ui`, not an Artifact preview,
so no download-sandboxing concern applies here).

**Why not server-side PNG rendering:** the graph the user wants to export is *the one currently on
their screen* — their zoom, their pan, their filters, their isolate/impact-mode state. Reproducing
exactly that server-side would mean re-sending the entire view-state and re-running a headless
renderer for a result strictly worse than "read the pixels already on screen."

## 6. Non-Goals

- **Precise edge-level impact-path attribution** (§3) — needs a new Rust-side traversal primitive
  that returns parent edges, not just hop-depth nodes. Real, separate, not attempted here.
- **A persistent, cross-restart layout cache** (§4) — an `lru_cache` is enough for the stated
  problem (repeated views of the same filtered graph within one console session); a database- or
  disk-backed cache is real, separate scope with its own invalidation design, not justified by
  anything found while building this.
- **3D export of the 2D graph as a modeled scene** — the glTF export is a flat point/line
  representation of the current 2D layout (z=0), not a re-derived 3D force-directed scene. Fine
  for "open this in Blender/a glTF viewer to look at the shape"; not a return to RFC 0127 §9.2's
  original (later revised away) 3D rendering plan.

## 7. Open questions

1. Does precise edge-path attribution for impact mode turn out to matter once real users try it?
   If node-hop highlighting plus "edges between two impacted nodes" reads as good enough in
   practice, the Rust-side traversal primitive Non-Goal above never needs building.
2. Is 2,000 nodes the right server-layout threshold on real hardware, or should it be configurable
   per-deployment? Left at the RFC 0127-specified default; revisit with real usage.

## 8. Verification

- Python: unit tests (a `RecordingMcp` fake, matching `test_graph_unit.py`'s existing pattern) for
  the two new proxy routes' argument-forwarding and response shape; a real, non-mocked test of the
  layout endpoint against `fa2_modified` (no MCP involved — pure graph-structure input); live tests
  (matching `test_graph_live.py`'s pattern, gated on `$EKOS_BIN`) against this repo's own real
  compiled `.ekos/` for neighbourhood/impact end to end.
- TypeScript: `tsc -b --noEmit` + `vite build` (this project's existing, only frontend gate — no
  test runner exists in `web/ui` yet, unchanged by this RFC).

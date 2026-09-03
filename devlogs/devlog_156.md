# Devlog 156 — RFC 0133: Web Console Phase 5 (graph view, LOD 0/1)

**Date:** 2026-09-03
**Branch:** `feat/0133-web-console-phase-5` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0133-web-console-phase-5.md`

---

## Summary

`ekos graph export` / `ekos_graph_export` shipped in Phase 0 with nothing that drew them. Phase 5
draws them: an overview as one super-node per object kind, click-to-expand one kind into its real
objects, kind / relationship-kind filters, a search that flies the camera to a node, and a side
panel showing an object's full `ekos_state` plus the resolved evidence behind every claim.

**Decisions (maintainer):** 2D `react-force-graph-2d` (not the three.js `-3d` build RFC 0127 §9.2
named — ~⅓ the bundle, readable labels); scope = LOD 0 + LOD 1 + filters + search + object panel.
Neighbourhood isolation, impact-mode trace, server-side layout, and glTF/PNG export are Phase 6.

---

## PR — Phase 5

### API — one new endpoint

| Endpoint | Backed by |
|---|---|
| `GET /api/workspaces/{id}/objects/{object_id}` | `ekos_state` MCP tool — object + relationships + resolved evidence; `404` on a bad id |

Plus `include_properties: bool` passed through on the existing `/graph`. Everything else Phase 5
needs (`/graph` with `level`/`group_by`/`kind`/`exclude_rel_kind`/`min_degree`/`max_nodes`,
`/search`) already existed.

### Frontend

| File | Role |
|---|---|
| `pages/graph-shared.ts` | Dependency-free types (`GNode`/`GLink`) + the kind→colour palette — so `Graph.tsx` imports it statically without pulling in the renderer |
| `pages/GraphCanvas.tsx` | The `ForceGraph2D` wrapper. **Lazy-loaded** — `React.lazy(() => import("./GraphCanvas"))` — a separate 177 KB / 58 KB-gzipped chunk; the entry bundle is unchanged from Phase 4 |
| `pages/Graph.tsx` | The `/w/:id/graph` route: LOD state, `exclude_rel_kind` filter set (default off: `CoupledWith`, `FeedsInto`), `min_degree` slider, search, selection |
| `pages/ObjectPanel.tsx` | The right-hand panel — properties, relationships (each links to the other object), and one evidence row per claim (path:line · analyzer · confidence · fragment) |

**LOD 0** → `graph?level=aggregate&group_by=kind`: super-nodes sized by `count`, weighted group
edges. **LOD 1** → click a super-node → `graph?level=object&kind=<K>&max_nodes=500&min_degree=<n>`
in place. `truncated` renders as a "showing the 500 most-connected of N" banner.

---

## Verification

Local gates only (`[skip ci]`):

- Rust: untouched (`ekos_state` already exists).
- `web/api`: `ruff` clean, `pytest` **72/72** with `EKOS_BIN` (4 new — aggregate→expand a kind,
  `/objects/{id}` returns the evidence shape, `404` for a bogus id, `include_properties` accepted).
- `web/ui`: `tsc -b --noEmit` clean, `vite build` succeeds — **`GraphCanvas` is a separate lazy
  chunk** (177 KB), entry chunk 661 KB (≈ Phase 4).
- End to end against this repo: `/w/self/graph` renders the 16 kind super-nodes; clicking
  `RustSymbol` expands to its 500 most-connected; unchecking `CoupledWith` visibly thins the
  graph; searching `ledger` flies to the node; its panel shows the `rust_analyzer` evidence row.

---

## Knowledge Captured

- **The Phase 0 MCP client's NDJSON reader had a 64 KiB line limit** (asyncio's
  `StreamReader` default). It never surfaced because every test until now used tiny fixture
  workspaces; the first real `ekos_search` / object-level `ekos_graph_export` over this repo's
  ledger produced a >64 KiB response line and `readline()` raised *"Separator is not found, and
  chunk exceed the limit"* → HTTP 500. Fixed: `open_connection(..., limit=64 MiB)` in
  `mcp_client.py`, plus a regression test that expands a large kind.
- **A `React.lazy` boundary is defeated by any static import from the same module.** `Graph.tsx`
  originally did both `const GraphCanvas = lazy(() => import("./GraphCanvas"))` **and**
  `import { colorFor } from "./GraphCanvas"` — the static one pulled `react-force-graph-2d` into
  the entry chunk (bundle jumped 650 → 840 KB, only one JS file emitted). Move the shared
  non-heavy bits (`colorFor`, types) to their own module.
- **`ekos_graph_export` node `k` indexes that response's own `kind_index`, not a global one.**
  Each call returns only the kinds present. Fine for LOD 1 (single kind → uniform colour); for a
  stable cross-workspace palette you'd need a fixed table (Phase 7 nicety).
- **Edges are `{s, t}` = indices into the node array**, not ids. Map to
  `{source: nodes[s].id, target: nodes[t].id}` for `react-force-graph`.
- **`ObjectState` serializes as `{object, relationships, evidence}`** — `object` is a full
  `KirObject` (so `object.id`, `object.kind`, `object.properties`), `evidence` entries carry a
  `location`/`source_location` + `fragment`/`excerpt` + `analyzer` + `confidence` (field names
  vary by analyzer — the panel checks both spellings).

---

## Files Changed

| File | Change |
|---|---|
| `web/api/app/routes/graph.py` | `GET /objects/{id}` (ekos_state); `include_properties` on `/graph` |
| `web/ui/src/pages/{graph-shared.ts,GraphCanvas.tsx,Graph.tsx,ObjectPanel.tsx}` | New — the view |
| `web/ui/src/{main,pages/Dashboard}.tsx` | Route + link |
| `web/ui/src/index.css` | Graph layout + panel styles |
| `web/ui/package.json` | `react-force-graph-2d` |
| `web/api/tests/test_graph_live.py` | New |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 5 note |

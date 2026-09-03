# RFC 0133 — Web Console Phase 5: graph view (LOD 0/1)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-03
**Phase 5 of:** RFC 0127 (§9, §10) · **builds on:** RFC 0128 R1/R3 (`ekos graph export` /
`ekos_graph_export`), RFC 0129 (Phase 1 endpoints), RFC 0131 (auth)
**Defers:** RFC 0127 Phase 6 (neighbourhood isolation, impact mode, server-side ForceAtlas2,
PNG/glTF export), Phase 7 (hardening) → RFC 0134+

---

## Motivation

`ekos graph export` and `ekos_graph_export` have existed since Phase 0 with nothing that draws
them. Phase 5 draws them: an overview of the whole compiled graph as one super-node per kind, the
ability to expand one kind into its real objects, kind / relationship-kind filters, a search that
flies to a node, and a side panel that shows an object's full state and the evidence behind it.

**Decisions locked before this RFC** (from the maintainer):

- **2D, not 3D.** `react-force-graph-2d` (canvas, same library family) instead of the three.js
  `-3d` build RFC 0127 §9.2 named — ~⅓ the bundle, readable labels, intuitive pan/zoom, no camera
  disorientation. This revises §9.2; the "3D requirement" was never load-bearing.
- **Phase 5 = LOD 0 + LOD 1 + filters + search + object panel.** Neighbourhood isolation, the
  impact-mode trace, server-side layout, and glTF/PNG export stay in Phase 6 (RFC 0127 §10).

**Not in this RFC:** anything that writes, path-prefix *expansion* (the overview can group by
path prefix, but LOD-1 expansion is kind-only — `ekos_graph_export` filters objects by kind, not
by path prefix), graphs above the client-side force-sim ceiling (the node budget keeps LOD 1
under it; a bigger workspace shows a truncation banner, per R1's existing `truncated` block).

---

## 1. API — one new endpoint

Everything else Phase 5 needs already exists: `GET /workspaces/{id}/graph` (RFC 0129, proxies
`ekos_graph_export` with `level` / `group_by` / `kind` / `exclude_rel_kind` / `min_degree` /
`max_nodes` / `max_edges`) and `GET /workspaces/{id}/search` (`ekos_search`).

```
GET /api/workspaces/{id}/objects/{object_id}     # ekos_state — object + relationships + evidence
```

Proxies the `ekos_state` MCP tool (`runtime.reconstruct_state`). Returns the object's name, kind,
properties, its relationships (each with the other end's id/name/kind and the relationship kind),
and the resolved evidence for every claim — path, fragment, confidence, and the analyzer that
produced it. `404` if the id isn't a compiled object. Read role.

`GET /workspaces/{id}/graph` also gains an `include_properties: bool` query param, passed through
to the tool, so the panel-less hover tooltip can show a couple of key properties without a second
round trip.

---

## 2. Frontend — `web/ui/src/pages/Graph.tsx`

Route `/w/:id/graph`, linked from the dashboard. The heavy deps (`react-force-graph-2d` pulls in
`force-graph` + `d3-force` + `d3-*`, ~200 KB) are **lazy-loaded** — `React.lazy(() =>
import("./GraphCanvas"))` — so the rest of the console's bundle is unchanged.

### 2.1 Two levels of detail (RFC 0127 §9.3)

**LOD 0 — overview (default).** `graph?level=aggregate&group_by=kind`. One node per object kind
(`File`, `Table`, `RustSymbol`, `Custom(...)`, …), sized by `count`, plus weighted group edges.
Always < ~30 nodes; instant. A `group by kind ▸ path prefix` toggle re-fetches with
`group_by=path_prefix` (overview only — see §Non-goals).

**LOD 1 — expansion.** Clicking a kind super-node fetches
`graph?level=object&kind=<K>&max_nodes=500&min_degree=<slider>` and renders those real objects in
place of the super-node; every other kind stays collapsed. A second click (or an "collapse"
control) returns to LOD 0. The `truncated` block from R1 renders as a "showing the 500
most-connected of N" banner.

Node colour = kind (a fixed palette keyed by `kind_index`); node size = degree `d` (post-filter,
per R1); edge width = weight `w` at LOD 0, uniform at LOD 1.

### 2.2 Filters

Kind toggles (from the response's `kind_index`) and relationship-kind toggles (from
`rel_kind_index`), rendered as a checklist. `Custom("CoupledWith")` (co-change) and
`Custom("FeedsInto")` (pipeline wiring) are **off by default and shown as off** — the same call
`render_architecture` makes, and for the same reason (one Pentaho transform has 86 `FeedsInto`
edges). Toggling a relationship kind adds `exclude_rel_kind`; toggling a kind adds/removes it from
the `kind` list (LOD 1) or dims its super-node (LOD 0).

### 2.3 Search

The existing `/search` endpoint. Results are a list in the sidebar; clicking one:
- if the hit is a node in the current graph → centre the camera on it and pulse it;
- if not (wrong LOD, or filtered out) → expand its kind, then centre.

### 2.4 Object panel

Clicking an object node calls `GET /objects/{id}` and opens a right-hand panel:
- header: name, kind, id (copyable);
- **Properties** — the `properties` map, pretty-printed;
- **Relationships** — grouped by kind, each row linking to the other object (clicking re-centres /
  expands as needed);
- **Evidence** — one row per resolved claim: the source path + line, the fragment excerpt, the
  confidence, and the analyzer name. This is the payoff — "every conclusion carries the evidence
  it was derived from" made visible.

### 2.5 Layout + performance

Client-side force simulation only (`d3-force`, the library default). The LOD-1 node budget (500)
keeps it well under RFC 0127 §9.4's ~2 000-node client ceiling. Server-side ForceAtlas2 with
cached coordinates is Phase 6. Coordinates are not persisted in Phase 5 — a reload re-simulates
(acceptable at these node counts; Phase 6's fixed coords are the real fix).

---

## 3. Testing

**`web/api`**
- `GET /objects/{id}` — returns the `ekos_state` shape for a real compiled object (live,
  `EKOS_BIN`); `404` for a bogus id; read role required.
- `graph?include_properties=true` passes the flag through (unit, against the mock MCP client).

**`web/ui`**
- `tsc` + `vite build`; the graph chunk is a separate lazy bundle (assert it's not in the entry
  chunk).
- A render test of `Graph.tsx` with a mocked `/graph` (aggregate) response — the super-nodes
  render, a click triggers the object-level fetch, the filter checklist reflects `kind_index`.
- A render test of the object panel against a mocked `/objects/{id}` — evidence rows show the
  path, fragment, and analyzer.

---

## 4. Verification

- Rust: untouched (the `ekos_state` tool already exists).
- `web/api`: `ruff` + `pytest` green (unit + `EKOS_BIN`-gated `/objects/{id}`).
- `web/ui`: `tsc` clean, `vite build` succeeds, entry chunk size unchanged (graph is lazy).
- End to end against this repo: `/w/self/graph` renders the ~16 kind super-nodes; expanding
  `RustSymbol` shows the 500 most-connected symbols; filtering out `CoupledWith` visibly thins the
  graph; searching `Ledger` flies to the node; its panel shows the `sql_analyzer` /
  `rust_analyzer` evidence rows.

---

## 5. Files changed (projected)

| File | Change |
|---|---|
| `web/api/app/routes/graph.py` | `GET /objects/{id}` (ekos_state); `include_properties` on `/graph` |
| `web/api/app/schemas.py` | `ObjectStateOut` (permissive pass-through) |
| `web/ui/src/pages/Graph.tsx` + `GraphCanvas.tsx` (lazy) | New — the view |
| `web/ui/src/pages/ObjectPanel.tsx` | New — the side panel |
| `web/ui/src/{main,pages/Dashboard}.tsx` | Route + link |
| `web/ui/package.json` | `react-force-graph-2d` |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 5 note |

---

## 6. Open questions

1. **Path-prefix expansion.** LOD-1 expansion is kind-only because `ekos_graph_export` filters
   objects by kind, not path prefix. A `--path-prefix` filter on the R1 function (Rust) would
   unlock it; deferred until the overview's path-prefix grouping proves worth drilling into.
2. **Colour stability.** The kind→colour map is keyed by `kind_index` position, which is stable
   for a given workspace but not across workspaces. A fixed global kind→colour table is a small
   nicety for Phase 7.
3. **Deep-linking.** `/w/:id/graph?focus=<object_id>` (open expanded + panelled on one object)
   would make graph links shareable. Trivial to add once Phase 6's fixed layout lands.

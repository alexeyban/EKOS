# Devlog 157 — RFC 0134: Web Console Phase 6 (graph time-travel / timelapse slider)

**Date:** 2026-09-04
**Branch:** `rfc/0134-web-console-phase-6-graph-time-travel` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0134-web-console-phase-6-graph-time-travel.md`

---

## Summary

A slider under the graph. Drag it back and the diagram redraws as the knowledge existed at that
instant — nodes and edges appear as you scrub forward through the compile history; stop on any
date and a clicked node's panel shows the evidence it had *then*. The data model already
supported this (`all_objects_at` / `all_relationships_at`, `ekos_state`'s `at` param,
`ekos ledger timeline`); Phase 6 is one Rust parameter, two query params, and a frontend that
scrubs a monotonic graph client-side with a frozen layout.

This also closes the `--as-of` graph-export deferral that had been sitting in `TODO.md`.

**Decisions (from the design discussion):** ledger time, not source/world time; snap to
`ledger timeline` day-buckets, not a continuous scrub; client-side subset filtering off one
union fetch + a frozen layout (the graph is monotonic — nothing is ever deleted), with a
server `as_of` path for the object panel and CLI/MCP parity.

---

## PR — Phase 6

### Rust — `as_of` + `include_first_seen` on the graph export

All in `ekos_runtime::graph_export` (the one function `ekos graph export` and `ekos_graph_export`
share):

| Change | Detail |
|---|---|
| `GraphExportOptions.as_of: Option<DateTime<Utc>>` | `Some(t)` → `all_objects_at(t)` / `all_relationships_at(t)` instead of the unbounded reads. One `match` at the top; every downstream step (filter, degree, `min_degree`, truncation, aggregation) unchanged, so `counts` are naturally "as of `t`" |
| `GraphExportOptions.include_first_seen: bool` | Stamps each node/edge with `fs` — the object's / relationship's `created_at` at object level, the member **min** at aggregate level |
| `GraphExport.as_of` | Echoed back (`skip_serializing_if` when `None`) |
| CLI | `ekos graph export --as-of <rfc3339>` + `--first-seen` |
| MCP | `ekos_graph_export` gains `as_of` + `include_first_seen`, both in the tool schema |

`ekos_state` already took an `at` param (`reconstruct_state_at`) — no Rust change for the object
panel.

### web/api

| Endpoint | Change |
|---|---|
| `GET /workspaces/{id}/graph` | `as_of` + `include_first_seen` query params, passed straight through |
| `GET /workspaces/{id}/objects/{object_id}` | `as_of` query param → forwarded to `ekos_state` as `at` |
| slider ticks | **no new endpoint** — reuses `/stats/timeline` (already proxies `ekos ledger timeline --json`) |

`GraphOut.as_of` added; nodes/edges are raw dicts so `fs` rides through untouched.

### Frontend

| File | Role |
|---|---|
| `pages/GraphTimeline.tsx` | **New.** The slider: a range input over the `ledger timeline` day-buckets, an activity histogram behind the track (objects added per bucket), play/pause (steps bucket→bucket at 700 ms), a date label, "⤒ latest". Dependency-free — stays in the entry chunk |
| `pages/GraphCanvas.tsx` | `nodeVisibility` / `linkVisibility` keyed on `asOf` (`!asOf || fs <= asOf`); **frozen layout** — `onEngineStop` pins every node's `fx`/`fy` so scrubbing only toggles visibility and nodes never move; a `refresh()` nudge when `asOf` changes |
| `pages/Graph.tsx` | `asOf` state (null = live); the graph fetch always sends `include_first_seen=1` and **no** `as_of` (union = latest, since monotonic); a "viewing as of …" banner; passes `asOf` to canvas + panel |
| `pages/ObjectPanel.tsx` | `asOf` prop → `?as_of=` on the `ekos_state` fetch; "state as of <date>" in the header |
| `pages/graph-shared.ts` | `firstSeen` on `GNode`/`GLink`; `bucketEnd()` — day label → end-of-day RFC 3339 |

The GraphCanvas chunk stays a separate 178 KB lazy bundle; the entry chunk is unchanged.

---

## Knowledge Captured

- **`content_signature` strips `created_at` before the version check**
  (`ledger/src/lib.rs:45`). That is the whole reason first-seen works: an object re-observed
  unchanged on a later `commit` gets **no** new version, so its `created_at` stays at the moment
  it first entered *this* ledger. On a real content change a new version *is* written with a
  fresh `created_at`, so `fs` is an exact first-seen for objects that never changed and an upper
  bound otherwise — `as_of` (which uses `written_at <=` on the ledger row) is the precise path.

- **`all_objects_at` filters on the ledger row's `written_at`, not the payload's `created_at`.**
  The two usually agree; they diverge exactly in the "object changed content" case above.

- **The `self` workspace has a nearly-flat timeline.** `ekos ledger timeline --bucket day` on this
  repo returns **one** bucket (2026-08-26) — the ledger was wiped and fully recompiled that day,
  not committed incrementally. The timelapse is only as deep as the ledger's retained history
  (RFC §7); it shines on a workspace `commit`-ed incrementally over weeks, and is close to a
  no-op on a freshly-rebuilt one. `GraphTimeline` renders nothing when there are zero buckets and
  a single bar when there is one — honest, not broken.

- **`nodeVisibility` alone may not repaint** when only a *prop* (not `graphData`) changes on
  `react-force-graph`. Pairing it with an explicit `fgRef.current.refresh()` in a `useEffect` on
  `asOf` makes the scrub reliably visible without a data-array swap (which would risk a relayout).

- **Monotonicity is what makes client-side scrubbing sound.** No object-level tombstone exists
  anywhere in the codebase, so the latest graph *is* the union of everything that ever existed;
  `fs <= T` is a pure subset filter. The one gap — a relationship whose *state* changed
  (`unconfirmed` → `rejected`) still shows at `T` — is handled by the opt-in "accurate mode"
  (refetch `?as_of=<tick>` per stop), which most workspaces never need.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0134-web-console-phase-6-graph-time-travel.md` | New — the RFC |
| `ekos/crates/runtime/src/graph_export.rs` | `as_of` + `include_first_seen` opts; `fs` on Node/Edge; `as_of` echo; time-sliced source; `EdgeRow`/`GroupAcc`/`CollapsedEdges` type aliases; 5 new tests |
| `ekos/crates/cli/src/commands/graph.rs` | `--as-of` (RFC 3339 parse) + `--first-seen` args; 2 new tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `--as-of` / `--first-seen` clap flags, threaded through |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_graph_export`: `as_of` + `include_first_seen` params + schema text |
| `web/api/app/routes/graph.py` | `as_of` + `include_first_seen` on `/graph`; `as_of` → `at` on `/objects/{id}` |
| `web/api/app/schemas.py` | `GraphOut.as_of` |
| `web/api/tests/test_graph_unit.py` | New — arg-passthrough unit tests (recording MCP override) |
| `web/api/tests/test_graph_live.py` | `as_of` / `first_seen` / object-panel-`as_of` live assertions |
| `web/ui/src/pages/GraphTimeline.tsx` | New — the slider |
| `web/ui/src/pages/{Graph,GraphCanvas,ObjectPanel}.tsx`, `graph-shared.ts` | Wire the slider; frozen layout; `as_of` on the panel |
| `web/ui/src/index.css` | `.graph-timeline` / `.gt-*` / `.graph-asof-banner` |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 6 note |

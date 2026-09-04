# RFC 0134 — Web Console Phase 6: graph time-travel (timelapse slider)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-04
**Phase 6 of:** RFC 0127 (§9, §10) · **builds on:** RFC 0133 (Phase 5 graph view), RFC 0128 R1
(`ekos graph export` / `ekos_graph_export`), RFC 0129 R6 (`ekos ledger timeline --json`),
RFC 0096 (`AS OF` point-in-time reconstruction)
**Closes:** the `--as-of` graph-export deferral recorded in `TODO.md` ("the `all_objects_at`
primitive exists, scope doesn't")
**Defers:** RFC 0127 Phase 6's other items — neighbourhood isolation, impact-mode trace,
server-side ForceAtlas2, PNG/glTF export — and Phase 7 (hardening) → RFC 0135+

---

## Motivation

The ledger is append-only and every `KirObject` / `KirRelationship` carries a `created_at`
stamp. `ekos_ledger::content_signature` **strips `created_at` before the version check**
(`crates/ledger/src/lib.rs:45`), so an object that is re-observed unchanged on a later `commit`
does **not** get a new version — its `created_at` stays at the moment it first entered *this*
ledger. `KnowledgeStore` already exposes `all_objects_at(t)` / `all_relationships_at(t)` /
`object_at(id, t)` (tested, `crates/ledger/src/lib.rs:857`+), and `ekos_state` already accepts an
`at` timestamp (`crates/cli/src/commands/mcp.rs:927`, `runtime.reconstruct_state_at`).

So "what did this system look like six weeks ago?" is already a well-defined, answerable
question. What's missing is the obvious UI: a slider under the graph. Drag it back and the
diagram redraws as the knowledge existed at that moment — nodes and edges appear as you scrub
forward through the compile history. Stop on any date and click a node to see its evidence as it
stood then.

**Decisions locked before this RFC** (from the design discussion):

- **Ledger time, not source/world time.** The slider axis is *when a fact entered the ledger*
  (`AS OF`), not git-commit dates or when a table was really created in a warehouse. World time
  needs every observation to carry a trustworthy source timestamp, which most don't. The API is
  shaped so a `basis=world` can be added later without breaking callers.
- **Snap to checkpoints, not a continuous millisecond scrub.** Facts land in bursts (one per
  `ekos commit`); between commits the graph is identical. The slider stops on the buckets
  `ekos ledger timeline --json` already produces (day / week / month).
- **Client-side subset scrubbing off one union fetch + a frozen layout.** The graph is
  *monotonic* in ledger time — objects and relationships are minted once and never deleted (no
  object-level tombstone exists anywhere in the codebase, per CLAUDE.md). So the browser fetches
  the latest full graph once, with a first-seen stamp per element, and filters `first_seen <= T`
  locally while dragging — zero round-trips. A server `as_of` path exists too, for the detail
  panel, for CLI/MCP parity, and as an opt-in "accurate mode".

**Not in this RFC:** anything that writes; source/world-time axis; per-commit (sub-day)
checkpoint granularity; neighbourhood isolation / impact mode / image export (still Phase 6/7 of
RFC 0127, now → RFC 0135+).

---

## 1. Rust — `as_of` + `first_seen` on the graph export

All in `ekos_runtime::graph_export` (`crates/runtime/src/graph_export.rs`), the one function both
`ekos graph export` and `ekos_graph_export` call.

### 1.1 `GraphExportOptions`

```rust
pub struct GraphExportOptions {
    // … existing …
    /// Reconstruct the graph as it stood at this instant. `None` = current state.
    pub as_of: Option<DateTime<Utc>>,
    /// Stamp each node/edge with its first-seen `created_at` (`fs` in the wire format).
    pub include_first_seen: bool,
}
```

`export_graph` changes exactly one thing at the top:

```rust
let (objects, relationships) = match opts.as_of {
    Some(t) => (store.all_objects_at(t)?, store.all_relationships_at(t)?),
    None    => (store.all_objects()?,     store.all_relationships()?),
};
```

Everything downstream — kind filter, rel-kind filter, degree, `min_degree`, truncation,
aggregation — is unchanged. `counts` (`total_objects`, `objects_after_filter`, …) are then
naturally "as of `t`".

### 1.2 Wire format

- `GraphExport` gains `as_of: Option<DateTime<Utc>>` (echoed back; `skip_serializing_if` when
  `None`).
- When `include_first_seen`, each `Node` gains `fs` and each `Edge` gains `fs`
  (`DateTime<Utc>`, `skip_serializing_if = Option::is_none`):
  - **object level:** the object's / relationship's own `created_at`;
  - **aggregate level:** the super-node's `fs` = the **minimum** `created_at` over its members;
    a group edge's `fs` = the minimum over the collapsed underlying relationships. (A super-node
    appears the instant its first member does.)

### 1.3 CLI + MCP

- `ekos graph export --as-of <rfc3339>` and `--first-seen`.
- `ekos_graph_export` gains `as_of` (string, RFC 3339) and `include_first_seen` (bool), described
  in the tool schema. This is the `--as-of` graph export `TODO.md` deferred.
- `ekos_state`'s `at` param and the point-in-time machinery already exist — no Rust change for
  the object panel.

### 1.4 Determinism

The determinism-modulo-`generated_at` test extends: with a fixed past `as_of`, two calls are
byte-identical (`generated_at` still excluded). `fs` values are drawn from stored `created_at`,
which is stable, so they don't perturb the test.

### 1.5 Backend coverage

`all_objects_at` / `all_relationships_at` are implemented for `Ledger` (SQLite) and `FactLedger`
(fact engine) — the two backends a single-node console workspace uses. On a **partitioned /
distributed** store the console does not run today (`McpSupervisor` starts `ekos mcp serve` for a
local workspace dir); if `as_of` is ever called against one and the primitive is unimplemented,
`export_graph` returns a clear `RuntimeError` rather than silently ignoring the parameter. Open
question §6.1.

---

## 2. web/api

### 2.1 `GET /workspaces/{id}/graph` — two new query params

```
as_of: datetime | None          # RFC 3339; passed through as ekos_graph_export `as_of`
include_first_seen: bool = False # passed through
```

`GraphOut` (schemas): add optional `as_of`, and permissive `fs` on the node/edge models.

### 2.2 `GET /workspaces/{id}/objects/{object_id}` — `as_of` passthrough

```
as_of: datetime | None   # forwarded to ekos_state as `at`
```

One line — the tool already does the work.

### 2.3 Checkpoints — **no new endpoint**

`GET /workspaces/{id}/stats/timeline` already proxies `ekos ledger timeline --json` (cumulative
object + relationship counts, `bucket` = day / week / month) and the dashboard already renders it
as the growth chart. The slider's ticks and its background histogram are exactly those points.
The frontend reuses the existing endpoint; the only addition is that `Graph.tsx` calls it with
`bucket=day`.

---

## 3. web/ui

`Graph.tsx` / `GraphCanvas.tsx` + one new `GraphTimeline.tsx`.

### 3.1 The union fetch

On entering a LOD, fetch **once** with `include_first_seen=1` and **no** `as_of` — since the
graph is monotonic, the latest graph *is* the union of everything that ever existed. Hold it in
memory with `fs` per element.

### 3.2 `<GraphTimeline>` — docked at the bottom of the canvas

- A full-width track with tick marks at each `stats/timeline` bucket, and a faint per-bucket
  histogram of `objects_added` behind it (so you see where the activity was).
- A draggable handle; a live date label; `⏮` `▶ / ⏸` `⏭` `⤒ latest`.
- Keyboard: `←` / `→` step one bucket, `space` play/pause.
- Dragging sets `asOf` (an ISO string) or `null` at the far right (= latest / live).

### 3.3 Client-side time filter

`asOf` becomes one more predicate in the existing `useMemo` that already applies `min_degree` /
rel-kind / focus-kind:

```ts
const visible = nodes.filter(n => !asOf || (n.fs && n.fs <= asOf));
```

Instant, no fetch. Edges are kept only if both endpoints are visible **and** `!asOf || e.fs <=
asOf`.

### 3.4 Frozen layout (replaces Phase 5's re-simulate-on-reload)

Run the force simulation **once** over the union node set, then pin every node's `fx` / `fy` and
set `cooldownTicks={0}`. Scrubbing toggles visibility / opacity only — surviving nodes never
move, entering nodes fade in at their final position. This is the client-side, compute-once form
of RFC 0127 §9.4's "server-side ForceAtlas2 with cached coordinates"; the server-side version
stays deferred, but the console stops re-simulating on every reload.

### 3.5 Not-at-latest banner

While `asOf != null`, a persistent pill over the canvas: **"Viewing as of 2026-08-01 · 27 days
back"**. Same pattern as Phase 5's truncation banner — the user never forgets they're in the
past.

### 3.6 Play

Steps `asOf` bucket → bucket every ~700 ms. New nodes / edges get a short CSS opacity transition
as they enter. `prefers-reduced-motion` → hard-cut, faster interval.

### 3.7 Object panel, as of T

When `asOf != null`, `ObjectPanel` fetches `GET /objects/{id}?as_of=<asOf>` and its header reads
**"state as of 2026-08-01"**. Properties, relationships and evidence are all reconstructed at
that instant by `reconstruct_state_at`.

### 3.8 Accurate mode (opt-in toggle)

Client-side `fs <= T` filtering reproduces node/edge **presence** exactly, but not relationship
**state changes** — an edge that was `unconfirmed` at T and later `rejected` still shows at T
(it never disappears from the union). Most edges never change state, so presence-only is the
default. An "accurate" toggle re-fetches `/graph?as_of=<tick>&include_first_seen=1` on every
stop, so `all_relationships_at` gives the exact historical state, at one round-trip per stop.

---

## 4. Testing

**Rust (`crates/runtime`, `crates/cli`)**
- `export_graph` with `as_of` in the past returns only the objects/edges minted at-or-before it;
  with `as_of = Utc::now()` it equals the unbounded export. (Fixture: seed at t0, seed more at
  t1, assert the t0 snapshot.) Both `sqlite()` and `fact()` fixtures, matching the existing
  `graph_export` test style.
- `include_first_seen`: object level stamps each node/edge with its `created_at`; aggregate level
  stamps the super-node with the member `min`.
- Determinism with a fixed `as_of`.
- `ekos graph export --as-of <ts> --first-seen` parses and round-trips (CLI arg test).

**web/api**
- `graph?as_of=<ts>&include_first_seen=1` forwards both to the tool (unit, mock MCP client).
- `objects/{id}?as_of=<ts>` forwards `at` (unit).
- Live (`EKOS_BIN`): a past `as_of` against this repo returns fewer nodes than latest.

**web/ui**
- `tsc` + `vite build`; graph chunk still a separate lazy bundle.
- `GraphTimeline` render test with a mocked `stats/timeline` — ticks render, dragging the handle
  fires `onChange` with a bucket timestamp.
- `Graph.tsx` with a mocked union response carrying `fs` — moving `asOf` back hides the
  later-minted nodes without a second `/graph` fetch.

---

## 5. Verification

- Rust: `cargo test -p ekos-runtime -p ekos` green; `clippy` + `fmt` clean.
- `web/api`: `ruff` + `pytest` green (unit + `EKOS_BIN`-gated).
- `web/ui`: `tsc` clean, `vite build`, entry chunk unchanged.
- End to end against `self`: `/w/self/graph` → drag the slider to mid-August → the graph visibly
  shrinks to the objects that existed then; the histogram shows the build activity; opening a
  node panel shows "state as of …" with the evidence it had at that point; "jump to latest"
  restores the live view.

---

## 6. Files changed (projected)

| File | Change |
|---|---|
| `crates/runtime/src/graph_export.rs` | `as_of` + `include_first_seen` opts; `fs` on Node/Edge; `as_of` echo; time-sliced source |
| `crates/cli/src/commands/graph.rs` | `--as-of` + `--first-seen` args |
| `crates/cli/src/commands/mcp.rs` | `ekos_graph_export`: `as_of` + `include_first_seen` params + schema text |
| `web/api/app/routes/graph.py` | `as_of` + `include_first_seen` on `/graph`; `as_of` on `/objects/{id}` |
| `web/api/app/schemas.py` | `GraphOut.as_of`; `fs` on node/edge models |
| `web/ui/src/pages/Graph.tsx` | union fetch; `asOf` state; time predicate; banner; panel `as_of` |
| `web/ui/src/pages/GraphCanvas.tsx` | frozen layout (`fx`/`fy`, `cooldownTicks={0}`); enter transition |
| `web/ui/src/pages/GraphTimeline.tsx` | new — the slider, histogram, transport controls |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 6 note |

---

## 7. Known limitation (state it plainly)

The timelapse is only as rich as the ledger's **retained history**. A workspace that was wiped
and fully rebuilt, or migrated to the fact engine in one pass, has every `created_at` clustered
at the rebuild instant — the slider is then nearly flat. The feature shines on a workspace that
has been `commit`-ed incrementally over weeks (RFC 0106 incremental builds). `content_signature`
stripping `created_at` is what lets an incrementally-built workspace keep real first-seen stamps
across recompiles; without incremental history there is simply little to scrub through. The
not-at-latest banner and the histogram make the available depth visible rather than implied.

---

## 8. Open questions

1. **Partitioned / distributed `all_objects_at`.** Confirm whether `SegmentBackend` /
   `DistributedLedger` implement the `_at` primitives; if not, `export_graph` errors clearly on
   `as_of` there. Not blocking — the console runs single-node workspaces today.
2. **Per-commit checkpoints.** Day buckets are the v1 tick granularity (reusing `ledger
   timeline`). A `ekos ledger checkpoints --json` listing distinct commit transaction timestamps
   would give exact per-`commit` stops; deferred until day granularity proves too coarse.
3. **Deep-linking.** `/w/:id/graph?as_of=<ts>&focus=<id>` — shareable "here is the graph on this
   date, centred on this object". Trivial once §3.4's fixed layout lands; pairs with RFC 0133
   §6.3.
4. **World-time axis.** Design `as_of` to later accept `basis=ledger|world`; `world` would key
   off git-commit / warehouse `created` timestamps carried on observations that have them.

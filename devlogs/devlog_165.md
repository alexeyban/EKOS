# Devlog 165 — RFC 0127 Phase 7: web console hardening

**Date:** 2026-09-05
**Branch:** `main` (direct — local-tests-only + `[skip ci]`, per standing maintainer direction)
**Related:** continues `devlog_164` (RFC 0136, Phase 6); the user then directed continuing
autonomously into RFC 0127's Phase 7 (hardening), the console's last named phase.

---

## Summary

Phase 7 is explicitly a hardening pass, not new console surface: performance, docs, theming, and
packaging, plus a handful of items the Phase 6 devlog had already flagged as deferred rather than
forgotten. Five items closed this session: a route-level code-splitting pass (main bundle
669KB → 254KB, no more chunk-size build warning), deep-linking (`?as_of=&focus=`) on the graph
view, a real distributed `evidence_count` RPC (closing the last `Err`-stub gap in
`DistributedLedger`), a docs/README rewrite for both `web/README.md` and the root README's Web
console section, and a `docker-compose.yml` cleanup that removed dead configuration confirmed
unused since Phase 1. Two items were investigated and explicitly left as-is: theming (already
~70 CSS custom properties deep, nothing to fix) and "true streaming ndjson" / "source-time slider
axis" (both real but out of scope for a hardening pass — see Decisions below).

---

## PR — Phase 7 hardening

### Problem / motivation

RFC 0127 §10 lists Phase 7 as the console's last phase: "performance, theming, docs, packaging."
Unlike Phases 1–6, this has no single RFC of its own — it's a punch-list against work already
shipped, plus closing out items the Phase 6 devlog (`devlog_164`) named as deferred: the
distributed `evidence_count` stub, and console UX (deep-linking) that Phase 6 didn't scope in.

### What was built

| Component | Change |
|---|---|
| Distributed `evidence_count` RPC | `WorkerRequest::EvidenceCount { partition }` + `QueryWorkerClient::evidence_count` (same shape as the existing `ObjectCount`/`RelationshipCount` RPCs) + `DistributedLedger::evidence_count` real fan-out (`self.partitions(PClass::Evidence)` → `self.fan_out(...)` → sum), replacing an `Err` stub. `ekos status`/`ekos_status` now report a real evidence count on a distributed workspace. |
| Route-level code-splitting | `main.tsx`: every route converted from a top-level `element: <X />` import to `lazy: () => import("./pages/X").then((m) => ({ Component: m.X }))` (React Router v6.4+). |
| Deep-linking | `Graph.tsx`: `?as_of=&focus=` seed initial state via `useSearchParams()` lazy initializers; state syncs back to the URL one-directionally (`replace: true`) as the time-travel slider or selection changes. A "🔗 copy link" toolbar button. |
| Docs | `web/README.md` rewritten (was still describing "Phase 0 skeleton"); root `README.md`'s Web console header now credits RFC 0136. |
| Packaging cleanup | `web/docker-compose.yml`: removed `mcp_host`/`mcp_port` seed fields and the `host.docker.internal` `extra_hosts` entry (dead since Phase 1 — confirmed via `WorkspaceSeed`'s own docstring); added the session-secret/write-token env vars the current auth flow actually reads. |
| Theming | Investigated, no change — `index.css` already uses ~70 `var(--...)` custom properties from earlier phases. |

### Implementation details worth remembering

- **The bundle-size fix is entirely route-level, not component-level.** `GraphCanvas` was already
  lazy-loaded on its own (Phase 5/6), which masked how much of the *rest* of the bundle was still
  eager — in particular `Dashboard`'s `recharts` dependency, which shipped on every single page
  visit regardless of route. Converting every route (not just the heavy one) to `lazy` is what
  actually removed the build's chunk-size-limit warning; converting only `Dashboard` would have
  left every other page's code inside the same oversized main chunk.
- **Deep-linking is one-directional by design (state → URL), not a two-way binding.** A full
  two-way sync (URL changes drive state on every render, not just mount) would need explicit
  guarding against browser back/forward re-driving state mid-session — out of scope for what this
  pass needed ("paste this link, see the same view"). The mount-only read uses a `useState` lazy
  initializer specifically so it fires exactly once, not on every re-render.
- **`?focus=<id>` alone needs a synthetic `focusKind` value to resolve.** `Graph.tsx`'s
  `focusKind` state has two "real" values (`null` = aggregate overview, a kind string = filtered)
  plus a value no click-driven interaction ever produces: `""`, meaning "expanded object view, no
  kind filter." A bare `?focus=<id>` with nothing else in the URL needs exactly this state — the
  id can't resolve against the aggregate overview's rolled-up nodes — so a one-time mount effect
  sets `focusKind` to `""` when `?focus=` is present and no other state already set it.
- **The `docker-compose.yml` cleanup was confirmed dead, not assumed dead.** `mcp_host`/`mcp_port`
  as `WorkspaceSeed` fields still exist in `app/settings.py` (used internally for the supervisor's
  own default bind host and starting port), but the seed-JSON fields of the same name that the old
  compose file passed were never read for that purpose past Phase 1 — `WorkspaceSeed`'s own
  docstring says so directly. Grepped the whole `web/` tree for `host.docker.internal`/`mcp_host`/
  `mcp_port`/`EKOS_MCP_TOKEN` before removing anything, to make sure no other file (tests included)
  still depended on the old shape.

### Decisions (alternatives considered, why this choice)

- **No production Dockerfile added for `web/ui`.** The existing `docker-compose.yml` runs Vite's
  own dev server inside the `ui` container (`npm run dev`), not a built static bundle — it was
  already dev-convenience-only before this pass, and nothing in RFC 0127 asked for a production
  deployment manifest. Fixing the compose file's stale env vars was in scope; inventing a new
  production packaging story was not, so it stayed out.
- **True streaming ndjson graph export — deferred, not attempted.** `Runtime`'s graph-export path
  builds the whole `GraphExport` in memory before any output write begins; only the *write* side
  is currently incremental. Making construction itself lazy is a real `ekos-runtime` refactor
  bigger than a hardening pass, and the console's own `ekos_graph_export` calls are already capped
  (`max_nodes`/`max_edges`, default 5000/20000) — this specifically matters for the CLI's
  uncapped standalone export, not the console UI this phase is about.
- **Source/world-time slider axis — deferred, and not purely a polish item.** RFC 0134 chose
  ledger time over source/world time deliberately: "world time needs every observation to carry a
  trustworthy source timestamp, which most don't." Building this axis for real means teaching
  every analyzer to emit a meaningful observation timestamp first — a recovery-layer project, not
  a console-hardening one.

---

## Knowledge Captured

- React Router v6.4+'s per-route `lazy` returns `{ Component }` (or `{ loader, action,
  Component, ... }`), not a raw component — `import(...).then((m) => ({ Component: m.X }))` is the
  full pattern; a bare `.then((m) => m.X)` will not satisfy the route object's shape.
- When a component-level lazy-load (like `GraphCanvas`) already exists in a codebase, don't treat
  it as evidence the bundle-size problem is solved — check whether every *route*, not just the one
  known-heavy component, is still eagerly imported at the top level. The two are independent axes.
- Grep before deleting "looks dead" config: `mcp_host`/`mcp_port` exist as two genuinely different
  things in this codebase (a `WorkspaceSeed` JSON field the console API long ago stopped reading,
  and unrelated `Settings` fields the supervisor still uses for its own defaults) — same names,
  different lifecycles. Confirming via the settings module's own docstring, not just "it looks
  unused," is what made the compose-file cleanup safe.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/distributed/src/protocol.rs` | Added `WorkerRequest::EvidenceCount { partition }` |
| `ekos/crates/distributed/src/worker.rs` | Dispatch arm for `EvidenceCount` |
| `ekos/crates/distributed/src/worker_client.rs` | `evidence_count()` client method |
| `ekos/crates/distributed/src/gateway.rs` | `DistributedLedger::evidence_count` real fan-out (was `Err` stub) |
| `ekos/crates/distributed/tests/gateway.rs` | End-to-end evidence-count assertion in the two-worker test |
| `ekos/crates/ledger/src/lib.rs` | Doc-comment note on the closed provenance-audit gap |
| `web/ui/src/main.tsx` | Every route converted to `lazy` |
| `web/ui/src/pages/Graph.tsx` | Deep-linking (`?as_of=&focus=`), copy-link button |
| `web/README.md` | Rewritten for current (Phases 0-6 shipped) status |
| `README.md` | Web console section header credits RFC 0136 |
| `web/docker-compose.yml` | Removed dead `mcp_host`/`mcp_port`/`extra_hosts`, added session-secret/write-token env vars |
| `TODO.md` | Phase 7 hardening tracking closed out item-by-item |

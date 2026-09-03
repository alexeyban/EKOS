# RFC 0129 — Web Console Phase 1: shell + statistics

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-03
**Phase 1 of:** RFC 0127 (§10) · **builds on:** RFC 0128 (`web/` skeleton, R4 TCP auth, `devlog_151`),
RFC 0116 (`ekos status`), RFC 0114 (query usage log), RFC 0096 (EKL `COUNT`/`GROUP BY`), RFC 0104
(cross-process write lock)
**Defers:** RFC 0127 Phases 2–7 (config UX, job runner, scheduler, graph) → RFC 0130+

---

## Motivation

RFC 0128 shipped a `web/` skeleton that only works if an operator has *already* started one
`ekos mcp serve --tcp` by hand and pasted its details into `EKOS_CONSOLE_WORKSPACES_JSON`. Its one
page shows a health check and a workspace list. That is enough to prove the wiring; it is not a
console.

Phase 1 makes the console **own its inputs and show the shape of a workspace**:

1. A persisted workspace registry — register a directory containing `ekos.toml`, the console does
   the rest.
2. The console spawns and supervises one `ekos mcp serve --tcp` per registered workspace. No more
   hand-started servers.
3. A real dashboard: entry / object / relationship / evidence counts, storage breakdown, objects
   by kind, a growth timeline, recent-query stats, and `doctor` status — the numbers that make a
   compiled workspace legible at a glance.

**Decisions locked before this RFC** (RFC 0127 §12 open questions, answered by the maintainer):

- **Auth stays a single static `CONSOLE_TOKEN`.** No user table, no session cookies, no read/write
  role split in Phase 1. The split lands with the first phase that introduces a write path from
  the browser (Phase 3, the job runner) — until then every route is read-only and one token gates
  all of them. This matches RFC 0127 §11's "one operator or one small trusted team" Non-Goal.
- **MCP-server supervision is its own mechanism, separate from the Phase 3 job runner.** The two
  have different shapes: MCP servers are long-lived, idle-cheap, and want restart-on-crash with
  backoff; pipeline jobs are bursty, heavy, queued, cancellable, and mutually exclusive per
  workspace. One abstraction forced over both would serve neither well. `supervisor.py` (this
  RFC) and `runner.py` (Phase 3) stay distinct modules.

**Explicitly not in this RFC:** `ekos.toml` editing / preview-scan (Phase 2), running any pipeline
command from the browser (Phase 3), scheduling (Phase 4), and every graph view (Phases 5–6). The
`web/` stubs for those stay stubs.

---

## 1. Rust-side additions

Two small machine-readable outputs, both mirroring RFC 0127 R2's `ekos status --json` in style —
one flat JSON object (or array), text output untouched.

### 1.1 R5 — `ekos doctor --json`

`ekos doctor` today prints a `[OK]/[WARN]/[FAIL]` checklist. `--json` emits:

```json
{
  "schema_version": 1,
  "ok": true,
  "checks": [
    {"name": "Rust toolchain", "status": "ok",   "detail": "rustc 1.98.0 (…)"},
    {"name": ".ekos/",         "status": "ok",   "detail": "/abs/path/.ekos"},
    {"name": "LLM provider",   "status": "ok",   "detail": "ollama (local, no API key)"}
  ]
}
```

`status` is one of `ok` / `warn` / `fail`; `ok` (top level) is `true` iff no check is `fail`.
`crates/cli/src/commands/doctor.rs` already computes each check as a struct — this is a serializer
plus a `--json` flag on `DoctorArgs`, no logic change.

### 1.2 R6 — `ekos ledger timeline --json`

The dashboard's growth timeline needs entries/objects/relationships bucketed over time. Doing it
with repeated `FIND Object AS OF <ts> COUNT` (RFC 0096) is N whole-store scans — too costly to be
a dashboard call. Instead, one pass over the ledger's append timestamps:

```
ekos ledger timeline --json [--bucket day|week|month] [--since <rfc3339>]
```

```json
{
  "schema_version": 1,
  "bucket": "day",
  "points": [
    {"t": "2026-08-26", "entries": 20793, "objects": 5533, "relationships": 8364}
  ]
}
```

Points are **cumulative** (running totals as of the end of each bucket), so the frontend renders
an area chart directly. Empty buckets are omitted; the frontend carries the last value forward.

- **Fact-segment backend:** the fact record carries `created_at` (already in the segment schema —
  `manifest.json` `attributes.names`). If `created_at` turns out to be observation-derived rather
  than append-derived (see §6 Open questions), R6 falls back to **segment seal timestamps** —
  coarser but honest, and monotonic by construction.
- **SQLite backend:** entry rows are timestamped; `GROUP BY date(ts)` directly.
- **Partitioned / distributed backends:** `Err("timeline not supported on <backend>")` for now,
  the same way RFC 0127 R2's `evidence_count` returns `Err` on the distributed gateway. The
  console degrades to hiding the timeline card.

Lives in `crates/cli/src/commands/ledger.rs` + a `timeline(&dyn KnowledgeStore, Bucket) ->
Result<Timeline>` helper next to R2's status builder. No new `KnowledgeStore` trait method if
`created_at` is already reachable through the existing read API; one narrow method
(`append_timeline() -> Result<Vec<(DateTime, EntryKind)>>`) if not.

---

## 2. MCP-server supervision — `web/api/app/supervisor.py`

Replaces the Phase 0 `ClientPool`-connects-to-a-hand-started-server model.

```python
class McpSupervisor:
    async def start(self) -> None                    # spawn a server for every registered workspace
    async def ensure(self, ws: Workspace) -> ServerHandle   # spawn on demand (new registration)
    async def stop(self, workspace_id: str) -> None
    async def aclose(self) -> None                    # SIGTERM all, SIGKILL after grace
    def handle(self, workspace_id: str) -> ServerHandle | None
```

- **One `ekos mcp serve --tcp 127.0.0.1:<port> --tcp-token-file <f>` per workspace.** Port is
  allocated from an ephemeral range and recorded on the `ServerHandle` (not persisted — re-derived
  on each console start). The token is a per-process random 32-byte hex string written to a
  `0600` temp file; it is the R4 (RFC 0128) bearer token the `EkosMcpClient` sends in its
  `initialize` handshake. Nothing binds anything but loopback.
- **Launch via `asyncio.create_subprocess_exec`** — argument list only, never a shell. The `ekos`
  binary path comes from `EKOS_BIN` (already used by the Phase 0 live test) or `PATH`.
- **Readiness:** after spawn, the supervisor opens the client and calls `tools/list` with a short
  timeout before marking the handle ready; `/api/workspaces/{id}/*` returns `503 mcp server
  starting` until then.
- **Restart on crash:** exponential backoff (1s, 2s, 4s, … capped at 30s), reset after 60s
  healthy. After 5 consecutive failures the handle goes `failed` and the dashboard shows it;
  no infinite spawn loop.
- **Shutdown:** `aclose()` on FastAPI lifespan teardown SIGTERMs every child, then SIGKILL after
  a 5s grace. Orphan `ekos` processes from a hard-killed console are adopted on next start by
  matching the `--tcp-token-file` path prefix and terminated.
- **`ClientPool` is folded into the supervisor** — one `EkosMcpClient` per handle, created when
  the handle becomes ready, reconnecting through the existing one-retry path (RFC 0128 §2).

This is deliberately *not* generalised for the Phase 3 job runner. Shared concepts (find the
binary, `create_subprocess_exec`, SIGTERM-then-SIGKILL) are a ~15-line `_proc.py` helper, not a
framework.

---

## 3. Console persistence — `web/api/app/models.py`

SQLite at `.ekos-web/console.db` (RFC 0127 §8.1), SQLModel. **Phase 1 defines one table:**

```python
class Workspace(SQLModel, table=True):
    id: str            # slug, unique
    name: str
    path: str          # absolute, contains ekos.toml — validated on register
    created_at: datetime
```

`Run`, `Schedule`, and any `User` table are added by the phases that need them (3, 4, and
whichever introduces the role split). The DB file and a `SQLModel.metadata.create_all` on startup
are all Phase 1 sets up. `EKOS_CONSOLE_WORKSPACES_JSON` stays supported as a **seed** — on first
start an empty DB is populated from it — so existing Compose setups keep working.

---

## 4. HTTP surface (Phase 1 subset of RFC 0127 §8.3)

```
GET    /api/health                                   # unchanged (public)

GET    /api/workspaces                               # registry, each with live server status
POST   /api/workspaces        {name, path}           # validate path has ekos.toml + .ekos/; spawn server
DELETE /api/workspaces/{id}                          # stop server, drop row

GET    /api/workspaces/{id}/health                   # R5  — ekos doctor --json  (subprocess)
GET    /api/workspaces/{id}/stats                    # R2  — ekos status --json  (subprocess)
GET    /api/workspaces/{id}/stats/kinds              # ekos_ekl "FIND Object COUNT GROUP BY kind" (MCP)
GET    /api/workspaces/{id}/stats/timeline?bucket=   # R6  — ekos ledger timeline --json (subprocess)
GET    /api/workspaces/{id}/stats/queries            # aggregate .ekos/query-log.jsonl (RFC 0114)
```

- `stats/kinds` goes through the **already-running MCP server** (`ekos_ekl` tool) — no subprocess.
- `stats/queries` reads `<workspace>/.ekos/query-log.jsonl` off disk (the console has the
  workspace path) and returns counts by tool, cache-hit rate, and p50/p95 duration over a
  window. No new Rust — RFC 0114 already writes the file.
- `stats`, `health`, `stats/timeline` need a **read-only subprocess** (§5).

Every `/api/workspaces/{id}/*` route stays behind the single `require_console_token` dependency
from RFC 0128 §3.3.

---

## 5. The read-only subprocess seam — `web/api/app/readproc.py`

R2/R5/R6 are CLI-only (the MCP tools expose leaner payloads). Phase 1 needs to shell out, but it
does **not** need the Phase 3 job runner (queue, mutex, cancellation, SSE, write-role gate). So:

```python
async def read_json(ws: Workspace, argv: list[str], *, timeout: float = 20.0) -> Any: ...
```

- **Hardcoded allowlist of exactly three argv shapes:** `["status", "--json"]`,
  `["doctor", "--json"]`, `["ledger", "timeline", "--json", ...]`. Anything else raises. There is
  no endpoint that takes a command string. (The full RFC 0127 §8.4 allowlist + `is_write` roles
  arrives with Phase 3.)
- `create_subprocess_exec(EKOS_BIN, *argv, "--workspace", ws.path)` — argument list only.
- `ws.path` is `Path.resolve()`d and confirmed to be a registered workspace root before the call —
  `..` cannot escape.
- Output capped (1 MiB), parsed as JSON, `timeout` enforced with SIGKILL. These are all read-only
  verbs; concurrent calls are safe and un-serialised (unlike writes, RFC 0104).

`_proc.py` (find binary, spawn, kill-after-grace) is shared with §2; nothing else is.

---

## 6. Frontend — the dashboard

`web/ui`, still Vite + React + TS. New this phase:

- **`react-router-dom`** — two routes: `/` (workspace picker + add form) and `/w/:id` (dashboard).
- **`recharts`** — the only new heavy dep. Charts: storage breakdown (stacked bar from R2
  `storage.components`), objects by kind (horizontal bar from `stats/kinds`), growth timeline
  (area from `stats/timeline`), query mix (small bar from `stats/queries`).
- **Stat cards:** entries, objects, relationships, evidence — big numbers from R2, with
  `last_write` and the backend tag as sub-labels.
- **Doctor panel:** the R5 checklist, `ok`/`warn`/`fail` dots.
- **Server status chip** per workspace: `ready` / `starting` / `failed (n retries)` from the
  supervisor, surfaced on `GET /api/workspaces`.
- **Generated API types:** `npm run gen:api` (openapi-typescript, already wired in Phase 0) now
  runs against a booted uvicorn in a `make types` step; `src/api/types.ts` becomes the generated
  `schema.d.ts` and the hand-stub is deleted. Whether CI gates drift on it is still open
  (carried from RFC 0127 §12) — not decided here.
- The console token input and per-workspace stats line from `devlog_151` fold into the new
  dashboard layout.

Theming stays the copied house `theme.css`; no component library yet (Phase 7 does the polish
pass).

---

## 7. Testing

**Rust**
- R5: `doctor --json` shape; `ok:false` when a check fails (point `--config` at a missing file).
- R6: `timeline --json` cumulative-monotonic on both sqlite and fact-segment fixtures; `--bucket`
  arithmetic; `Err` on partitioned/distributed.

**`web/api`** (pytest)
- Supervisor: spawns a real `ekos mcp serve --tcp` for a fixture workspace (gated on `EKOS_BIN`,
  like the Phase 0 live test), `GET /api/workspaces` reports `ready`, a killed child is
  restarted, `aclose()` leaves no orphan.
- Registry: `POST /api/workspaces` rejects a path with no `ekos.toml`; `DELETE` stops the server.
- `read_json`: the allowlist rejects an unlisted argv; a timeout SIGKILLs; `..` in a path is
  refused.
- `stats/queries`: aggregation math against a fixture `query-log.jsonl`.

**`web/ui`**: `tsc --noEmit` + `vite build`; a render test of the dashboard against a mocked API.

---

## 8. Verification

- Rust workspace gate clean (`fmt`, `build`, `clippy --workspace -D warnings`, `test --workspace`),
  new R5/R6 tests included.
- `web/api`: `ruff` clean, `pytest` green (unit + `EKOS_BIN`-gated supervisor tests).
- `web/ui`: `tsc` clean, `vite build` succeeds.
- End to end (recorded in the phase devlog): register this repo as a workspace through the API,
  the console spawns its MCP server, `/w/:id` renders real counts, the kinds bar, and the growth
  timeline; kill the `ekos` child and watch the status chip go `starting` → `ready`.

---

## 9. Files changed (projected)

| File / area | Change |
|---|---|
| `crates/cli/src/commands/doctor.rs` + `bin/ekos.rs` | R5 — `--json` flag + serializer |
| `crates/cli/src/commands/ledger.rs` + a `timeline()` helper | R6 — `ekos ledger timeline --json` |
| `crates/ledger` (maybe) | one `append_timeline()` read method if `created_at` isn't already reachable |
| `web/api/app/supervisor.py` | New — per-workspace MCP process lifecycle |
| `web/api/app/models.py` | New — SQLModel `Workspace`, `.ekos-web/console.db` |
| `web/api/app/readproc.py` + `_proc.py` | New — read-only subprocess allowlist + shared spawn helper |
| `web/api/app/routes/workspaces.py`, `routes/stats.py` | Registry CRUD + the five stats endpoints |
| `web/api/app/main.py` | Supervisor on lifespan; DB `create_all`; seed from `WORKSPACES_JSON` |
| `web/api/app/mcp_client.py` | `ClientPool` folded into the supervisor |
| `web/ui/` | Router, dashboard page, Recharts, generated API types |
| `.github/workflows/ci.yml` | R5/R6 covered by the existing Rust job; `web` job unchanged |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 1 note |

---

## 10. Open questions

1. **Is the fact record's `created_at` the append time or the observation time?** If the latter,
   R6 uses segment seal timestamps (§1.2). Resolved during implementation by reading
   `crates/ledger/src/fact.rs`; the RFC commits to *a* monotonic source, not a specific field.
2. **Generated-TypeScript drift in CI.** Still open from RFC 0127 §12. Phase 1 wires the
   generation; whether a CI step boots uvicorn and fails on a stale `schema.d.ts` is deferred to
   whoever finds the first drift bug annoying.
3. **When exactly does the read/write role split land?** Pinned to "the first browser write path",
   which is Phase 3. If Phase 2's `ekos.toml` editor counts as a write (it patches a file, not the
   ledger), the split moves up to Phase 2. Decided when Phase 2 is authored.

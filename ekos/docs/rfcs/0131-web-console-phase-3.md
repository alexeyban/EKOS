# RFC 0131 — Web Console Phase 3: command runner, job runner, OIDC auth

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-03
**Phase 3 of:** RFC 0127 (§8.4, §8.5, §10) · **builds on:** RFC 0130 (Phase 2 console, `devlog_153`),
RFC 0104 (cross-process write lock), RFC 0088 (`commit --yes`)
**Defers:** RFC 0127 Phases 4–7 (scheduler, graph) → RFC 0132+

---

## Motivation

The console can read a workspace and edit its config; it cannot *do* anything to one. Phase 3 adds
the ability to run EKOS pipeline commands from the browser and watch them — which is the first
real mutation from the console, so it also brings the **read/write role split** that Phases 0–2
deferred.

"Run EKOS commands from a browser" is a remote-code-execution surface. RFC 0127 §8.4's four rules
are load-bearing and none are optional:

1. A **hardcoded allowlist** is the only way to run anything. No endpoint takes a command string.
2. **Never a shell** — `create_subprocess_exec` with an argument list.
3. **Path parameters validated against registered workspace roots** after `resolve()`.
4. **Write commands need the write role.**

**Decisions locked before this RFC** (from the maintainer):

- **Auth is OIDC** (Authorization Code + PKCE) against an external provider — the console holds no
  passwords. A signed session cookie carries the authenticated identity; a configurable claim maps
  to the **write** role, everyone else authenticated is **read**. When `OIDC_ISSUER` is unset the
  console falls back to **two static tokens** (`CONSOLE_TOKEN` = read, `CONSOLE_WRITE_TOKEN` =
  write) so `docker compose`, CI, and local dev work with no IdP. OIDC is the real deployment
  path; the token mode is the escape hatch, not a parallel feature to maintain.
- **Run logs render in a plain `<pre>`** with autoscroll, ANSI stripped server-side. xterm.js
  (colors, cursor control) is a Phase 7 polish item.

**Not in this RFC:** the scheduler (Phase 4), the graph (Phases 5–6), running the two write-capable
MCP tools from the browser (they get the `write` gate wired but no dedicated UI yet), `init` from
the console (a workspace must already exist to be registered).

---

## 1. Auth — `web/api/app/auth.py`

One `Principal` (`{subject, email, role}`) resolved per request, two ways to get one:

### 1.1 OIDC mode (`OIDC_ISSUER` set)

- `authlib` for discovery (`<issuer>/.well-known/openid-configuration`), JWKS, and the code
  exchange. `starlette.middleware.sessions.SessionMiddleware` (key: `SESSION_SECRET`) holds the
  post-login `{sub, email, role}` — an opaque signed cookie, no server-side session store.
- **Endpoints:**
  - `GET /api/auth/login` → 302 to the IdP (PKCE, `state`, `nonce` in the session).
  - `GET /api/auth/callback` → validates `state`, exchanges the code, verifies the ID token,
    computes the role, writes the session, 302 to `/`.
  - `POST /api/auth/logout` → clears the session.
  - `GET /api/auth/me` → `{mode, email, role}` or `401`.
- **Role mapping:** `OIDC_ROLE_CLAIM` (default `groups`) is read from the ID token; if any of its
  values is in `OIDC_WRITE_VALUES` (comma-separated config) the principal is `write`, else `read`.
  `OIDC_WRITE_VALUES` empty → every authenticated user is `read` (a read-only deployment).

### 1.2 Token mode (`OIDC_ISSUER` unset)

- `Authorization: Bearer <CONSOLE_TOKEN>` → `read`; `Bearer <CONSOLE_WRITE_TOKEN>` → `write`
  (constant-time compare, `secrets.compare_digest`). `CONSOLE_WRITE_TOKEN` unset → only read is
  ever granted.
- `GET /api/auth/me` → `{mode: "token", role}` (no email).

### 1.3 The dependency

`require_role("read")` accepts `read` **or** `write`; `require_role("write")` needs `write`. Every
existing route moves from `require_console_token` to `require_role("read")`. New write routes use
`require_role("write")`. `401` = not authenticated, `403` = authenticated but wrong role.

---

## 2. Command allowlist — `web/api/app/commands.py`

`COMMAND_ALLOWLIST` is filled in (it has been an empty list since Phase 0):

```python
Command(name, argv, params, is_write, timeout, stages)
```

| name | argv | is_write | notes |
|---|---|---|---|
| `doctor` | `doctor` | no | |
| `build` / `recover` / `resolve` / `compile` / `commit` | each verb | **yes** | `resolve` takes `--force` (bool param); `commit` always gets `--yes` |
| `pipeline` | — | **yes** | `stages = [build, recover, resolve, compile, commit]`, one run entry, per-stage status |
| `clean` | `clean` | **yes** | |
| `status` / `ledger status` | `status` / `ledger status` | no | |
| `graph export` | `graph export --format json` | no | |
| `ledger repair` | `ledger repair` | **yes** | |
| `ledger migrate` | `ledger migrate --v3` | **yes** | |
| `artifact repack` | `artifact repack` | **yes** | |
| `docs generate` | `docs generate --layout curated --output doc` | **yes** (writes files) | |
| `ekl` | `ekl <query>` | no | `query` string param, passed as one argv element (never a shell) |

`params` is a small JSON-Schema-ish dict per command for the UI to render (`{force: {type: bool}}`,
`{query: {type: string, required: true}}`). Absent params → a bare Run button.

---

## 3. Job runner — `web/api/app/runner.py` + `_proc.py`

```python
class JobRunner:
    async def submit(self, ws, command: Command, params: dict) -> str   # -> run_id
    async def cancel(self, run_id: str) -> bool
    async def aclose(self) -> None
```

- **One worker task + one `asyncio.Lock` per workspace.** RFC 0104: EKOS takes a real
  cross-process write lock, so two writes on one workspace is a guaranteed conflict — the lock
  serialises them here instead of letting the second fail mid-run. Different workspaces run
  concurrently. A bounded per-workspace queue (default 16) returns `429` when full.
- **Execution:** `create_subprocess_exec(EKOS_BIN, *argv, cwd=ws.path)`, `stdout` and `stderr`
  merged, streamed line-by-line to `.ekos-web/runs/<run_id>.log` (ANSI stripped) *and* the `Run`
  row's tail. Per-command `timeout` → SIGTERM, then SIGKILL after 5 s, status `timed_out`.
- **Cancellation:** a registry of `{run_id: Process}`; `cancel` sends SIGTERM → SIGKILL, status
  `cancelled`. An interrupted `commit` is safe — commits are idempotent (entry ids are content
  hashes; a re-run skips what's already there).
- **Chained `pipeline`:** iterate `command.stages`; each stage is a subprocess; stop on the first
  non-zero exit. The `Run` row carries `stages: [{name, status, exit_code}]` so a failure at
  `recover` is legible in the history.
- **Crash-safety:** on console startup any `Run` left `running`/`queued` (the console died
  mid-run) is swept to `interrupted`.

### `models.Run`

```python
id, workspace_id, command, params (JSON), status, stages (JSON), created_at,
started_at, ended_at, exit_code, log_path
```

`status ∈ {queued, running, succeeded, failed, cancelled, timed_out, interrupted}`.

---

## 4. HTTP surface (RFC 0127 §8.3)

```
GET  /api/auth/me | login | callback | (POST) logout    # §1
GET  /api/commands                                       # the allowlist catalogue (read)
POST /api/workspaces/{id}/commands/{name}   {params}     # -> {run_id}   (write if is_write, else read)
GET  /api/runs?workspace=&status=&limit=                 # history (read)
GET  /api/runs/{run_id}                                  # detail incl. stages + log tail (read)
GET  /api/runs/{run_id}/logs                             # SSE: text/event-stream, follows to terminal (read)
POST /api/runs/{run_id}/cancel                           # (write)
```

The SSE endpoint opens the log file, emits existing lines, then follows (250 ms poll) until the
run reaches a terminal status, then emits a final `event: end` and closes. No websocket.

---

## 5. Frontend

- **Auth gate** (`Layout.tsx`): `GET /api/auth/me`. `mode: "oidc"` + `401` → a "Sign in" button
  (`window.location = /api/auth/login`). `mode: "token"` → the existing token field, now with a
  second "write token" input. A `role` badge in the header; write-only controls are hidden/disabled
  for `read`.
- **Run page** (`/w/:id/run`): the command catalogue as cards — name, a param form where needed, a
  Run button (disabled for `is_write` commands when `role !== "write"`). Submitting navigates to
  the run detail.
- **Run detail** (`/runs/:runId`): status, per-stage rows for `pipeline`, a `<pre class="log">`
  fed by the SSE stream with autoscroll, and a Cancel button (write role, while non-terminal).
- **Runs list** (`/w/:id/runs`): recent runs, status chips, link to detail.

No new heavy deps (no xterm.js). `EventSource` is built in.

---

## 6. Testing

**`web/api`** (pytest)
- Auth: token mode — read token gets `read`, write token gets `write`, no `CONSOLE_WRITE_TOKEN`
  means write is never granted; `require_role("write")` returns `403` for a read principal.
  OIDC — the claim→role mapping is a pure function, unit-tested with a synthetic ID-token payload;
  the login redirect carries `state`/PKCE; `me` is `401` with no session.
- Allowlist: an unknown command name is `404`; a `is_write` command with a read principal is
  `403`; `params` that don't match the schema are `422`.
- Runner (live, `EKOS_BIN`): `doctor` runs to `succeeded` with log output; `pipeline` on a fresh
  workspace runs all stages; a long command is cancelled and lands `cancelled`; two writes on one
  workspace serialise (the second starts only after the first ends); startup sweeps a stale
  `running` row to `interrupted`.
- SSE: the stream replays existing lines and terminates with `event: end`.

**`web/ui`**: `tsc` + `vite build`; a render test of the run detail against a mocked SSE stream.

---

## 7. Verification

- Rust workspace gate unaffected (Phase 3 is console-only — no Rust changes).
- `web/api`: `ruff` + `pytest` green (unit + `EKOS_BIN`-gated runner/SSE).
- `web/ui`: `tsc` clean, `vite build` succeeds.
- End to end (recorded in the phase devlog, token mode): register a scratch workspace, run
  `pipeline`, watch the log stream stage by stage in the browser, cancel a re-run mid-`build`,
  see it in the history as `cancelled`.

---

## 8. Files changed (projected)

| File / area | Change |
|---|---|
| `web/api/app/auth.py` | New — `Principal`, OIDC + token modes, `require_role` |
| `web/api/app/commands.py` | Stub → the real `COMMAND_ALLOWLIST` |
| `web/api/app/runner.py` | Stub → `JobRunner` |
| `web/api/app/_proc.py` | `run_streaming(argv, cwd, log_path, on_line)` added |
| `web/api/app/models.py` | `Run` table + the startup sweep |
| `web/api/app/routes/{auth,commands,runs}.py` | New |
| `web/api/app/routes/*.py` (existing) | `require_console_token` → `require_role("read")` |
| `web/api/app/main.py` | `SessionMiddleware`, `JobRunner` on lifespan, auth router |
| `web/api/pyproject.toml` | `authlib`, `itsdangerous` |
| `web/ui/src/pages/{Run,RunDetail,Runs}.tsx`, `Layout.tsx` | New pages + the auth gate |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Phase 3 note |

---

## 9. Open questions

1. **Per-workspace queue depth.** Default 16; a deployment that batches many `ekl` runs might want
   more. A config knob if anyone hits it.
2. **Log retention.** `.ekos-web/runs/*.log` grows unbounded. A max-age / max-count sweep on
   startup is a one-liner when it matters; not in this RFC.
3. **`init` from the console.** Deferred — a workspace has to exist (`ekos.toml` + `.ekos/`) to be
   registered, and `init` on an arbitrary browser-supplied path is exactly the traversal risk
   §8.4 rule 3 guards against. A "scaffold a new workspace" flow needs its own design.

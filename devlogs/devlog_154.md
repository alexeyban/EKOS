# Devlog 154 — RFC 0131: Web Console Phase 3 (command runner, job runner, OIDC auth)

**Date:** 2026-09-03
**Branch:** `feat/0131-web-console-phase-3` → `main` (local merge, `[skip ci]`, local gates only)
**RFC:** `ekos/docs/rfcs/0131-web-console-phase-3.md`

---

## Summary

The console can now *run* EKOS pipeline commands from the browser and watch them stream. That
first browser mutation brings the read/write role split the earlier phases deferred. Console-only
— no Rust changes.

- **Auth** — OIDC (Authorization Code + PKCE via `authlib`, signed session cookie, a claim → the
  `write` role) with a **two-static-token fallback** (`CONSOLE_TOKEN` = read, `CONSOLE_WRITE_TOKEN`
  = write) when `OIDC_ISSUER` is unset. Both modes end in a session cookie so `EventSource` works.
- **Command allowlist** — a hardcoded `COMMAND_ALLOWLIST` (15 commands); hardcoded argv, never a
  shell, `is_write` per command.
- **Job runner** — one bounded queue + one worker task per workspace (the single worker
  serialises, which is what RFC 0104's write lock requires), stdout+stderr streamed to
  `.ekos-web/runs/<id>.log`, SIGTERM→SIGKILL cancel, chained `pipeline` with per-stage status,
  startup sweep of stale `running` rows.
- **UI** — an auth gate, a command-catalogue page, a run-detail page with a live `<pre>` fed by
  SSE and a cancel button, and a run-history list.

---

## PR — auth (`web/api/app/auth.py`, `routes/auth.py`)

`Principal{subject, email, role}` resolved per request. `require_role("read")` accepts read or
write; `require_role("write")` needs write (`401` unauth, `403` under-privileged). `check_role`
is the framework-independent core so `routes/commands.py` can gate per-command inside the handler.

- **OIDC:** `authlib` discovers `<issuer>/.well-known/openid-configuration`, does the code
  exchange, and `SessionMiddleware` (`SESSION_SECRET`) holds the post-login `{sub, email, role}`.
  `role_for_claims(claims, OIDC_ROLE_CLAIM, OIDC_WRITE_VALUES)` — a pure function — maps a claim
  value to `write`; an empty `OIDC_WRITE_VALUES` makes the deployment read-only for everyone.
  `/api/auth/{login,callback,logout,me}`.
- **token:** `POST /api/auth/token-login {token}` validates against the two static tokens and
  sets the session cookie; `Authorization: Bearer` is also accepted directly (curl / tests).
- **Every existing route** moved `require_console_token` → `require_role("read")`; `POST`/`DELETE
  /workspaces` and `PUT /config` are now `require_role("write")`.

## PR — command allowlist (`web/api/app/commands.py`)

`COMMAND_ALLOWLIST` filled in (empty since Phase 0). `Command(name, base_argv, is_write, timeout,
params, stages)`. `render_argv(params)` appends validated params as argv elements — bool params
become `--flag`, string params become `--name value` (rejected if they contain a NUL). The only
string param in the list is `ekl`'s `query`. `pipeline` has `stages = (build, recover, resolve,
compile, commit)` and no `base_argv`. `catalogue()` is the UI-facing description.

## PR — job runner (`web/api/app/runner.py`, `_proc.run_streaming`, `models.Run`)

- **Per-workspace:** `_queues[ws] : asyncio.Queue(maxsize=16)` + `_workers[ws] : Task`. The
  single worker naturally serialises everything on that workspace — a full-pipe deadlock or a
  concurrent-write conflict (RFC 0104) can't happen. `QueueFull` → HTTP 429.
- **`_proc.run_streaming`** merges stdout+stderr, strips ANSI, appends each line to the log file,
  hands the live process to a `register` callback (for cancel), and enforces a per-command
  timeout with `asyncio.timeout` (→ exit 124 → status `timed_out`).
- **Cancellation:** `_running : {run_id: Process}`; `cancel` SIGTERMs → SIGKILLs, or marks a
  still-queued run `cancelled` so the worker skips it.
- **`pipeline`:** `_run_chain` iterates stages, writes a `[console] === stage: X ===` banner,
  updates `Run.stages[i].status`, stops on the first non-zero exit and marks the rest `skipped`.
- **`Run` table:** `status ∈ {queued, running, succeeded, failed, cancelled, timed_out,
  interrupted}`. `sweep_stale_runs()` on startup moves `queued`/`running` → `interrupted`.

## PR — SSE + routes (`routes/{commands,runs}.py`)

`GET /api/runs/{id}/logs` → `text/event-stream`: emits existing lines, then polls the file every
250 ms, emits new lines, and closes with `event: end` once the run is terminal. No websocket.

## PR — UI

- **`Layout.tsx`** — `GET /api/auth/me`; `401` → a "Sign in with SSO" button (OIDC) or a token
  field (`mode: "token"`, from the 401 body). Header shows `email · role` + sign out.
- **`/w/:id/run`** — the catalogue as cards, a param form where needed, a Run button disabled for
  `is_write` commands unless `role === "write"`.
- **`/runs/:runId`** — status + per-stage rows, a `<pre class="log">` fed by an `EventSource`
  with autoscroll, a Cancel button (write, while non-terminal).
- **`/w/:id/runs`** — recent runs with status chips.
- `client.ts` switched from a `localStorage` Bearer header to `credentials: "include"` — the
  session cookie is the single mechanism, which is what makes `EventSource` work.

---

## Verification

Local gates only (`[skip ci]`):

- Rust: unchanged by this phase; `fmt` + `clippy --workspace` still clean.
- `web/api`: `ruff` clean, `pytest` **58/58** with `EKOS_BIN` (26 new: auth token/OIDC-mapping
  units, allowlist gating, `JobRunner` unit tests, and async runner live tests — `doctor` /
  `pipeline` to `succeeded`, SSE replay+end, per-workspace serialisation, cancel a running
  pipeline, startup sweep).
- `web/ui`: `tsc -b --noEmit` clean, `vite build` succeeds.
- **End to end** (token mode) against this repo: `token-login` (read) → `/commands` lists 15,
  `POST .../commands/build` → `403`; `token-login` (write) → `POST .../commands/doctor` →
  `succeeded`, `exit 0`, real `[OK] …` log lines; `GET /api/runs/<id>/logs` replays the log and
  ends.

---

## Knowledge Captured

- **The chained-`pipeline` runner had a latent deadlock-shaped bug.** `_execute` opened the log
  file (via `_run_chain`'s stage banner) *before* `_proc.run_streaming` created the parent
  directory, so the first `pipeline` run raised `FileNotFoundError`; and because `final` was only
  bound on the success paths, the `except` couldn't set a status — the run sat at `running`
  forever and `_wait_terminal` polled until timeout. Fix: `mkdir` the log parent in `_execute`
  up front, default `final = "failed"`, and wrap the body in `except Exception`. Any future
  runner path that can raise must not leave `final` unbound.
- **A long background asyncio task can't be tested through the sync `TestClient`.** Between
  `client.get()` calls the event loop isn't running, so the runner's worker doesn't drain the
  subprocess pipe → the child blocks on a full pipe → deadlock. The runner tests use
  `httpx.AsyncClient(ASGITransport)` + `app.router.lifespan_context` so `await asyncio.sleep`
  yields to the worker continuously. `doctor` (sub-second) would pass either way; `pipeline`
  needs the continuous pump.
- **`EventSource` can't set headers** — only cookies. That forced both auth modes to end in a
  session cookie rather than the Phase 0–2 `localStorage` Bearer. Bearer stays supported for
  curl/tests in token mode.
- **The per-workspace single worker serialises *all* commands, not just writes.** A quick `status`
  queues behind a long `build`. RFC 0104 only requires write serialisation; relaxing reads to run
  concurrently is a Phase 7 refinement.
- **`authlib`'s starlette client stores PKCE/state/nonce in `request.session`** — it needs
  `SessionMiddleware` installed before any auth route is hit, and `session_secret` must be stable
  across restarts or every existing session is invalidated.

---

## Files Changed

| File | Change |
|---|---|
| `web/api/app/auth.py` | New — `Principal`, OIDC + token, `require_role` / `check_role` |
| `web/api/app/routes/auth.py` | New — `/api/auth/{me,login,callback,logout,token-login}` |
| `web/api/app/commands.py` | Stub → `COMMAND_ALLOWLIST` + `render_argv` + `catalogue` |
| `web/api/app/runner.py` | Stub → `JobRunner` |
| `web/api/app/_proc.py` | `run_streaming` + `strip_ansi` |
| `web/api/app/models.py` | `Run` table + `sweep_stale_runs` + run CRUD |
| `web/api/app/routes/{commands,runs}.py` | New |
| `web/api/app/routes/{workspaces,stats,config,graph}.py` | `require_console_token` → `require_role(...)`; write gates |
| `web/api/app/deps.py` | `require_console_token` removed (moved to `auth`) |
| `web/api/app/main.py` | `SessionMiddleware`, `JobRunner` on lifespan, auth/commands/runs routers |
| `web/api/app/settings.py` | OIDC + `CONSOLE_WRITE_TOKEN` + `SESSION_SECRET` + runner settings |
| `web/api/pyproject.toml` | `authlib`, `itsdangerous`, `httpx` |
| `web/api/tests/` | `test_auth.py`, `test_commands.py`, `test_runner_live.py` new; `test_api.py` + live fixtures updated for roles |
| `web/ui/src/pages/{Run,RunDetail,Runs}.tsx` | New |
| `web/ui/src/{Layout,main,api/client,index.css,pages/Dashboard}.tsx` | Auth gate, routes, cookie auth, styles |

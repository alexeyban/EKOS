# Devlog 268 — Console settings from the console, local mode, OS keychain (RFC 0184)

**Date:** 2026-10-14
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

First of four phases toward an installer for Linux, Windows 11 and macOS with everything
configurable from the web and a chat (RFC 0184–0187, plan agreed 2026-10-14). The console took
its settings only from environment variables, had no sign-in suitable for one person on a
laptop, and had never been prepared for Windows. Now a platform owner edits the settings on a
Settings page (saved to `<home>/control/console.toml`, the environment still winning), secrets
and model keys go to the OS keychain, and a local mode signs in by a one-time code written into
the home.

---

## RFC 0184

### What was built

| Piece | What it does |
|---|---|
| `app/console_file.py` | `FIELDS` (group, help, secret, editable for every `Settings` field), the file's path, `FileSource` / `KeychainSource` settings sources, `tomlkit` write that keeps comments, `reading()` to validate a would-be file |
| `app/keychain.py` | `keyring` wrapper: `env:<NAME>` and `console:<field>` entries under service `ekos`, an `index` entry listing names, `available()`, `EKOS_CONSOLE_KEYCHAIN=off` |
| `app/routes/platform.py` | Owner-only: `GET/PUT /platform/settings`, `POST /platform/restart`, `GET/PUT/DELETE /platform/keychain[/{name}]` |
| `app/local_signin.py` | One-time codes: file named by SHA-256 under `control/local-signin/`, two minutes, deleted on use |
| `settings.py` | `mode`, `host`, `port`, `ui_dir`, `supervised`; precedence env > dotenv > keychain > file > default; local mode refuses OIDC and production; paths via `pathlib` |
| `auth.py`, `routes/auth.py` | Principal kind `local` (owner); `GET/POST /api/auth/local`; static tokens refused in local mode |
| `main.py` | `LoopbackOnly` middleware in local mode (client address and `Host` header); UI directory from `ui_dir` |
| `envs.py` | Windows base variables; granted variables filled from the keychain |
| UI | `Settings.tsx` (grouped form, secrets as set/not set, keychain names), local sign-in screen, header link for owners |
| Image, CI | `EKOS_CONSOLE_KEYCHAIN=off`, `EKOS_CONSOLE_SUPERVISED=true`; `web-os` job on `windows-latest` and `macos-14` (informational) |

### Implementation details worth remembering

- **A field with a `validation_alias`** (`oidc_issuer` → `EKOS_CONSOLE_OIDC_ISSUER`, `ekos_bin` →
  `EKOS_BIN`) must be returned by a custom settings source **under the alias**, or the value is
  silently dropped. `console_file._key` does it.
- **Restart** marks `control/restart-requested` and raises `SIGTERM` in the process
  (`signal.raise_signal`), so uvicorn shuts down cleanly and the lifespan stops the MCP servers.
  `os._exit` would have orphaned them. A launcher (RFC 0185) or a restart policy starts it again.
- A save is validated by building `Settings` with the would-be file content
  (`console_file.reading`, a context variable read by `FileSource`), so production-without-OIDC
  or local-with-OIDC is refused before anything is written. The value written is the validated
  one, so `"300"` sent for a number lands as `300`.
- The keychain can't list entries, so the names live in one extra entry (`index`). No database
  table, no migration.

### Decisions

- **Every setting needs a restart** rather than some applying live: the session middleware,
  the OIDC client and the supervisor read settings once, and a half-applied change is worse than
  a clear "restart to apply".
- **Not editable from the console:** where it is and where its state lives (`ekos_bin`,
  `ekos_home`, `console_db`, `runs_dir`, `ui_dir`, `supervised`). A page served by the console
  must not be able to point it at another database.
- **Local sign-in proves write access to the home** instead of using a token in a URL or a
  password: the person who owns the install is exactly who can write `control/`.

---

## Knowledge Captured

- **A `.env` in the working directory ranks with the environment** (pydantic-settings
  `env_file`). A developer's `web/api/.env` set the session secret and made a precedence test
  fail; "set by the environment" must count `.env` keys too (`console_file._dotenv_keys`).
- **`TestClient(client=(host, port))`** (Starlette 1.6) sets the client address, which a
  loopback check needs: the default `testclient` is not an IP. Opening a second
  `with TestClient(app)` on the same app reruns the lifespan and fails in APScheduler; use one
  without `with` for a second cookie jar.
- `keyring` on this machine resolves to `SecretService.Keyring`; it was only detected, not
  written to. Tests replace the backend with an in-memory one in an autouse fixture.

---

## Verification

Console API 358 passed (with `EKOS_BIN`), UI 97 passed, `tsc` clean, ruff clean. The image
and `deploy/e2e/run.sh` are checked after this commit. Not run: Windows, macOS (CI job added,
not triggered — needs a push).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0184-console-settings-local-mode-keychain.md` | New RFC |
| `web/api/app/{console_file,keychain,local_signin}.py`, `routes/platform.py` | New |
| `web/api/app/{settings,auth,envs,main}.py`, `routes/auth.py` | Sources, local mode, keychain fill, Windows variables, loopback guard |
| `web/api/tests/test_{console_settings,local_mode}.py`, `conftest.py`, `test_envs.py`, `test_authz_routes.py` | Tests; in-memory keychain fixture |
| `web/ui/src/pages/Settings.tsx` (+ test), `Layout.tsx`, `main.tsx`, `api/client.ts`, `index.css` | Settings page, local sign-in |
| `Dockerfile`, `.github/workflows/ci.yml`, `deploy/.env.example` | Image settings, Windows/macOS job |
| `README.md`, `docs/deployment.md`, capabilities page, `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Docs |

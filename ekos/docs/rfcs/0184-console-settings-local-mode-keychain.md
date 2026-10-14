# RFC 0184 — Console settings in a file, local mode, secrets in the OS keychain

**Status:** Accepted — implemented 2026-10-14 (devlog_268)
**Author:** EKOS team
**Created:** 2026-10-14
**Builds on:** RFC 0131 (console auth), RFC 0179 (console state under the home), RFC 0180 (owners, roles), RFC 0183 (per-tenant environments)
**Part of:** installer and web configuration (0184 this RFC, 0185 installer, 0186 every setting and command from the web, 0187 chat)

## Motivation

The console takes its own settings only from environment variables. That suits a container and
nothing else: a person who installed EKOS on a laptop cannot change the port, sign-in or MCP
limits without editing a service definition, and nobody can do it from the console itself.

The console has two ways to be signed in to, OIDC and development tokens, and neither fits one
person on their own machine: OIDC needs a provider, and the development token is a published
default. A desktop install needs a third mode that is safe on a shared machine.

A model key is given to a project by naming an environment variable (RFC 0183). On a desktop
there is nowhere natural to set that variable for a service started at login, and writing the
key into a file defeats the point. Every desktop OS has a keychain for exactly this.

Finally the console has never run on Windows or macOS. The installer (RFC 0185) ships it there.

## 1. `console.toml`

`<EKOS_HOME>/control/console.toml` (or `EKOS_CONSOLE_CONFIG_FILE`) holds console settings, one
key per `Settings` field, flat, named as the field (`oidc_issuer = "…"`). Precedence, highest
first:

1. environment variables (`EKOS_CONSOLE_*`, `EKOS_BIN`, `EKOS_HOME`), as today;
2. console secrets in the keychain (§2), for the secret fields only;
3. `console.toml`;
4. defaults.

So a container or an existing deployment, configured by environment, behaves exactly as before,
and a value set in the environment cannot be changed from the console: it is shown as "set by
the environment" and is read-only.

Each field carries metadata (`app/console_file.py::FIELDS`): group, help text, whether it is a
secret, and whether it is editable from the console. Not editable: `ekos_bin`, `ekos_home`,
`console_db`, `runs_dir`, `workspaces_json`, `ui_dir` (where the console is and where its state
lives — the installer sets them). Secret fields (`oidc_client_secret`, `session_secret`,
`console_token`, `console_write_token`, `mcp_token`) are never written to the file by the
console and never returned to a browser; a response says only whether each is set and where.

Every setting is read once at start, so a change applies at the next start. The response of a
save says so, and `POST /api/platform/restart` restarts the console when it runs under a
supervisor that restarts it (`EKOS_CONSOLE_SUPERVISED=1`: the `ekos console` launcher of RFC
0185, or a container with a restart policy). Otherwise it answers 409 with that reason.

Saving validates the result first: the file's new content, overlaid by the environment, must
build a valid `Settings` (production mode still needs OIDC, owners and a session secret, local
mode refuses OIDC), or nothing is written. The file is edited with `tomlkit`, so a comment a
person wrote stays.

Routes, owner only (`authz.allow_platform`):

| Route | Does |
|---|---|
| `GET /api/platform/settings` | Every field: value (not for secrets), source (`environment`, `keychain`, `file`, `default`), editable, secret, set, group, help |
| `PUT /api/platform/settings` | `{"values": {...}, "secrets": {...}}`; a `null` removes a key from the file (or a secret from the keychain) |
| `POST /api/platform/restart` | Restart when supervised; 409 otherwise |
| `GET /api/platform/keychain` | Whether a keychain is usable, its kind, and the names stored |
| `PUT /api/platform/keychain/{name}` | Store a value under an environment-variable name |
| `DELETE /api/platform/keychain/{name}` | Remove it |

## 2. Secrets in the OS keychain

`app/keychain.py` wraps the `keyring` library: Windows Credential Manager, macOS Keychain,
Secret Service (GNOME Keyring, KWallet) on Linux. Service name `ekos`; two namespaces of entry:

- `env:<NAME>` — a value for an environment variable a project names (`ANTHROPIC_API_KEY`);
- `console:<field>` — a console secret field.

`keyring` cannot list entries, so the names stored are kept in one more keychain entry,
`index` (JSON, names only): no database table, and the list travels with the keychain. A value
is never returned by any route, never written to a log; the audit log records only the request
path, which holds the name.

**Where values go.** `envs.for_tenant` (RFC 0183) builds every child process's environment. A
variable the tenant is granted (in its `model_env`, while `allow_metered` is on) and that the
console's environment lacks is filled from `env:<NAME>` in the keychain. A tenant not granted a
name never receives its value; the RFC 0183 tests apply unchanged.

**Unusable keychain.** A container or a headless server usually has none (no D-Bus session).
`keychain.available()` says so; the console then refuses to store and the UI tells the person
to set an environment variable instead. `EKOS_CONSOLE_KEYCHAIN=off` disables it outright (the
image sets this). Tests run against an in-memory backend, never the developer's real keychain.

## 3. Local mode

`mode = "local"` (`EKOS_CONSOLE_MODE`), for one person on their own machine:

- **No sign-in provider.** OIDC settings are refused (a misconfiguration, not a fallback), as is
  production mode. The two static development tokens are not accepted.
- **Signing in proves access to the home.** `ekos console open` (RFC 0185) writes a random
  one-time code's SHA-256 as a file name under `<home>/control/local-signin/` and opens
  `http://127.0.0.1:<port>/api/auth/local?code=<code>`. The console accepts a code whose file
  exists and is younger than two minutes, deletes the file (single use) and sets the session
  cookie. Only someone who can write the home's control directory — the person who owns the
  install — can sign in. The page also accepts a pasted code for a browser on the same machine
  that did not open the link.
- **The person is a platform owner** (`Principal.kind = "local"`), with every role everywhere.
  API tokens for agents work as in team mode.
- **Loopback only.** Each request must come from a loopback address and carry a `Host` header
  naming a loopback name (`127.0.0.1`, `localhost`, `[::1]`), else 403. The `Host` check stops a
  web page on another site from reaching the console through DNS rebinding; the session cookie
  is `SameSite=Lax`, so another site cannot post with it.

Team mode (`mode = "team"`, the default) is today's behaviour, unchanged.

## 4. Running on Windows and macOS

- `envs.BASE` gains what a Windows process needs to start and find its files: `SYSTEMROOT`,
  `WINDIR`, `COMSPEC`, `PATHEXT`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `PROGRAMDATA`,
  `HOMEDRIVE`, `HOMEPATH`, `USERNAME`. Without `SYSTEMROOT` a child process on Windows fails
  before `main`.
- Paths are built with `pathlib`; `settings.py` no longer joins with `/`.
- `_proc.terminate` already works on Windows: `send_signal(SIGTERM)` is `TerminateProcess`
  there.
- The built UI's directory is a setting (`ui_dir`, `EKOS_CONSOLE_UI_DIR`), since a package does
  not keep the repository's layout.
- `.github/workflows/web.yml` gains Windows and macOS rows for the console's tests.

## Tests (written first)

- Precedence: environment over keychain over file over default; an environment value is
  reported read-only and a save cannot change it.
- A save with an invalid result writes nothing; comments in the file survive a save.
- No secret value in any response of the platform routes, in the console database, in the file
  or in the audit log (a marker value is searched for).
- Keychain values reach a child process only for granted names of the right tenant, and only
  while metered calls are on.
- Local mode: a code works once; an expired code, a reused code, a wrong code fail; a foreign
  `Host` or a non-loopback client is refused; OIDC or production with local mode fails to
  start; static tokens are refused.
- The route-walk test covers the new routes (owner only).

## Not in this RFC

The launcher, the installer and service registration (RFC 0185); editing project configs and
running every command (RFC 0186); chat (RFC 0187).

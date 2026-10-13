# Devlog 265 — End-to-end check of the production plan

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

RFCs 0178–0183 were each tested alone, and four things had never been run together: the image
rebuilt from the committed tree, a real OIDC provider, two tenants over the API and the MCP
gateway, and a restore into an empty volume. `deploy/e2e/run.sh` now does all of it against
Dex and passes 108 checks. The run found four defects, fixed here: three sign-in failures
answered 500, the session cookie was never marked `Secure`, and the guide's restore steps did
not work on a new volume.

---

## The check

### What was built

| Part | Where |
|---|---|
| Orchestration: fixture sources, Dex config, Compose override, five stages, cleanup | `deploy/e2e/run.sh` |
| Sign-in through the provider's form, run a command, call the gateway | `deploy/e2e/lib.py` |
| Stages | `s1_setup.py` (21 checks), `s2_pipeline.py` (16), `s3_scope_gateway.py` (56), `s4_ids.py` (5), `s5_restore.py` (10) |

### What it showed working

- Sign-in through Dex for five people; a role granted to an address before first sign-in is
  held at first sign-in; a person with no role sees nothing.
- A tenant admin cannot widen source roots, name model variables, or make a project over
  another tenant's sources or `/etc`.
- A two-source project made with the CLI appears after a restart; pipelines run from the API.
- Everything of another tenant is 404: workspace routes, admin routes, the gateway.
- A viewer denied `handbook` `hr/**` gets nothing of it from search, the graph, any scoped MCP
  tool, or a hidden object's id; the unrestricted control gets it from the same calls.
- No token is offered or can call a review tool; a token cannot be made or granted reviewer or
  admin; a client-supplied scope does not replace the caller's.
- The audit log has every gateway call by token name, refusals included, and no arguments.
- Each project's server process holds its own tenant's model variable and no `EKOS_CONSOLE_*`.
- A full backup restored into a new volume: roles, limits, tokens, audit log and ledgers are
  back, and the pipeline runs again.

### Defects found and fixed

| Defect | Fix |
|---|---|
| `GET /api/auth/login` was 500 when the provider could not be reached | 502 naming the issuer setting (`routes/auth.py`) |
| `GET /api/auth/callback` was 500 when the person cancelled or the state did not match | 400, recorded as a denied sign-in |
| The session cookie was never `Secure` | `Secure` when the redirect URI is `https://` (`main.cookie_is_https_only`) |
| The guide said to restore into an empty home without `--force`; a started console has already made its database, so the restore refused | Guide corrected (`docs/deployment.md`) |

### Decisions

- **Dex, not Keycloak.** About 10 MB of memory against several hundred, on a machine with 3 GB
  free. The price: Dex's static users carry no groups, so claim-mapped groups stay unverified.
- **`Secure` follows the redirect URI**, not production mode. A first try on plain loopback
  HTTP in production mode must still be able to sign in, and a `Secure` cookie would never
  come back.
- **The script does not read `deploy/.env`** (`env_file: !reset []`) and uses its own Compose
  project and volume, so it cannot touch a real deployment on the same machine.

---

## Knowledge Captured

- **A container on a Compose network could not reach `host-gateway` here**, though one on the
  default bridge could (host firewall). Put the test provider on the same Compose network and
  use its service name as the issuer; map that name to loopback in the test client.
- **The issuer URL must be one both the console and the browser can use.** The console fetches
  discovery from it and sends the browser to the endpoints it lists.
- **authlib caches provider metadata** after the first successful sign-in redirect, so
  stopping the provider afterwards does not reproduce "unreachable". Restart the console.
- **authlib checks `error` and `state` before any network call**, so those two callback
  failures are testable with no provider.
- **`ekos_retrieve` echoes the question in its plan.** A leak check must not search for a
  marker it sent. This false alarm appeared again here, as in RFC 0181's suite.
- **A test that sends the wrong argument name passes a leak check**: the tool answers with an
  error, which holds no hidden text. `s4_ids.py` counts malformed calls and requires the
  control to return the hidden text for the same ids.
- **The classic Docker builder keys the cache on build arguments.** Building once with
  `CARGO_BUILD_JOBS=4` and once without recompiles everything.
- **Without `EKOS_BIN`, 43 console tests skip silently** and the suite still reads as green
  (263 passed). The full count needs `EKOS_BIN=<repo>/ekos/target/debug/ekos`.
- Idle memory: console 107 MB, each project server about 21 MB on the fixture.

---

## Files Changed

| File | Change summary |
|---|---|
| `deploy/e2e/run.sh`, `lib.py`, `s1_setup.py` … `s5_restore.py` | New: the end-to-end check |
| `web/api/app/routes/auth.py` | 502 / 400 for sign-in failures |
| `web/api/app/main.py` | `cookie_is_https_only` |
| `web/api/tests/test_auth.py` | Five tests |
| `docs/deployment.md` | Restore steps, issuer reachability, pointer to the check |
| `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | Updated |

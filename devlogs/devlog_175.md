# Devlog 175 — the web console met a real read-only container

**Date:** 2026-09-10
**Commits:** `216cae9`, `903b93d`, plus this session's `doctor`/`fact_ledger`/`Layout` fix
**Branch:** main (local)

---

## Summary

Running the RFC 0127 web console against a real deployment — the `ekos` binary bind-mounted into
a `python:3.12-slim` container over a `:ro`-mounted workspace, no Rust toolchain — surfaced a
cluster of places where EKOS still assumed it was running inside its own build environment. The
read path wrote to the workspace (`903b93d`), Compose silently ignored the credential file
(`216cae9`), and `ekos doctor` reported a healthy workspace as broken because `rustc` was not on
`PATH`. Fixing the last one also uncovered a dead test that `903b93d` had shipped: an editing
slip left `open_read_only_rejects_every_write_method` without its `#[test]` attribute and a
duplicate `#[test]` on the function above it.

---

## PR `903b93d` — a read-only open wrote to the workspace

Already committed; recorded here because devlog_174 predates it and because it shipped a dead
test (see below). Every stats endpoint of the console failed against a `:ro`-mounted workspace
with `Read-only file system (os error 30)` despite only reading counts. Four causes, found one
behind the other by reproducing in a container each time:

1. `SegmentStore` had no read-only open — it created `segments/`, took a
   `.create(true).append(true)` handle on the active segment, and rewrote `HEAD`. Added
   `open_read_only_with_backend`; `active` became `Option<File>`.
2. `read_active_committed` treated a missing active segment as an error (the writable open had
   always created it first).
3. Any tantivy reader acquires `META_LOCK`, which writes a lockfile — no reload policy avoids it.
   A read-only open now degrades to an empty in-RAM index and logs loudly: counts, timelines,
   EKL aggregates and object reads still work; `query find` returns no hits and says why.
4. `ledger status`, `ekl`, `ask`, `query find` and `diff` all opened the store writably for pure
   reads — switched to `open_store_read_only`.

---

## PR `216cae9` — `api/.env` is now the single credential source

`web/api/.env` is what `app/settings.py` reads (pydantic `env_file`), but Compose ignored it —
and would have kept ignoring it if simply added, because Compose applies `environment:` *after*
`env_file:`, so the hardcoded `${CONSOLE_TOKEN:-dev-console-token}` defaults won. `docker-compose.yml`
now points `env_file:` at `./api/.env` and carries only container-specific values in
`environment:` (notably the absolute `EKOS_BIN` bind-mount path, which `.env`'s dev-relative one
would not resolve to inside the container). New `api/.dockerignore`.

---

## This session — `ekos doctor` no longer fails on a missing Rust toolchain

### Problem

Reported live from the console's own Doctor page:

```
[FAIL] Rust toolchain        rustc not found in PATH
```

`rustc` is a **build-time** dependency. If `ekos doctor` is running, the binary already exists,
and nothing in the codebase shells out to `rustc`/`cargo` outside this one check. Failing here
made `doctor` call a perfectly healthy workspace broken on the two most normal deployments — a
prebuilt binary, and the console container — and flipped `doctor --json`'s `ok` field, which the
console reads as its verdict.

### What was built

| Change | Detail |
|---|---|
| `rust_toolchain_check(Option<String>) -> Check` | Extracted from the inline `match` in `collect_checks`. `Some(v)` → `Check::ok` with the version; `None` → `Check::ok("not installed (only needed to build EKOS from source)")`. Never `Check::fail`. |
| Tests | `a_missing_rust_toolchain_is_reported_not_failed` (asserts `check.ok` *and* `build_doctor_json(..).ok`, so the `--json` verdict is covered), `a_present_rust_toolchain_still_reports_its_version`. |

Same reasoning as `llm_provider_check`'s not-configured case directly below it: an absent
optional thing is a fact to state, not a failure to raise.

### Dead test recovered in `fact_ledger.rs`

`903b93d` inserted `open_read_only_writes_nothing_to_the_workspace` by writing `#[test]`, then a
doc comment, then `#[test]` again — the new function got two `#[test]` attributes and the
existing `open_read_only_rejects_every_write_method` below it lost its own. Result: a
`duplicate_macro_attributes` warning (which `cargo clippy -- -D warnings` fails on) and one test
silently not running. Removed the duplicate, restored the attribute. `ekos-ledger`'s `open_read_only`
tests are back to 5 (were 4). The identical slip was in this session's own in-progress
`doctor.rs` edit and was fixed the same way.

---

## This session — "sign out" did nothing in the console

### Problem

Clicking **sign out** in the console left you signed in. The button's handler did the right
things server-side — `POST /api/auth/logout` pops `session["user"]`, Starlette sends the
cookie-deletion header — and the `me` query refetched and `/api/auth/me` correctly returned
`401`. But `Layout` decided the auth state from `me.data` alone, and React Query v5 **keeps the
last successful `data` on a failed refetch** (it sets `error` alongside, it does not clear
`data`). So `me.data` still held the old `{mode, email, role}` and the console stayed on the
authenticated view. The `Layout` component had no test.

### Fix (`web/ui/src/Layout.tsx`)

```ts
const unauthorized = me.error instanceof ApiError && me.error.status === 401;
const identity = unauthorized ? undefined : me.data;
```

A **401** is the definitive not-signed-in signal and collapses `identity` to `undefined` →
the sign-in screen. Any *other* error (500, network blip) is left as-is: the stale `me.data`
keeps a still-authenticated operator in place rather than bouncing them to sign-in on a
transient failure. The logout handler now `await`s `qc.invalidateQueries()` so the `me` refetch
(and its 401) lands before the click settles. New `web/ui/src/Layout.test.tsx` covers it.

---

## Knowledge Captured

- **`ekos doctor` runs in environments that never built EKOS.** The console ships the binary into
  a Python container. Any check that assumes a build toolchain, a source tree, or a writable
  workspace is wrong there. `doctor --json`'s `ok` field is a machine verdict the console trusts —
  a spurious `fail` is not cosmetic.
- **A tantivy reader always writes a lockfile** (`META_LOCK`), no matter the reload policy. A
  genuinely read-only ledger open cannot use the real index; degrading to an empty in-RAM index
  and logging is the accepted approximation (same as RFC 0111 §7 for an unsynced partition).
- **Editing hazard: `#[test]` + a doc comment + `#[test]`.** Inserting a doc-commented test
  immediately after an existing `#[test]` line, without deleting that line, double-attributes the
  new function and orphans the old one. The orphan compiles as dead code and just stops running;
  the duplicate trips `duplicate_macro_attributes`, so `cargo clippy -- -D warnings` catches it —
  but only if clippy actually runs (both these landed with `[skip ci]`-style local merges). Grep
  for `^\s*#\[test\]\s*$` followed by `///` after any test insertion.
- **docker-compose applies `environment:` after `env_file:`.** A hardcoded `${VAR:-default}` in
  `environment:` silently overrides the same key from an `env_file`, so adding the file changes
  nothing until the `environment:` entry is also removed.
- **React Query v5 keeps `data` on a failed refetch.** A query that succeeded once, then errors
  on refetch, has *both* `data` (stale, last-good) and `error` set. Any component that gates on
  `query.data` alone will not react to the failure — for auth state that means "sign out" or a
  session expiry does nothing visible. Gate on the error too (here: specifically a 401).
- **The Compose `ui` service writes root-owned files into the host tree.** `image: node:20-slim`
  runs `npm ci` as root over the `./ui` bind mount, leaving `node_modules/.vite/` (and `.deps`)
  owned by root — after which a host-side `vitest`/`vite` run fails with `EACCES` on the cache
  dir. Workaround for a host run: `--config` a throwaway vite config with `cacheDir` pointed
  outside the tree. Worth a proper fix (a named volume for `node_modules`, or a non-root user).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/doctor.rs` | New `rust_toolchain_check`; a missing toolchain is `ok`, not `fail`; two tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | Removed a duplicate `#[test]`, restored the one on `open_read_only_rejects_every_write_method` |
| `web/ui/src/Layout.tsx` | Auth state now collapses to signed-out on a 401 from `/auth/me`, not just on absent `me.data`; logout awaits the refetch |
| `web/ui/src/Layout.test.tsx` | New — covers the sign-out → sign-in transition |

# Devlog 264 — Backup, restore, model credentials per tenant (RFC 0183)

**Date:** 2026-10-13
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

Phase F, the last of the production plan agreed on 2026-10-13. `ekos home backup` writes the
whole EKOS home as one archive and `ekos home restore` puts it back without ever deleting.
In the console, each tenant's commands now receive only the model-key variables an owner named
for that tenant, and none of the console's own settings. A deployment guide covers the whole
plan, RFC 0178 to 0183.

---

## RFC 0183

### Problem / motivation

Everything a deployment holds is in one directory and nothing said how to copy it safely. A
ledger cannot be rebuilt without re-running the pipeline, which costs money where model-backed
passes are on.

Every command the console started inherited the console's whole environment. A project names
the variable holding its model key in its own `ekos.toml`, so one tenant's project could name
another tenant's variable and spend it. The console's session secret and OIDC client secret
were in that environment as well.

### What was built

| Part | Change | Where |
|---|---|---|
| Backup | `ekos home backup [--out]`, with the global `--tenant`: a zstd tar with a manifest, projects, and `control/` | `ekos/crates/cli/src/commands/home_backup.rs` (new) |
| Restore | `ekos home restore <file> [--force]`: staging directory, checked paths, replaced things moved to `trash/` | same |
| Write lock | `fact_ledger::hold_write_lock` | `ekos/crates/ledger/src/fact_ledger.rs` |
| Environments | `envs.for_tenant` / `for_path`: a base list plus the tenant's named variables | `web/api/app/envs.py` (new) |
| Every spawn | `_proc.spawn` and `run_streaming` take `env`; six call sites pass it | `_proc.py`, `readproc.py`, `semantics_write.py`, `runner.py`, `supervisor.py`, `routes/workspaces.py` |
| Tenant settings | `model_env`, `allow_metered` (migration 3); `PATCH /api/tenants/{t}` | `models.py`, `routes/tenants.py` |
| UI | Model credentials and the metered switch on the owner's tenant card | `pages/Admin.tsx` |
| Guide | Install to upgrade, and what is not built | `docs/deployment.md` (new) |

### Implementation details worth remembering

- **All write locks are taken before the first byte is written.** A project in use fails the
  backup while there is nothing to clean up. The archive is written as `.partial` and renamed.
- **The console database is copied with `VACUUM INTO`**, which is consistent against a running
  writer. Copying the file is not.
- **`write.lock` is not archived.** It is how the copy is protected, not part of the state.
- **Restore decides everything before moving anything.** One existing project stops the whole
  restore; the others are not restored either.
- **Restore reads tar entries itself** instead of calling `unpack`: an entry must be a plain
  file or a directory, its path made of normal components, and under `data/` or `control/`.
  The manifest must be the first entry.
- **The environment is an allow-list.** A deny-list would pass any key nobody had registered.
- **`OLLAMA_HOST` is in the base.** A tenant with metered calls off can still use a local
  model server, which needs no key.
- **A stored variable name is checked again when the environment is built.** A row naming
  `EKOS_CONSOLE_SESSION_SECRET` grants nothing.

### Decisions

- **`tar` + zstd, not a format of our own.** An operator can list an archive with standard
  tools. One new dependency.
- **A per-tenant backup leaves `control/` out.** The console database holds every tenant's
  members and token hashes.
- **Metered calls off for a new tenant.** Nobody spends until an owner says so.
- **The `default` tenant keeps its environment outside production mode**, minus the console's
  own settings and other tenants' named variables. An existing single-tenant install keeps its
  model key after an upgrade.
- **No encryption and no schedule.** The host has tools for both.

---

## Verification

| Check | Result |
|---|---|
| Backup and restore (`cli/tests/home_backup.rs`) | 5 pass, through the real binary: restore into an empty home returns the same object ids; a project being written is refused and no file is left; nothing replaced without `--force`, the old project kept in the trash; one tenant alone; five hostile or broken archives refused with nothing written |
| `cargo test --workspace --no-fail-fast` | 2,842 pass |
| clippy, fmt, `tests/integration`, `scripts/audit.sh` | clean; 5 pass; exit 0 |
| Environments (`test_envs.py`) | 23 pass: who gets what as a table; a real process started by the runner sees its tenant's key and not another's; a scan for a spawn without `env=` |
| Whole console API suite | 301 pass (277 before) |
| Console UI | 93 pass |

Not verified: restoring an archive written by a different EKOS version; a backup of a
partitioned or SQLite-backed project under load (those have no lock, and the command says so).

---

## The plan as a whole

| RFC | What | Devlog |
|---|---|---|
| 0178 | EKOS home, projects, named sources | 258, 259 |
| 0179 | Installer, production image, console runs home projects | 260 |
| 0180 | Tenants, users, groups, roles, API tokens, audit | 261 |
| 0181 | Read scope inside a project, enforced at the store | 262 |
| 0182 | MCP gateway, servers on demand | 263 |
| 0183 | Backup, restore, model credentials, guide | 264 |

Never run across the whole plan: a real OIDC provider, a real MCP client against the gateway,
the console in a browser, the image rebuilt since RFC 0179, the `image` job on GitHub.

---

## Knowledge Captured

- **A fake binary that writes "where it was started" writes into the test runner's directory**
  when one caller starts it without a working directory. The MCP supervisor does. The fake now
  writes only inside a workspace.
- **Adding a keyword argument to a helper breaks every test that replaces the helper** with a
  fake of the old signature, and the runner turned the `TypeError` into a failed run and a
  timeout. Give fakes `**kwargs` or update them with the helper.
- **`fs4::FileExt` is at the crate root** in 0.8; there is no `fs_std` module.
- **The `tar` crate will not write a path with `..`** through `append_data`. A test for a
  hostile archive has to write the name into the header bytes.
- **`cli` gaining a dependency changes `tests/integration/Cargo.lock` and not
  `benchmark/Cargo.lock`**: the benchmarks do not depend on the CLI.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0183-operations.md` | New |
| `ekos/crates/cli/src/commands/home_backup.rs`, `tests/home_backup.rs` | New |
| `ekos/crates/cli/src/app.rs`, `commands/mod.rs`, `Cargo.toml` | `ekos home`; `tar`, `zstd`, `fs4`, `rusqlite` |
| `ekos/crates/ledger/src/fact_ledger.rs` | `hold_write_lock` |
| `ekos/Cargo.toml`, `ekos/Cargo.lock`, `tests/integration/Cargo.lock` | `tar` |
| `web/api/app/envs.py` | New |
| `web/api/app/{_proc,readproc,semantics_write,runner,supervisor,models}.py`, `routes/{workspaces,tenants}.py` | Environments; tenant settings |
| `web/api/tests/test_envs.py` | New |
| `web/api/tests/test_runner.py`, `test_tenants.py` | Fakes take `env`; migration 3 |
| `web/ui/src/pages/Admin.tsx`, `Admin.test.tsx`, `api/types.ts`, `index.css` | Model credentials |
| `docs/deployment.md` | New |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |

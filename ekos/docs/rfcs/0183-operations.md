# RFC 0183 — Operations: backup, restore, model credentials per tenant

**Status:** Accepted — implemented 2026-10-13 (devlog_264)
**Author:** EKOS team
**Created:** 2026-10-13
**Builds on:** RFC 0178 (the EKOS home), RFC 0179 (image, console state under the home), RFC 0180 (tenants)
**Part of:** production deployment (0178–0182, this RFC; the last of the plan)

## Motivation

A deployment keeps everything in one directory, the EKOS home, and nothing yet says how to
copy it safely or put it back. A ledger is append-only and cannot be rebuilt without re-running
the pipeline, which for a project with model-backed passes costs money.

Every command the console runs inherits the console's whole environment. With several tenants
that means a project of one tenant can name, in its own `ekos.toml`, the environment variable
holding another tenant's model key, and spend it. The console's own secrets (session secret,
OIDC client secret) are in that environment too.

## 1. Backup and restore

```
ekos home backup  [--tenant <t>] [--out <file>]
ekos home restore <file> [--tenant <t>] [--force]
```

### What a backup holds

A `tar` archive compressed with zstd:

```
ekos-backup.json                      manifest: format, time, ekos version, tenants → projects
data/<tenant>/<project>/…             each project: ekos.toml, state/, out/
control/console.db                    a consistent snapshot of the console database
control/runs/…                        run logs
```

- Without `--tenant`: every tenant, and `control/`.
- With `--tenant`: that tenant's projects only. `control/` is left out, because the console
  database holds every tenant's members and tokens.
- Not included: `bin/`, `trash/`, `backups/`, and the sources. Sources are not in the home.
- Default output: `<home>/backups/ekos-backup-<time>[-<tenant>].tar.zst`, mode 0600. An
  existing file is never overwritten.

### Consistency

- **A project is copied under its ledger's write lock.** A pipeline stage that is writing makes
  the backup fail for that project with its name, and nothing is written; a stage that starts
  during the copy waits or fails on the lock as it would against any other writer. Readers,
  including the console's MCP servers, are not affected.
- **The console database is copied with SQLite's `VACUUM INTO`**, which gives a consistent
  snapshot of a database in use.
- A pre-RFC 0016 SQLite ledger and a partitioned store have no such lock. They are copied as
  files, and the backup says so; run it when no pipeline is running.

### Restore

- The archive is unpacked into a staging directory in the home, then each project is moved into
  place. An entry that is not a plain file or directory, or whose path would leave the staging
  directory, fails the restore before anything is moved.
- **A project that already exists is not replaced** unless `--force`, and the check is made for
  every project before any is touched. With `--force` the existing project is moved to
  `trash/<tenant>/`, as `ekos project remove` does. Nothing is deleted.
- `control/` likewise: an existing console database needs `--force` and is moved to
  `trash/control-<time>/`.
- `--tenant` restores that tenant's projects from an archive and nothing else.
- Restore refuses an archive whose manifest names a newer format.

After restoring projects without `control/`, the console registers them at its next start (RFC
0179); their role bindings are not in the archive.

## 2. Model credentials per tenant

A project's `ekos.toml` names the environment variable holding a model key (`api-key-env`), never
the key. What changes is which variables a tenant's commands can see.

`Tenant` gains, set by a platform owner only:

- `model_env`: names of environment variables this tenant's commands receive;
- `allow_metered`: whether they receive them at all. Off for a new tenant.

Every `ekos` process the console starts for a workspace (pipeline stages, read commands, review
commands, the project's MCP server) gets an environment built for its tenant:

1. a fixed base: `PATH`, `HOME`, locale, time zone, temp directory, proxy and certificate
   variables, `OLLAMA_HOST`, and `EKOS_HOME`;
2. plus the variables named in the tenant's `model_env`, when `allow_metered` is on and the
   console itself has them.

Nothing else: not another tenant's variables, and none of the console's own (`EKOS_CONSOLE_*`).

With `allow_metered` off a tenant's projects can still use a local model server: `OLLAMA_HOST`
is in the base, and a local server needs no key. A hosted provider fails for want of its key.

### Compatibility

Outside production mode the tenant `default` keeps the whole environment minus
`EKOS_CONSOLE_*`, so an existing single-tenant install keeps working after an upgrade. In
production mode, and for every other tenant, the rule above applies.

### API

`PATCH /api/tenants/{t}` (platform owner) accepts `model_env` and `allow_metered`. A name must be
an environment variable name and may not start with `EKOS_CONSOLE_`. `GET /api/tenants` shows an
admin or owner the names and, for each, whether the console has it set. Values are never
returned.

## 3. Deployment guide

`docs/deployment.md`: install, first start, sign-in, owners, tenants and source roots, roles,
limits inside a project, agents over MCP, model credentials, backup and restore, upgrading, and
what is not built.

## Not built

- Scheduled backups, retention, upload to object storage. `ekos home backup` writes one file;
  a cron job or the host's backup system does the rest.
- Encryption of the archive. It holds the ledgers and the console database (token hashes, not
  tokens). Store it as you would the volume.
- Per-tenant spending limits or usage accounting.
- Withholding variables from a locally run `ekos`: the rule is the console's.
- Restoring into a home written by a newer EKOS than the one restoring.

## Verification

- Backup then restore into an empty home through the real binary: the restored project answers
  the same queries with the same object ids.
- A project being written is refused by name and no archive is left behind.
- Restore onto an existing project refuses and changes nothing; with `--force` the old project
  is in `trash/` and the restored one answers.
- A crafted archive with `..`, an absolute path, or a symlink is refused before anything moves.
- `--tenant` on both commands; a per-tenant archive holds no `control/`.
- The console database in an archive opens and holds the rows written before the backup.
- Environment: the table of who gets what, as unit tests; and through the real binary a
  variable named for one tenant is visible to its commands and not to another tenant's.

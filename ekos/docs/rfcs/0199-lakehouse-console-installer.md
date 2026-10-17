# RFC 0199 — EKOS Lakehouse: console tab, one-command install, user documentation

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-17
**Builds on:** RFC 0190–0196 (Lakehouse), RFC 0198 (console extension pages), RFC 0185 (full
package, `ekos setup`), RFC 0179 (installer)
**Code:** `ekos-lakehouse` (`lakehouse-ext`, `install.sh`, `scripts/`)

## Goal

Someone with Docker and a PostgreSQL database goes from nothing to a monitored Iceberg lakehouse
with one command, and runs it from the EKOS web console: health per table and layer, load modes,
open proposals, and the buttons to sync, check and doctor — without building anything.

## Design

### 1. The console tab (RFC 0198)

`LakehouseExtension::console_pages()` declares one page, **Lakehouse**:

| Panel | Command (class read) | View |
|---|---|---|
| Health | `lakehouse status --json` | table: table, bronze, silver, gold (badges), probable cause |
| Tables | `lakehouse tables --json` (new) | table: table, load mode, watermark, columns not landed exactly |
| Proposals | `lakehouse proposals --json` (new) | table: kind, table or mart, branch, what changes |
| Stack | `lakehouse tables --json` | fields: Trino, Polaris, storage, Dagster URLs, schedule |

Actions: `lakehouse doctor`, `lakehouse sync`, `lakehouse check`, `lakehouse plan`,
`lakehouse generate` (their own classes; the console asks for the typed confirmation an
external command needs). `tables` and `proposals` read the pipeline repository only (the lock
file and its git branches): no connection, no source data, class read.

### 2. One command to install

```bash
curl -fsSL https://raw.githubusercontent.com/alexeyban/ekos-lakehouse/main/install.sh | sh
```

`install.sh` downloads the release's **full package** for the platform — `ekos-lakehouse`, a
pinned Python and the EKOS console, assembled by EKOS's `package-full.sh` with the binary under
its own name — checks it against `SHA256SUMS`, unpacks it under `~/.local/share/ekos-lakehouse`,
links `ekos-lakehouse` into `~/.local/bin`, and runs `ekos-lakehouse lakehouse install`, which:

1. checks Docker and Compose v2, and that the stack's ports are free;
2. writes the stack (Compose file, Trino catalogs, Polaris bootstrap — compiled into the binary,
   so the binary and its stack never disagree) to `<home>/lakehouse/stack`, starts it, waits for
   Trino and Polaris, creates the catalog;
3. sets up the console in local mode for this user (`ekos-lakehouse setup --answers`), which
   starts it and prints a one-time sign-in link — the browser opens it unless `--no-open`.

`--source` (`host:port/database`, user and password variable) also creates the first project
and its Trino catalog; without it the console's Lakehouse tab says how to add one.
`lakehouse install --uninstall` stops and removes the stack (data volumes kept unless `--purge`).
The script is POSIX `sh`, never runs `sudo`, and changes nothing outside the user's home.

### 3. Documentation

In the repository: README (what it is, the one command, the console), `docs/install.md` (the
installer, offline install, what it changes, uninstall), `docs/console.md` (the tab, each panel
and action, roles), and the existing quickstart, runbook and troubleshooting updated to use the
release binary. In EKOS: the capabilities page and `docs/deployment.md` gain the extension-pages
mechanism and point at the Lakehouse docs.

## Releases

A Lakehouse release is a tag; `scripts/release.sh` builds the binary, assembles the full package
with EKOS's `package-full.sh` (EKOS checked out at the tag the Lakehouse `Cargo.lock` pins) and
uploads it with `SHA256SUMS`. Linux x86_64 first (the platform the stack is tested on); other
targets follow when they are tested.

## Out of scope

Windows (the stack needs Docker Desktop and has not been tested there), a hosted installer
service, upgrading a running stack in place (the docs give the steps).

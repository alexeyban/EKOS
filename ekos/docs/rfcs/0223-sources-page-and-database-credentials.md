# RFC 0223 — Sources in the console: databases, their passwords, a connection test

**Status:** Accepted
**Date:** 2026-10-25
**Plan:** Oracle/iDempiere plan v3, Part II.1 (step 1 of the operator workflow)
**Depends on:** RFC 0178 (`[[sources]]`), RFC 0180/0183/0184 (console roles, tenant environments, keychain),
RFC 0211 (live Oracle connector), RFC 0213 (step bar)

## 1. Problem

Step 1's exit: *a first-time operator goes from an empty project to a compiled ledger without a terminal; every
number on the page matches the census or a log line.* Today the console can create a project from one directory and
run the pipeline, but:

- a database can only be added by editing `ekos.toml` (`[[databases]]`), and nothing tests the connection;
- its password must reach `ekos build` as an environment variable, and the console passes a tenant's variables only
  **while the tenant may make metered model calls** (RFC 0183 §2) — a database password would vanish when a tenant
  turns metered calls off;
- nothing shows the database's own census next to what was compiled, so "every number matches the census" cannot be
  checked on the page.

## 2. Design

### 2.1 CLI — `ekos database`

| Command | Class | |
|---|---|---|
| `ekos database list [--json]` | read | the `[[databases]]` entries (never a password; whether its variable is set) |
| `ekos database add <name> --connect <host:port/service> --user <u> --password-env <VAR> --owner <O>… [--profile p0\|p1\|p2] [--dictionary idempiere]` | write | appends a `[[databases]]` entry through `toml_edit` (comments kept); the edited file must still load |
| `ekos database remove <name>` | admin | removes the entry; compiled objects stay (append-only) |
| `ekos database test <name> [--json]` | external | connects read-only, reads each owner's catalog (no profiling), reconciles it with `*_OBJECTS`, checks a write is refused, and — when the project has a ledger — puts the census next to what was compiled from this database |

`test` never prints the password or a row value; a missing variable is `MissingSecret` naming the variable.

Census vs compiled (by kind): table, view, function + procedure (routines), trigger. The ledger side counts objects
with `origin: live` and `database = <name>`; a mismatch is shown, not hidden.

### 2.2 Console — secrets for sources

- `Tenant.source_env`: variable names the tenant's projects use for **source credentials**. Unlike `model_env`
  (metered model keys, granted only with `allow_metered`), these are always in the tenant's process environment —
  a database password is not a metered call.
- Values live in the keychain (`env:<NAME>`, RFC 0184), never in `ekos.toml`, the console database or a log.
- Adding a database from the Sources page takes the password once: the console derives a variable name
  (`EKOS_DB_<TENANT>_<PROJECT>_<DATABASE>`, upper-cased, non-word characters → `_`), stores the value under it,
  grants it to the tenant, and runs `ekos database add … --password-env <that name>`. Replacing the password is the
  same call. Removing the database deletes the secret and the grant.
- Rights: listing and testing need `read`/`operate`; adding a source or database needs `operate` (RFC 0180 operator);
  removing needs `admin`.

### 2.3 Console — the Sources page

A project tab **Sources**: the named directory sources (`ekos source list --json`) and databases
(`ekos database list --json`); forms to add each; **Test connection** per database (census table, read-only check,
census vs compiled); **Analyse** is the existing *Init & compile*. A directory source must lie under one of the
tenant's allowed host directories (RFC 0180), the same check project creation uses.

A project the console creates from one directory has no named sources (it observes `[observe] paths` under its
root), and naming a source makes EKOS read named sources only. So the first directory added on the page names the
project's root first (after the directory's own name), then the new one; the CLI's `source add` is unchanged.

### 2.4 The process environment

The console's base environment (RFC 0183) gains `LD_LIBRARY_PATH`, `TNS_ADMIN` and `ORACLE_HOME`: the Oracle driver
loads Instant Client at run time, so without them a production console's `ekos build` cannot read a configured
database. They are configuration, not secrets. `Init & compile` ends with a `workflow status --json` stage, which
computes the review once and caches it (RFC 0213), so the step bar and Review page open fast after an analysis.

## 3. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | `ekos database list/add/remove/test` | on the testbed: `test` matches the census in `evals/oracle-idempiere/part0` |
| 2 | `source_env`, routes, Sources page | a browser run: empty project → git source → database with password → test → analyse → census vs compiled equal |

## 4. Tests

CLI: add/remove keep the rest of the file and refuse an invalid entry or a duplicate name; `list --json` never holds a
password; `test` without the variable names it. Console: the password reaches only the keychain (not the DB, not the
argv, not the log); `source_env` survives `allow_metered = false`; a directory outside the tenant's host directories
is refused; route rights.

## 5. Results

**Phase 1** (2026-10-25, iDempiere testbed): `ekos database test idempiere-db` — connected read-only through `DBA_*`
views, writes refused by the server, `ADEMPIERE`: 3,280 objects and 30,457 columns, census vs compiled tables 903 =
903, views 160 = 160, routines 62 = 62, triggers 0 = 0 (the Part 0 census). 19 s: 6 s connecting and reading the
catalog, 13 s reading the ledger for the comparison. Without the variable it names `EKOS_ORACLE_PASSWORD` and stops.

**Phase 2** (browser, `evals/oracle-idempiere/part2-sources/`): an empty project created from the iDempiere checkout,
the docs repository added (the root named first), the database added with its password, tested (read-only, nothing
compiled yet), analysed with Init & compile in the console (32 min 33 s, every stage succeeded), tested again: every
kind equal; step 1 `done` on the bar. 11 checks, 0 failed. The password was held only by the console's keychain (an
in-process one for the run) and the tenant's process environment.


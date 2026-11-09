# Devlog 330 — `ekos_sql_generate` takes the caller's tables; the Azure demo's lakehouse, timeouts and stale-source noise

**Date:** 2026-11-09
**PRs:** direct to main (v1.4.4); ekos-superset ekos-4.1.1 013f4be32e (image 4.1.1-ekos.10) and later; ekos-demo-public 2ffc20e…806ab0d
**Branch:** main

---

## Summary

A visitor asked the Azure demo "what does Joe Block buy the most?" and got *refused*: EKOS's SQL
agent saw all 1,233 compiled iDempiere tables and wrote SQL over `ad_user`, which Superset does not
let questions use. `ekos_sql_generate` now takes `tables` (RFC 0250 amendment 1) and the agent sees
only those. The same day the lakehouse (Polaris + Trino + a bronze job) was brought up on Azure —
six deployment faults, 856/900 tables loaded — and then switched off at the user's request; Superset
got a live step log, threaded workers, and a job that keeps EKOS's caches warm.

---

## RFC 0250 amendment 1 — `tables`

### Problem / motivation
Superset's guard rightly refused SQL over a table outside its allow-list; the fault was that the agent
never knew the list. A BI tool's allow-list is the normal case, not an edge case.

### What was built
| Component | Change |
|---|---|
| `sqlgen/src/schema.rs` | `Schema::retain_tables(names) -> (Schema, unknown)`: named tables (bare/qualified, any case) and only the FKs, declared joins, values and metrics among them |
| `cli/src/commands/sql_check.rs` | `allowed_tables(args)` (1–500 names); narrowing before the prompt; `tables_unknown` in the result; a list naming nothing compiled is refused before the cap or a provider |
| `cli/src/commands/mcp.rs` | `tables` in the tool's input schema; test with a recording provider |

### Decisions
Narrowing the schema (not adding "use only these tables" to the prompt) means the agent's own lint
flags any other table as unknown, so a wrong table is repaired inside the agent loop instead of
refused afterwards.

---

## RFC 0250 amendment 2 — the tool never returns a write (v1.4.5)

Found while recording the demo video: "Delete all invoices of Joe Block" came back as a `DELETE`
with status `open_findings`; Superset's guard refused it and nothing ran. `generate_core` now checks
the final SQL with `guard::single_select` (or, unparsed, by writing verbs outside literals) and
answers `refused` with no SQL. Superset no longer retries a refusal by drafting again unless the
reason is a table outside the allowed ones, and logs the real reason.

---

## Azure demo (ekos-demo-public)

### Lakehouse — six faults, then off
| Fault | Fix |
|---|---|
| Bronze job could not `git clone`: the demo repo is private | the job mounts the `sources` share, the runner comes in an env var |
| Trino crash-looped: `${ENV:…}` in an unquoted heredoc | quoted heredocs |
| HTTP ingress adds `X-Forwarded-Proto: https`: Trino 406, or (with `process-forwarded`) result URLs at `https://…:443` nothing serves | Trino behind a **TCP** ingress (`exposed_port = 80`) |
| `bi_reader`'s Azure password was never stored | separate read-only role `lake_reader` |
| Key Vault value set with `--file` keeps the trailing newline | role password set to the exact value |
| Polaris had no Azure identity (it writes metadata and signs storage access itself) | the apps' managed identity + `AZURE_CLIENT_ID` |
| Polaris OOM-killed at 1 GiB under 4 parallel commits | 1 vCPU / 2 GiB |

44 tables failed while Polaris restarted (503s; "failed to verify the token" — Polaris generates a new
token-signing key on every start). Then `enable_lakehouse = false`: apps and jobs destroyed, the
Iceberg files and Polaris's catalog in PostgreSQL kept.

### Superset
- **Live step log** (fork `live.py`, `StepLog`): `stream: true` answers NDJSON — each EKOS step as it
  starts and ends, then the result; the question runs in a thread with a copy of the request.
- **gthread workers**: a sync worker with `--timeout 120` was killed mid-question while EKOS rebuilt
  its caches after a console restart.
- **`ekosdemo-ekos-warm`** (every 10 min): the schema export and an `ekos_sql_check`, retried past
  the ingress's 240 s cut, so a visitor never pays a cold build.
- **`ekosdemo-restore-mtimes`** (manual): after an upload every source file has the upload's mtime
  and EKOS's freshness check calls the whole tree changed ("12601 file(s) changed" on every answer);
  the job sets each file's committed-manifest mtime back where the size matches (12,601 restored).

---

## Knowledge Captured

- **A text-to-SQL tool must check its own output is read-only.** The model will write a DELETE when
  asked to delete; lint findings are advice, not a gate.
- **An agent that writes SQL must be told the caller's allow-list.** A guard after the fact turns a
  good question into "refused".
- **Azure Container Apps HTTP ingress and Trino do not mix**: the forwarded proto is `https` even for
  an in-environment `http` call; use a TCP ingress for Trino (and anything that builds absolute
  URLs from the request).
- **Polaris on Azure**: needs an identity of its own (metadata writes + SAS signing), ≥2 GiB, and a
  fixed token key — otherwise every restart invalidates every client's token.
- **`az keyvault secret set --file`** keeps the file's trailing newline; `-o tsv` hides it on read.
- **gunicorn sync workers kill long requests** at `--timeout`; any page that waits on EKOS needs gthread.
- **Uploading sources resets mtimes**, and EKOS's freshness check is size + mtime: restore the
  manifest's times, or every read carries a stale-source note.
- **EKOS's schema caches are memory-only**: every console restart costs minutes of rebuilding on Azure
  Files. A warm job hides it; persisting the built schema in the project's state is the real fix (TODO).

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/sqlgen/src/schema.rs` | `retain_tables` + test |
| `ekos/crates/cli/src/commands/sql_check.rs` | `tables` argument, `tables_unknown` |
| `ekos/crates/cli/src/commands/mcp.rs` | tool schema; recording-provider test |
| `ekos/docs/rfcs/0250-mcp-sql-generate.md` | amendment 1 |
| `CHANGELOG.md`, `ekos/Cargo.toml`, lockfiles | v1.4.4 |

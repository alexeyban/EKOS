# Devlog 275 — EKOS Lakehouse v1 built (RFC 0192–0196) on the extension seam

**Date:** 2026-10-17
**PRs:** none (local main: a2b52d1, 58d0f3a, 584a130, 0575255 and this one)
**Branch:** main

---

## Summary

All six milestones of RFC 0190 are built in the separate `ekos-lakehouse` repository (its
devlogs 1–7), on the RFC 0191 extension seam from devlog_274. What lives here: the design of each
milestone (RFC 0192 bronze, 0193 silver, 0194 schema changes, 0195 gold, 0196 monitoring), one
small core addition (`migrate-logic::check_candidate_with`), and this record of what the work
taught about EKOS itself.

---

## What changed in EKOS

| Change | Why |
|---|---|
| RFC 0192–0196 | the Lakehouse milestones, each with a "found while building it" section |
| `migrate-logic::invention::check_candidate_with(source, sql, dialect)` | the anti-invention check under another grammar (Trino); `check_candidate` delegates (ClickHouse), unchanged |
| capabilities page | the extension paragraph now describes Lakehouse as built |

## What Lakehouse proved about the core

- **The seam is enough for a product**: commands, catalogue classes, settings tables, scoped MCP
  tools and post-commit facts carried five milestones without another core change.
- **EKOS's own dbt analyzer compiles generated projects back**: 1 193/1 193 LedgerSMB silver
  columns traced to bronze through `column_lineage`.
- **RFC 0164's anti-invention check works on a model's SQL generally**, not only on lowered
  views — with one lesson: a draft that invents nothing can still not run (`COUNT("ar".*)`), so
  the engine plans every draft too.
- **The fact ledger's single writer** is felt by an orchestrator running tables in parallel:
  Lakehouse waits for the lock (backoff, 2 min).
- **Append-only + fixed ids** gives "current state" facts for free (schema state per table).

## Knowledge Captured

Platform findings, all recorded in the RFCs' "found" sections and Lakehouse's troubleshooting
page: Trino's PostgreSQL connector silently drops unmapped columns by default and misreads
`'infinity'` timestamps; Polaris refuses Trino's purge unless the catalog allows it; Iceberg REST
`CREATE SCHEMA IF NOT EXISTS` races; no-op Iceberg DDL writes no metadata; `dagster-dbt` targets
a fresh directory per invocation; MinIO's community edition is unmaintained (SeaweedFS instead).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0192…0196-*.md` | new |
| `ekos/crates/migrate-logic/src/invention.rs` | `check_candidate_with` + test |
| `TODO.md`, `docs/generated/ekos-self-documentation.html` | Lakehouse milestones; capabilities |

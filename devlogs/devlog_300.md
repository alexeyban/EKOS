# Devlog 300 — Oracle as a lakehouse source: the source-engine seam (RFC 0216)

**Date:** 2026-10-25
**PRs:** none (direct to `main`); ekos-lakehouse devlog_14
**Branch:** `main`

---

## Summary

Steps 5–7 of the operator workflow can now read Oracle. EKOS Lakehouse had PostgreSQL wired into its catalog reads,
type map, watermark rules and reconciliation; RFC 0216 puts a `Source` trait in front of them (the PostgreSQL pipeline
stays byte-identical) and adds an Oracle source that reuses the project's `[[databases]]` entry. In EKOS,
`migrate-validate` gains `Dialect::Oracle` and a column-digest row hash, because Oracle cannot build a row string past
4000 bytes. On the iDempiere testbed: 901 of 903 tables planned, 3,616 pipeline files generated, 12 business and
dictionary tables loaded through Trino and reconciled green at V1–V3 with the planted control. The first Oracle runs
found two defects that applied beyond Oracle: a reconciliation could agree over rounded decimals, and a wide row's
SQL overflowed the stack of the Trino read guard.

---

## RFC 0216 — source-engine seam + Oracle source

### Problem / motivation
Steps 1–3 already ran on Oracle (RFC 0211, 0213, 0223); the lakehouse could not follow. Four places assumed
PostgreSQL: `source.rs` (`pg_class`, `pg_trigger`, …), `typemap.rs`/`mode.rs`, `connect_source` returning a
`PgSource`, and `reconcile.rs` rendering the source side in `Dialect::Postgres`.

### What was built

| Component | Repo | Change |
|---|---|---|
| `migrate-validate/src/dialect.rs` | EKOS | `Dialect::Oracle`: canonical forms in Oracle functions, `STANDARD_HASH` (AL32UTF8 required), hex `TO_NUMBER` prefix, `MOD`, subquery grouping, `LENGTHB`; `column_digest_expr`; Trino's row string as a flat, chunked `concat` |
| `migrate-validate/src/tiers.rs`, `bisect.rs` | EKOS | `UnitPlan::digest_columns`; V3 and bisect hash `source_row`/`target_row` |
| `tests/snapshots/oracle.sql.txt` | EKOS | the Oracle SQL pinned; the Trino snapshot updated for `concat` |
| `migrate-logic/src/v5.rs` | EKOS | Oracle arms (never run: Oracle is not a V5 engine) |
| `bronze`: `SourceEngine`, `typemap_oracle.rs`, `mode::choose_for` | lakehouse | the engine in `PlanSettings` (written only when not PostgreSQL); the Oracle type map; Oracle watermark types |
| `lakehouse-ext`: `source::Source`, `source_oracle.rs`, settings, doctor | lakehouse | the trait; PostgreSQL moved behind it; the Oracle catalog reader and `EngineReader`; `source-engine = "oracle"` + `source-database` naming a `[[databases]]` entry |
| `deploy/trino/catalog/idempiere.properties` | lakehouse | the testbed as a Trino `oracle` catalog |
| `tests/oracle_live.rs` | lakehouse | canonical forms/hashes/buckets on Oracle; plan → load → reconcile for 12 tables |

### Implementation details worth remembering
- An Oracle source keeps Oracle's names (`ADEMPIERE.C_ORDER`). Reconciliation quotes them for Oracle; Trino folds every
  identifier, quoted or not, so the generated SQL needs no change. Only the connector's column list is matched
  case-insensitively.
- An unconstrained `NUMBER` is measured per table in one scan. A column with no values gets precision 0 and lands at
  `decimal(38,10)`; that is safe only because the Oracle decimal rendering never rounds (below).
- `writes_refused` probes with a zero-row `INSERT … SELECT … WHERE 1 = 0` on a table of another schema, always rolled
  back — Oracle DDL would commit.

### Decisions
- **Reuse `[[databases]]`**, not new `source-*` keys: one connection definition for steps 1–7, and the console's Sources
  page already stores its password. PostgreSQL keys are refused with `oracle`.
- **Column digests on both sides**, not a CLOB row string: `STANDARD_HASH` rejects LOBs and `DBMS_CRYPTO` needs a grant
  a read-only user rarely has. V1/V2 stay on canonical text.
- **Empty `NUMBER` columns land instead of refusing their table**: the first plan refused 176 tables, 30 of them with
  rows (`M_INOUT`, `M_COST`, …) because one amount column was all NULL.

---

## Knowledge Captured

- **Trino rounds a decimal cast silently** (`CAST(number '1.005' AS decimal(38,2))` = `1.01`), and the source-side
  canonical `TO_CHAR(c, 'FM…0.00')` rounds the same way — so a lossy load reconciled green. Oracle's decimal rule now
  renders a value with more digits than the scale unrounded. PostgreSQL's `to_char` rule has the same blind spot and
  was left alone here (its pinned SQL and the Pagila gate); worth the same guard.
- **The Trino read guard re-prints every query** (`check_read_only` → RFC 0160 classifier → `Statement::to_string`),
  which recurses once per `||`. A 92-column row overflowed a 2 MiB test thread in a debug build. Flat
  `concat(…)` calls (≤ 64 arguments, nested) give the same string with constant depth.
- **`cargo clippy` does not pass a top-level `--config` on to its inner `cargo check`**, and `cargo fmt` runs
  `cargo metadata` without it: with a local `[patch]` in a `--config` file, put it after the subcommand
  (`cargo clippy --config patch.toml`), and expect `Cargo.lock` to flip back to git after `cargo fmt`.
- **Trino's Oracle connector** reads an unconstrained `NUMBER` as its own `number` type (castable to `decimal`), a `DATE`
  as `timestamp(0)`, `CHAR(1)` flags as `char(1)`. The Oracle container must share the lakehouse network: its port is
  bound to 127.0.0.1, which `host.docker.internal` cannot reach.
- iDempiere stores currency conversion rates (`MULTIPLYRATE`, `DIVIDERATE`) with more than 38 digits — division results.
  No Iceberg decimal holds them; they are refused with that reason.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0216-lakehouse-source-engine-oracle.md` | new RFC with results |
| `ekos/crates/migrate-validate/src/dialect.rs` | `Dialect::Oracle`, `column_digest_expr`, Trino flat `concat`, tests |
| `ekos/crates/migrate-validate/src/tiers.rs`, `bisect.rs` | `digest_columns`, `source_row`/`target_row` |
| `ekos/crates/migrate-validate/tests/sql_snapshot.rs`, `snapshots/{oracle,trino}.sql.txt` | Oracle pinned, Trino updated |
| `ekos/crates/migrate-logic/src/v5.rs` | Oracle arms |
| `ekos/crates/cli/src/commands/migrate.rs`, `pg-live/tests/live_catalog.rs`, `migrate-validate/tests/live_tiers.rs` | `digest_columns: false` |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0216 |

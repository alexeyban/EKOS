# RFC 0216 — EKOS Lakehouse: a source-engine seam, and Oracle as a source

**Status:** Accepted
**Date:** 2026-10-25
**Plan:** Oracle/iDempiere plan v3, Part II.8 (internals steps 5–7 need), Part VI.1–VI.2
**Repos:** EKOS (`migrate-validate`), ekos-lakehouse (`bronze`, `lakehouse-ext`)
**Depends on:** RFC 0155/0156 (canonical form, tiers), RFC 0190/0192 (lakehouse, bronze), RFC 0211 (live Oracle
connector, `[[databases]]`)

## 1. Problem

EKOS Lakehouse reads one engine: PostgreSQL. Steps 1–3 of the operator workflow now run on Oracle (iDempiere), but
steps 5–7 — plan, bronze, reconciliation — cannot, because PostgreSQL is wired into four places:

| Place | PostgreSQL-specific |
|---|---|
| `lakehouse-ext/src/source.rs` | catalog reads (`pg_class`, `pg_attribute`, `pg_trigger`, …), numeric and infinity measurements |
| `bronze/src/typemap.rs`, `mode.rs` | the type map (PostgreSQL type first, connector type second); which types can order a watermark |
| `lakehouse-ext/src/plan_cmd.rs` | `connect_source` returns a `PgSource` |
| `lakehouse-ext/src/reconcile.rs` | the source side's canonical SQL is `Dialect::Postgres` |

## 2. Design

### 2.1 The seam

`bronze::SourceEngine` (`postgres` | `oracle`) is part of `PlanSettings`, serialized only when not `postgres`, so a
PostgreSQL lock file is byte-identical to today's. `bronze::plan` maps columns with the engine's type map and orders
watermarks by the engine's types.

In `lakehouse-ext`, `source::Source` is the connected engine:

```rust
pub trait Source {
    fn engine(&self) -> SourceEngine;
    /// Base tables of `schemas` with columns, keys, foreign keys, update triggers, comments and the
    /// connector's column types; measured where the type map needs a measurement.
    fn read_tables(&self, trino: &TrinoReader, catalog: &str, schemas: &[String],
                   redaction: &RedactionConfig, max_measured_rows: i64) -> anyhow::Result<Vec<SourceTable>>;
    /// The source side of a reconciliation (RFC 0156): read-only, in the engine's dialect.
    fn reader(&self) -> &dyn EngineReader;
}
```

`plan_cmd::connect_source(settings, purpose) -> Box<dyn Source>` picks the engine from settings. PostgreSQL's
implementation is today's code, moved. The refactor's gate: the Pagila pipeline generated before and after it is
byte-identical, and the PostgreSQL live suites stay green.

### 2.2 Settings

```toml
[extensions.lakehouse]
source-engine   = "oracle"          # default "postgres"
source-database = "idempiere-db"    # for oracle: the name of a [[databases]] entry
source-catalog  = "idempiere"       # the Trino catalog over the same database
schemas         = ["ADEMPIERE"]     # default: the entry's owners
```

For Oracle, the connection (Easy Connect string, user, password variable) comes from the project's `[[databases]]`
entry (RFC 0211, RFC 0223): one definition serves steps 1–3 and 5–7, and the console's Sources page already manages
its password. `source`, `source-user`, `source-password-env` are PostgreSQL's and are refused with `oracle`.

### 2.3 Oracle catalog

Read through `ekos-ora-live`'s guarded session (read-only transaction, pinned NLS, `TIME_ZONE = UTC`, `DBA_*` or
`ALL_*`): tables (`*_TABLES`, not temporary, not nested, not in the recycle bin), columns (`*_TAB_COLUMNS`, type as
`render_type` writes it), primary keys and foreign keys (`*_CONSTRAINTS` / `*_CONS_COLUMNS`), comments (redacted),
`NUM_ROWS` as the row estimate, and `BEFORE EACH ROW` triggers on `UPDATE` with their body (`TRIGGER_BODY`), whose
`:NEW.<col> :=` assignments prove a watermark exactly as a PostgreSQL trigger does. Names keep Oracle's case
(`ADEMPIERE.C_ORDER`): the direct reads in reconciliation need it, and Trino folds every identifier, quoted or not,
so the generated SQL works unchanged. The connector's column list is matched case-insensitively.

Unconstrained `NUMBER` columns are measured — integer digits and scale used — in **one scan per table** over all such
columns, only for tables at or under `full-refresh-max-rows`. A column with no values to measure (an empty table, or
all `NULL`) is recorded as such (precision 0).

### 2.4 Oracle → Iceberg type map (plan VI.1)

Decided from the Oracle type first, the connector's type second:

| Oracle | Iceberg | Lossiness | Compared as |
|---|---|---|---|
| `NUMBER(p,0)` p ≤ 9 / p ≤ 18 | `integer` / `bigint` | Exact | Int |
| `NUMBER(p,0)` p > 18, `INTEGER` | `decimal(p,0)` / `decimal(38,0)` | Exact | Decimal(0) |
| `NUMBER(p,s)`, 0 < s ≤ p | `decimal(p,s)` | Exact | Decimal(s) |
| `NUMBER` unconstrained, measured p ≤ 38 | `decimal(38,s)` | NarrowingSafe | Decimal(s) |
| `NUMBER` unconstrained, no values to measure | `decimal(38,10)` | NarrowingSafe (vacuously) | Decimal(10) |
| `NUMBER` unconstrained, not measured, or measured p > 38; negative scale | — | Refused | |
| `FLOAT`, `BINARY_FLOAT`, `BINARY_DOUBLE` | `double` / `real` | Exact | not compared (named) |
| `VARCHAR2`, `NVARCHAR2` | `varchar` | Exact | Text |
| `CHAR`, `NCHAR` | `varchar` | AsText (blank-padded) | Text |
| `CLOB`, `NCLOB` | `varchar` | Exact | not compared (a LOB cannot be hashed in SQL without `DBMS_CRYPTO`) |
| `DATE` | `timestamp(6)` | Widening (a `DATE` has a time) | TimestampNaive |
| `TIMESTAMP(p)` p ≤ 6 | `timestamp(6)` | Exact (p = 6) / Widening | TimestampNaive |
| `TIMESTAMP(p)` p > 6 | — | Refused (sub-microsecond digits would be cut) | |
| `TIMESTAMP(p) WITH [LOCAL] TIME ZONE` | `timestamp(6) with time zone` | Exact | TimestampUtc |
| `RAW` | `varbinary` | Exact | Bytes |
| `BLOB` | `varbinary` | Exact | not compared |
| `ROWID`, `UROWID`, `INTERVAL …`, `XMLTYPE`, others the connector gives as `varchar` | `varchar` | AsText | Text |
| `LONG`, `LONG RAW`, object types, anything the connector does not expose | — | Refused, naming the column | |

A refused column refuses its table, with the reason, as for PostgreSQL. `''` is `NULL` in Oracle; bronze keeps the
`NULL` (silver's Y/N → boolean and the empty-string rule are step 6's).

### 2.5 Reconciliation: `Dialect::Oracle` (EKOS `migrate-validate`)

The canonical form (RFC 0155) rendered in Oracle SQL, run in the session's pinned NLS settings:

| Rule | Oracle expression |
|---|---|
| Text, Char, Inet | `REPLACE(REPLACE(TO_CHAR(c), '\', '\\'), CHR(31), '\x1f')` (`TO_CHAR` brings `NVARCHAR2` into the database character set) |
| Int | `TO_CHAR(c)` |
| Decimal(s) | `TO_CHAR(c, 'FM9…90.0…0')` (s zeros; no point at s = 0) — but `TO_CHAR(c)`, unrounded, when `c <> ROUND(c, s)` |
| TimestampNaive | `TO_CHAR(CAST(c AS TIMESTAMP(6)), 'YYYY-MM-DD"T"HH24:MI:SS.FF6')` — a `DATE` too |
| TimestampUtc | the same over `SYS_EXTRACT_UTC(c)` |
| Date | `TO_CHAR(c, 'YYYY-MM-DD')` |
| Bytes | `LOWER(RAWTOHEX(c))` |

NULL is decided on the column, as in every dialect. The decimal rule never rounds: Trino's `CAST` to
`decimal(38,s)` rounds a value with more digits silently (measured: `1.005` → `1.01`), and a `TO_CHAR` with `s`
digits would round it the same way, so a lossy load would reconcile green. A value the scale cannot hold renders
unrounded and cannot match. This is what makes the "no values" landing above safe: a later value with more digits
fails the next reconciliation. Hash: `LOWER(RAWTOHEX(STANDARD_HASH(x, 'SHA256')))`, which hashes
the text's bytes in the database character set — so reconciliation requires `NLS_CHARACTERSET = AL32UTF8` and the
Oracle reader checks it at connect, refusing otherwise with the reason (plan trap: non-AL32UTF8 charsets). 60-bit
prefix: `TO_NUMBER(SUBSTR(h, 1, 15), 'XXXXXXXXXXXXXXX')` (big-endian, exact); modulo `MOD(…)`; sums are `NUMBER`,
exact to 38 digits. Oracle cannot `GROUP BY` an alias or a position, so bucket queries group a subquery.

**The 4000-byte limit.** A row's canonical string is its columns joined, and an Oracle `VARCHAR2` expression stops at
4000 bytes (`ORA-01489`) — iDempiere tables have over a hundred columns. `UnitPlan::digest_columns` makes V3 and
bisect hash each row as the SHA-256 of its **column digests** (the first 16 hex digits of each column's canonical
hash, joined by the unit separator) instead of its column texts. It is set on both sides of a unit, so both compute
the same thing; 17 bytes per column keeps a row of up to 235 columns under the limit, and a wider table is reported,
not hashed. V1 and V2 are unchanged (V2 compares each column's canonical form, which a single `VARCHAR2(4000)` column
with backslashes could still overflow — reported as a read error naming the table, never a green).

**Trino's row string is a flat `concat`.** The Trino read path parses every query and prints it back (RFC 0160's
classifier), and printing a left-deep chain of `||` recurses once per operator: a 92-column row overflowed a test
thread's stack. `row_hash_expr` for Trino builds `concat(…)` calls of at most 64 arguments, nested — the same string,
a nesting depth of a few levels whatever the width. PostgreSQL, ClickHouse and Oracle keep `||` (their reads are not
parsed on our side).

### 2.6 What is not in this RFC

- Incremental loads for tables whose watermark the **application** maintains (iDempiere's `Updated`, set by `PO`
  — RFC 0212's write-behaviour facts): plan D4, step 5 (RFC 0212 lakehouse part). Here an Oracle table is loaded in full
  unless a trigger proves its watermark.
- The source position recorded per load (`_ekos_source_lsn`) stays empty on Oracle (the orchestrator's PostgreSQL
  query fails and records "unknown", as on a replica); the SCN belongs with CDC (RFC 0217).
- `lakehouse install --source` writing an Oracle catalog; the console's Extraction page (step 5).

## 3. Phases

| Phase | Repo | Deliverable | Exit |
|---|---|---|---|
| 1 | lakehouse | `SourceEngine` in `PlanSettings`, `Source` trait, PostgreSQL moved behind it | Pagila pipeline byte-identical; unit + PostgreSQL live suites green |
| 2 | EKOS | `Dialect::Oracle`, `UnitPlan::digest_columns` | pinned SQL; live (in the lakehouse repository, which holds an Oracle connection): Oracle's canonical forms, hashes, prefixes and buckets equal the Rust ones on the testbed |
| 3 | lakehouse | Oracle `Source`, type map, settings, doctor | live on the iDempiere testbed: `lakehouse plan` over `ADEMPIERE`, generated loads run through Trino for a set of business tables, reconciliation V1–V3 green with the planted control firing |

## 4. Tests

Lock-file byte identity for a PostgreSQL plan (no `source_engine` key). The Oracle type map, one test per row of §2.4,
including the refusals. Oracle trigger bodies: `:NEW.updated := SYSDATE` is an assignment, `IF :NEW.a = :OLD.a` is not.
`Dialect::Oracle` SQL snapshot (`tests/snapshots/oracle.sql.txt`); a 300-column Trino row stays shallow. Live
(`EKOS_LAKEHOUSE_LIVE=1 EKOS_ORACLE_LIVE=1`, lakehouse `tests/oracle_live.rs`): every shared fixture case rendered on
Oracle equals `canon.rs` (or is explained: `''` and an empty `RAW` are `NULL` in Oracle, there are no arrays), hashes,
60-bit prefixes and bucket checksums equal the Rust ones, a `DATE` keeps its time, `1.005` at scale 2 is not rounded;
then plan → load → reconcile on the testbed with the control.

## 5. Results

iDempiere testbed (Oracle XE 21c, `ADEMPIERE`: 903 tables, 18,520 columns), Trino 483 with the `oracle` connector,
2026-10-25.

**Phase 1 — the seam.** The Pagila pipeline generated before and after the refactor is byte-identical (84 files,
`diff -r`). Every lakehouse suite, the live ones included (Pagila bronze, the RFC 0194 change classes on `m3`, Trino's
canonical forms), passes: 152 tests. LedgerSMB could not be run: its database is no longer in the sandbox.

**Phase 2 — the dialect.** Oracle reproduces all 29 applicable fixture cases, the hashes and prefixes of three texts
(one with a unit separator and an emoji) and a 200-row, 16-bucket checksum exactly as the Rust implementation does.

**Phase 3 — the source.**

| | |
|---|---|
| `lakehouse plan` (release, CLI) | 903 tables read in 15 s: **901 planned, 2 refused** |
| refused | `C_CONVERSION_RATE`, `I_CONVERSION_RATE`: `MULTIPLYRATE`/`DIVIDERATE` hold more than 38 digits (stored division results) |
| `lakehouse generate` | 3,616 files in 5 s |
| `lakehouse doctor` | source reachable; the connector exposes all 18,520 columns; the server refuses `ekos_ro`'s writes |
| loaded and reconciled (live test) | 12 tables — `AD_CLIENT`, `AD_ORG`, `C_BPARTNER`, `M_PRODUCT`, `C_ORDER`, `C_ORDERLINE`, `C_INVOICE`, `C_INVOICELINE`, `C_PAYMENT` (102 columns), `M_INOUT`, `AD_COLUMN` (26,006 rows, 10 s), `A_ASSET_ADDITION` (empty): **all green at V1–V3, the control fired on every table with rows**, no column left uncompared |
| `lakehouse reconcile` (release, CLI) | `C_ORDER` and `C_PAYMENT` from the generated `load.sql`: green, control fired, 1.7 s |

Before the "no values" rule, 176 tables were refused: 146 empty, and 30 with rows (`M_INOUT`, `M_COST`, `PP_ORDER`, …)
whose amounts in some column were all `NULL` — refusing a table for a column with nothing in it hid its data. The
first Oracle plan also surfaced two defects fixed here: the rounding blind spot (§2.5) and the stack overflow on wide
rows (§2.5).

**Not yet:** a Dagster run of the whole `ADEMPIERE` pipeline, silver models over Oracle names (step 6), and incremental
loads from application-maintained watermarks (step 5, plan D4).

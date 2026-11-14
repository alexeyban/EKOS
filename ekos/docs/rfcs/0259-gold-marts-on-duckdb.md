# RFC 0259 — Gold marts on DuckDB

**Status:** accepted (2026-11-14) · **Target:** EKOS 1.6.0, ekos-lakehouse 0.2.0, ekos-superset ekos.20
**Depends on:** RFC 0193 (silver), RFC 0195 (gold drafting), RFC 0242/0243 (sql check, probes), RFC 0250
(`ekos_sql_generate`), RFC 0255 (registered dialects, conformance kit), RFC 0257/0258 (marts, dashboards)
**Plan:** `lakehouse-duckdb-demo-plan.md` (2026-11-12), items C1, C2, C5, L1–L7, S1–S5

## 1. Problem

The lakehouse builds every layer in Trino. Gold marts are small, read often and read by one BI tool; a
server is the wrong shape for them. DuckDB serves a mart from one file with no server, and reads the same
Iceberg silver through the same Polaris catalog, so EKOS can show itself as the control plane over two
engines instead of one. Today nothing in EKOS knows DuckDB: there is no dialect plugin, no probe, no
reconcile canon; the lakehouse `gold` crate parses with the Trino dialect and generates dbt-trino only; the
Superset fork's guard has no DuckDB rules — and DuckDB, unlike PostgreSQL, reads files and the network from
a plain `SELECT`.

## 2. Spike result (M0, 2026-11-14) — the design rests on it

A scratch stack (SeaweedFS 4.48, Polaris 1.8.0, Trino 483; DuckDB CLI 1.5.5 with `iceberg` + `httpfs`):

- Trino wrote `silver.c_invoice` (format v2; bigint, varchar, date, decimal(18,2), boolean, timestamp(6),
  timestamp(6) with time zone), 10 000 rows, then `DELETE` 3 333 and `UPDATE` 667 — 2 data files and 2
  position-delete files.
- DuckDB attached the catalog (`ATTACH 'lakehouse' AS lh (TYPE iceberg, ENDPOINT …/api/catalog, SECRET
  <iceberg oauth2 secret>, ACCESS_DELEGATION_MODE 'none')` + an S3 secret with path style, no SSL) and
  returned exactly Trino's answer: 6 667 rows, `sum(grandtotal)` 41 675 667.00, 3 333 `CO`, same min
  timestamp and max date. Delete files are applied; all eight types map.
- `CREATE TABLE marts.gold.… AS SELECT … FROM lh.silver.…` into `marts.next.duckdb` reconciled with silver
  (3 333 / 20 838 167.00) and published by `rename` (536 KB).
- Opened with `-readonly` and `SET enable_external_access = false; SET lock_configuration = true;`:
  the mart query answers; `read_csv('/etc/passwd')`, re-enabling access, `INSTALL`, `CREATE` and `ATTACH`
  are all refused by DuckDB itself.

Decision 1 of the plan is therefore taken: **dbt-duckdb reads silver from Iceberg and writes DuckDB
tables**; the "Trino builds gold, DuckDB reads it" fallback is not needed.

## 3. Design

### 3.1 EKOS core (1.6.0)

| Part | What | Where |
|---|---|---|
| D1 | `sql-dialect-duckdb` plugin: `sqlparser`'s `DuckDbDialect`, a line-preserving `preprocess` for what sqlparser 0.53 cannot read (to be measured on dbt-duckdb output and DuckDB's own docs examples, each rewrite listed in the crate docs), `facts()` and `generation_notes()` (the `every_registered_dialect_has_generation_notes` test). Registered as `"duckdb"`; `init --detect` maps a `duckdb/` directory | `plugins/sql-dialect-duckdb`, `recovery/src/sql_dialect_registry.rs`, `cli/src/detect.rs` |
| D2 | dbt analyzer: a project whose `profiles.yml`/`dbt_project.yml` names `type: duckdb` (or whose models fail Trino and parse as DuckDB) uses D1; models keep `ref()`/`source()` lineage to silver so `ekos_ossie_export` and the column chain reach the source column | `recovery/src/dbt_analyzer.rs` |
| D3 | `CanonDialect` for DuckDB — canonical row serialization and the bucket checksum, so reconcile and V1–V3 can compare a DuckDB mart with Trino silver. Implemented in tree as a `static DUCKDB` passed as `Dialect::Registered(&DUCKDB)` (no enum change), and passing RFC 0255's conformance kit against a real DuckDB | `migrate-validate/src/duckdb.rs` |
| D4 | Probe: `sql check --duckdb <file>` / `Probe` impl opening the file **read-only** with `enable_external_access = false` and `lock_configuration = true` set before the first statement, plus the shared single-`SELECT` guard; a deadline by `interrupt` from a timer thread (DuckDB has no statement timeout) | `plugins/duckdb` (new, feature `duckdb`, bundled build) |
| D5 | `[[databases]] kind = "duckdb"` (`path`, no password): catalog (tables, views, columns, types; reconciled against `duckdb_tables()`/`duckdb_views()` counts), profile tiers P0–P2 with the same no-value rule, so `ekos_sql_generate` can probe | `plugins/duckdb`, `observation_sdk::live_catalog` |

The `duckdb` Rust crate bundles the C++ engine (long build); it is behind the `duckdb` cargo feature like
`oracle`, on in release builds and in CI's feature job, off in the default `cargo test`.

### 3.2 ekos-lakehouse (0.2.0)

| Part | What |
|---|---|
| L1 | `[gold] engine = "trino" \| "duckdb"` (default `trino`, so existing plans are unchanged). `gold` takes the dialect from the engine: drafting check (`check_draft`), invention check (`check`), lowering and the type map Iceberg/Trino → DuckDB |
| L2 | dbt-duckdb project generation: `profiles.yml` with the `iceberg` + `httpfs` extensions, the S3 and Iceberg secrets **by environment variable name**, `ATTACH` of the Polaris catalog as an `on-run-start` hook; silver as dbt sources from the attached catalog; marts as tables in `marts.next.duckdb` |
| L3 | Publish: build into `marts.next.duckdb`, run tests, reconcile (L5), then `rename` over `marts.duckdb`; a failed test or reconcile leaves the published file untouched and says why |
| L5 | Reconcile across engines: each measure's total in the DuckDB mart against silver in Trino, through D3 |
| L6 | Dagster: a `gold_duckdb` asset group after silver; health and freshness (RFC 0258) for DuckDB marts |
| L7 | `tools/lakehouse-docs` lineage: parse gold SQL with sqlglot's `duckdb` dialect |

### 3.3 ekos-superset

| Part | What |
|---|---|
| S1 | `duckdb` + `duckdb-engine` pinned to the DuckDB version the marts are written with (the file format must be readable); connection `duckdb:////marts/marts.duckdb` with `connect_args` `read_only = true` and `config = {enable_external_access: false, lock_configuration: true}` — DuckDB enforces the lock, the guard is the second barrier |
| S2 | `EKOS_AI_DIALECT = "duckdb"` through guard, planner and the `ekos_sql_check` call |
| S3 | Guard rules for DuckDB: refuse table functions and statements that reach files, network or settings — `read_csv*`, `read_parquet`, `read_json*`, `read_text`, `read_blob`, `glob`, `iceberg_scan`, `delta_scan`, `parquet_*`, `sniff_csv`, `ATTACH`, `DETACH`, `COPY`, `EXPORT`, `IMPORT`, `INSTALL`, `LOAD`, `PRAGMA`, `SET`, `RESET`, `CALL`, `getenv`, and a string literal in `FROM` |
| S4 | Query deadline: `interrupt()` after the configured seconds (PostgreSQL's `statement_timeout` does not exist here) |
| S5 | "From EKOS" shows mart → silver → source column (needs D2) |

## 4. Not changed

Bronze and silver stay on Trino. No DuckDB write path anywhere in EKOS: EKOS reads a DuckDB file only
read-only (D4/D5). The marts file is written only by dbt-duckdb in the lakehouse.

## 5. Tests

- D1: round trip of every dbt-duckdb model the lakehouse generates; DuckDB-only syntax (`GROUP BY ALL`,
  `EXCLUDE`, `QUALIFY`, list literals, `::` casts) parses; the registry and generation-notes tests.
- D3: RFC 0255's conformance kit against an in-process DuckDB; a planted one-cent difference is found.
- D4: a probe refuses `read_csv`, `ATTACH`, `SET`, `COPY`; a long query is interrupted at the deadline; the
  database file is byte-identical after a probe run.
- L1–L5: a plan with `engine = "duckdb"` generates a dbt-duckdb project that builds, reconciles and
  publishes against the spike stack (live suite, env-gated); a failing reconcile leaves `marts.duckdb` as it was.
- S3: one test per refused function/statement; the guard refuses `FROM '/etc/passwd'`.

## 6. Open

- Whether D5's catalog read belongs in `plugins/duckdb` or a `duck-live` crate beside `pg-live`/`ora-live`
  (decide when D4 exists; the probe and the catalog share the read-only session).
- DuckDB version pinning across three images (Dagster, Superset, the EKOS release): one constant, checked
  by a test that reads all three.

## Implementation log

- 2026-11-14 — M0 spike (§2) passed.
- 2026-11-14 — **D1–D4 built** (EKOS core, branch for 1.6.0):
  - **D1** `plugins/sql-dialect-duckdb`, registered as `duckdb`; `init --detect` maps a `duckdb/`
    directory. Measured on 116 DuckDB statements (dbt-duckdb materializations, DuckDB docs
    examples): `DuckDbDialect` reads 89 unaided. Three constructs that matter for recovery did not
    parse and `preprocess` blanks them, line-preserving: sample clauses (`USING SAMPLE …`,
    `TABLESAMPLE 10%`, `reservoir(…)`, `REPEATABLE (…)`), the CTE hint `AS [NOT] MATERIALIZED`,
    and `ATTACH`'s option list (`TYPE iceberg, ENDPOINT …`) — 98 parse after. Still unread, and
    listed in the crate docs: `FROM`-first queries, the `PIVOT`/`UNPIVOT`/`SUMMARIZE` statements,
    `#1`, list comprehensions, struct-literal field access, `GLOB`, `LIMIT n%`, `**`, `E'…'`,
    `lambda x:`, `SET GLOBAL`, `CREATE TYPE … ENUM`, `EXPORT DATABASE`, `CHECKPOINT`. `facts()`
    measured on the DuckDB 1.5.5 CLI: `7 / 2 = 3.5`, NULLs last, `ILIKE`, `LIMIT` and `FETCH FIRST`,
    no backticks. The clause tokenizer moved from the Trino crate into `sql-dialect-sdk::lex`.
  - **D2** the dbt analyzer reads the adapter from the `profiles.yml` beside `dbt_project.yml`
    (through `read_redacted`; only the adapter name leaves); a `duckdb` project parses DuckDB
    first, any other model falls back to DuckDB after Trino. Pass version `v6`, adapter in the
    cache key, models carry `dbt_adapter`.
  - **D3** `migrate-validate::duckdb::DUCKDB` as `Dialect::Registered(&DUCKDB)`, core functions
    only (no `icu`: a `TIMESTAMPTZ` renders through `epoch_us`, so the session's `TimeZone` never
    reaches the canonical form — tested under three zones). The kit passes 31 cases, 4 hashes and
    the bucket check through the 1.5.5 CLI (`EKOS_DUCKDB_CLI`) and in process (1.5.6,
    `ekos-plugin-duckdb`, feature `duckdb`); a planted one-cent difference in one of 500 rows
    changes exactly its bucket.
    The `duckdb` crate is not a dev-dependency of `migrate-validate`: its bundled build takes
    ~5 min (debug) / 8 min (release, 8 jobs), too much for every `cargo test --workspace`.
  - **D4** `plugins/duckdb` (feature `duckdb` → `duckdb = "=1.10506.1"`, i.e. DuckDB 1.5.6,
    bundled, no extensions — not 1.5.5's `=1.10505.0`, whose `libduckdb-sys` build-depends on
    `zip` 6 with four RustSec advisories even though the bundled build never unzips; 1.5.6 reads
    files the 1.5.5 CLI writes, which the CLI-made test file shows): the file opened `READ_ONLY`,
    then `autoinstall/autoload` off, `enable_external_access = false`, `lock_configuration = true` before the first statement; the
    shared single-`SELECT` guard plus a DuckDB guard refusing file/network table functions, a quoted
    path in `FROM`, `getenv` and `query()`; a deadline by `interrupt()` from a timer thread
    (default 30 s). `ekos sql check|generate --duckdb <file>`. Tested: `read_csv`, `ATTACH`, `SET`,
    `COPY`, `INSERT`, `CREATE` refused by the guard **and**, sent unguarded, by DuckDB itself; a
    long cross join interrupted at a 0.3 s deadline with the session still usable; the directory
    holding the file byte-identical after a run (no WAL). Open question 1 answered: the catalog
    belongs in `plugins/duckdb` beside the probe, sharing its `session` module.
  - **D5 not built** (see the branch report): `[[databases]] kind = "duckdb"` needs a
    `DatabaseKind`, a `CatalogDialect` arm in `recovery::oracle_catalog_analyzer`, a `Flavor`
    arm in `observation-sdk`'s `signals`/`overlap`/`probe`/`profile` builders (RFC 0252), the
    build/database/profile/relationships command arms and the console's Sources form.

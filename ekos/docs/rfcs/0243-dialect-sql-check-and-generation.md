# RFC 0243 — Per-dialect SQL checking and generation in every SQL plugin

**Status:** Accepted
**Created:** 2026-11-05
**Builds on:** RFC 0031 (SQL dialects), RFC 0211/0235 (live Oracle, PostgreSQL, ClickHouse
catalogs), RFC 0242 (text-to-SQL checks and agent)

## 1. Problem

Until now, EKOS's SQL plugins only **read**:

- a `sql-dialect-*` crate parses DDL and transforms for recovery;
- a database plugin (`postgres`, `clickhouse`, `oracle`) reads a live catalog.

RFC 0242 added checks and an agent for SQL that someone *writes*. They have two limits:

- **They know little about dialects.** Three dialects state facts; none says how to write SQL for
  it (Oracle has no `LIMIT`, ClickHouse's `/` is float, MySQL reads `"x"` as a string).
- **They run only on SQLite.** The probes (result, value spellings, fan-out, ties) run only
  against the BIRD bench's SQLite files.

The user asked for each SQL plugin to also check SQL and help generate it for its own engine.

## 2. Design

### 2.1 What a dialect plugin states (`sql-dialect-sdk`)

`SqlDialectParser` gains `generation_notes(&self) -> &'static [&'static str]`: short rules for
writing correct SQL for that engine. They cover row limits, integer division, string
concatenation, case-insensitive matching, date literals and functions, identifier quoting, and
NULL ordering.

The default is empty. Every plugin fills it in, and `facts()` with it:

| Dialect | Row limit | Integer `/` truncates | NULLs first ascending | `ILIKE` | Identifier quote |
|---|---|---|---|---|---|
| generic | `LIMIT` | not claimed | not claimed | — | `"` |
| sqlite | `LIMIT` | yes | yes | no | `"` (also `` ` `` `[]`) |
| postgres | `LIMIT` / `FETCH FIRST` | yes | no (last) | yes | `"` |
| mysql | `LIMIT` | no (`/` is decimal; `DIV` truncates) | yes | no | `` ` `` (`"` is a string) |
| mssql / tsql / synapse | `TOP` / `OFFSET … FETCH` | yes | yes | no | `[]` or `"` |
| snowflake | `LIMIT` / `FETCH` | no | no | yes | `"` |
| databricks | `LIMIT` | no | yes | yes | `` ` `` |
| clickhouse | `LIMIT` | no (`intDiv` truncates) | no (last) | yes | `` ` `` or `"` |
| trino | `LIMIT` / `FETCH FIRST` | yes | no (last) | no | `"` |
| oracle | `FETCH FIRST n ROWS ONLY` (12c+) | no | no (last) | no | `"` |

`DialectFacts` gains three fields: `row_limit: RowLimit` (`Limit` | `FetchFirst` | `Top` |
`LimitOrFetch` | `TopOrFetch`), `identifier_quote: char`, and `double_quote_is_string: bool`
(MySQL by default). Everything is `Default`, so a dialect states only what it knows.

### 2.2 New checks (`ekos-sqlgen` lint)

| Code | Rule |
|---|---|
| `L009` | The row-limit form is not the dialect's: `LIMIT` on Oracle or SQL Server, `TOP` elsewhere, `FETCH FIRST` on MySQL or SQLite |
| `L010` | `ILIKE` on a dialect without it |
| `L011` | An identifier quoted the dialect's wrong way: `` ` `` on PostgreSQL, Oracle, Trino or Snowflake; `"name"` on MySQL, which reads it as a string |
| `E001` | The query does not parse under the dialect's parser (a warning: sqlparser's coverage of a dialect is incomplete, so the engine has the last word when a probe runs) |

`ekos sql check` and `ekos_sql_check` also return the dialect's `generation_notes`. An agent then
gets them **before** writing, not only as findings after.

### 2.3 Probes on live databases

Each database plugin implements `ekos_sqlgen::probe::Probe` over its read-only session:

| Plugin | Session | Read-only guarantee |
|---|---|---|
| `plugins/postgres` | `pg-live::PgSource` | `default_transaction_read_only`, `statement_timeout`, **and** the role's write check must say "refused" (a session setting can be undone by a statement; a role without privileges cannot) |
| `plugins/clickhouse` | catalog HTTP client | `readonly=1` on every request (a query cannot change settings under it) |
| `plugins/oracle` | `ora-live::OraSource` (feature `oracle`) | `SET TRANSACTION READ ONLY` |
| SQLite (`--sqlite FILE`) | `rusqlite` read-only open | `SQLITE_OPEN_READ_ONLY`, no `ATTACH` |

Before any of them sends SQL, `ekos_sqlgen::guard::single_select(sql, dialect)` must accept it:

- exactly one statement, a query;
- no `SELECT … INTO`;
- no locking clause (`FOR UPDATE` / `FOR SHARE`).

The probe then wraps it as `SELECT * FROM (<sql>) AS ekos_probe <row limit>` in the dialect's own
row-limit form, so at most the asked rows come back.

A database that fails the guarantee is not probed. The command says why and checks statically
instead.

### 2.4 Commands

- **`ekos sql check [FILE|-] [--dialect D] [--database NAME | --sqlite FILE] [--json]`.**
  - `--database` takes a `[[databases]]` entry. Its kind sets the dialect: postgres, clickhouse
    or oracle.
  - With a database, the probes run too: P001–P005 and E000.
  - The class becomes `external` when `--database` is given: it reaches a live system. The
    catalogue entry is `external`, since one class covers the command.
- **`ekos sql generate "<question>" [--dialect D] [--database NAME | --sqlite FILE]
  [--max-turns N] [--allow-metered] [--json]`.** This is RFC 0242's agent (tools and verify, cell
  K2's design) over the project's ledger schema.
  - The task prompt holds the schema (tables, columns, keys, descriptions, foreign keys), the
    dialect's generation notes and the question.
  - The probe tools (`run_sql`, `check_sql` with probes) are offered only with a database.
  - The model route is `[llm.tasks.nl-sql]`. A metered route needs `--allow-metered`, as
    `eval bench run` does.
  - The output is the SQL, the turns, the tools used and the findings left open.
  - The catalogue class is `external`.
- **MCP.** `ekos_sql_check` returns `dialect_notes`. `sql` becomes optional: called with only
  `dialect`, it returns the notes, for an agent about to write SQL. The MCP server still never
  opens a database.

### 2.5 Not in scope

- Executing generated SQL for its answer: `sql generate` returns SQL; it does not return data.
- Write statements of any kind.
- MySQL / SQL Server / Snowflake / Databricks live probes. No live plugin exists for them yet;
  they get notes, facts and static checks.

## 3. Verification

- Per dialect: facts and notes are non-empty for every registered name. A registry test fails
  for a dialect with no notes.
- `L009`–`L011` and `E001`: a positive and a negative case each.
- The guard refuses multi-statement text, an unbalanced-paren escape
  (`SELECT 1) x; DELETE …`), `SELECT … INTO` and `FOR UPDATE`, and accepts CTEs and set
  operations.
- The probe implementations' row conversion is unit-tested (ClickHouse `JSONCompact`, PostgreSQL
  NULL as `None`). The live runs use the existing compose testbeds where available.
- `sql generate` runs end to end with a scripted provider over a fixture project and a SQLite
  file.

## 4. Results (2026-11-05)

### 4.1 Live runs

These ran against EKOS's own testbed (`docker-compose.migrate.yml`). The fixture is schema
`sqlgen` with a read-only role `sqlgen_ro`, and ClickHouse database `sqlgen`. A scratch project
has three `[[databases]]` entries: `pg` (`sqlgen_ro`), `pgadmin` (`ekos`) and `ch`.

| Query | Result |
|---|---|
| `COUNT(*) * 100 / (SELECT COUNT(*) …) … WHERE city = 'Germany'` on `pg` | L005, and P003: 0 exact rows, 1 containing (`'Berlin, Germany'`); the result shown |
| Any query on `pgadmin` | Not probed: the role can write. The reason is printed and the checks are static |
| `SELECT 1; DELETE FROM sqlgen.orders` on `pg` | Refused by the guard (`not sent`); the table still has its 4 rows |
| `… ORDER BY amount DESC LIMIT 1` on `ch` | P005: the first two rows tie |
| `ILIKE … LIMIT 3` with `--dialect oracle` | L009 and L010 |
| `sql generate` on `pg` and `ch` (local `qwen2.5-coder:3b`) | The agent used every tool and the probes ran read-only. ClickHouse: correct. PostgreSQL: the ratio inverted, L005 left open |

### 4.2 What the live runs found, and the fixes

- **P003 queried a bare table name.** The query named `sqlgen.customers`; the probe queried
  `"customers"`, which failed silently. Probes now name each table as the query does
  (`Analysis::written`).
- **L005 missed a scalar subquery** (`… / (SELECT COUNT(*) …)`). A subquery now takes its column's
  kind.
- **The PostgreSQL driver panics inside an async runtime.**
  - `sql generate` runs the agent on a runtime.
  - A database session now lives on its own plain thread (`ProbeWorker` in
    `commands/sql_check.rs`) and is queried over a channel.
- **The model wrote unqualified names for a live table in another schema.**
  - `Schema` now carries each live table's `owner` as its schema and shows `sqlgen.orders`.
  - Name lookup accepts either form.
  - `--database` narrows the schema to that entry's own live tables (two entries on one server
    hold the same tables).
- **Two probe queries were SQLite-only:** `SUM(col = 'x')`, and an inner `LIMIT` in P003/P005.
  They are now portable: `SUM(CASE WHEN … THEN 1 ELSE 0 END)`, and the probe reads the first
  rows.

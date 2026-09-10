# Devlog 177 — a real workspace silently lost its entire DB schema

**Date:** 2026-09-10
**PRs:** (docs only) `docs: [recover.sql] dialect rules are load-bearing for schema dumps`
**Branch:** main (local → pushed)

---

## Summary

A user reported that `ekos compile`/`commit` on the `analytics` (Plausible Analytics) workspace
"didn't read and save database tables and columns." It hadn't: the ledger held **0 `Table`
objects** for a repo whose full schema sits in two checked-in dump files with 56 `CREATE TABLE`
statements between them. Root cause was a **config regression** — the `[recover.sql]` dialect
rules had been dropped from that workspace's `ekos.toml` — but the failure mode it exposed is the
real lesson: a wrong SQL dialect produces zero tables *silently*, as an `SQL001` warning rather
than an error, and nothing downstream notices the whole database schema is missing.

---

## Investigation

`.ekos/diagnostics/recover.log`:

```
[Warning] SQL001: no tables found in priv/ingest_repo/structure.sql
[Warning] SQL001: no tables found in priv/repo/structure.sql
```

- `priv/repo/structure.sql` — 41 PostgreSQL `CREATE TABLE`s (`pg_dump` output)
- `priv/ingest_repo/structure.sql` — 15 ClickHouse `CREATE TABLE`s

Both files were observed and handed to `SqlAnalyzerPass`. They produced nothing because the
workspace `ekos.toml` had **no `[recover.sql]` section**, so both parsed under the `generic` ANSI
dialect. `parse_ddl_structural` (`crates/recovery/src/sql_analyzer.rs`) runs one whole-file
`Parser::parse_sql`; if *any* statement fails, it returns an empty graph. `pg_dump` output is
full of statements `sqlparser`'s generic dialect rejects (`CREATE TYPE … AS ENUM`,
`CREATE EXTENSION`, `COMMENT ON EXTENSION`, `CREATE SEQUENCE`, `SET`, dollar-quoted functions,
`ALTER TABLE ONLY … NOT VALID`); ClickHouse DDL has `ENGINE = MergeTree`, `CODEC(ZSTD(3))`,
backtick-quoted columns. Either file loses all its tables on the first unsupported line.

The dialect-specific preprocessing that makes *these exact two files* parse
(`PostgresDialectParser`/`ClickHouseDialectParser::preprocess`, RFC 0057 / 0058 / 0059 — all
found and fixed against this same repo) only runs when `[[recover.sql.dialect-rules]]` routes the
file to `postgres` / `clickhouse`. Those rules were in the workspace's `ekos.toml` for the
devlog_60/61 sessions and were dropped when devlog_174's corpus-contamination cleanup rewrote it
down to "backend-Elixir-only."

## Fix

Restored to `analytics/ekos.toml` (a separate repo — not tracked here):

```toml
[recover.sql]
default-dialect = "generic"

[[recover.sql.dialect-rules]]
path-glob = "priv/repo/**"
dialect = "postgres"

[[recover.sql.dialect-rules]]
path-glob = "priv/ingest_repo/**"
dialect = "clickhouse"
```

Re-ran `recover → resolve --force → compile → commit`. Result:

| | before | after |
|---|---|---|
| `Table` objects | 0 | **57** (42 Postgres + 15 ClickHouse) |
| columns | — | full, with data types, on each `Table`'s `columns` property |
| evidence | — | each table → `CREATE TABLE …` at `priv/**/structure.sql` |

`resolve` needed `--force` for 4 pre-existing cross-kind name conflicts (`application` / `config`
/ `error` / `clickhouse` each appear as both an Elixir function symbol and a module) — the same 4
the Sep-9 build carried.

---

## Knowledge Captured

- **A wrong SQL dialect fails silently as "0 tables", never an error.** `SQL001: no tables found`
  for a file that visibly contains `CREATE TABLE` is the signature of a dialect mismatch, not an
  empty file. `parse_ddl_structural` has no per-statement fallback: one statement `sqlparser`
  can't handle discards every table in that file. Worth making louder — see TODO.
- **`[recover.sql]` dialect rules are load-bearing config, not a nicety.** Any repo with a real
  schema dump (`pg_dump` / ClickHouse `structure.sql`, or hand-written DDL past plain ANSI) needs
  its schema files routed to a real dialect. Drop the rules and the *entire database schema*
  vanishes from the ledger with only a buried warning — no other pipeline stage flags it, and
  `ekos ekl "FIND Object WHERE kind = 'Table' COUNT"` returning 0 is the only tell.
- **`ekos.toml` cleanups are dangerous.** devlog_174 trimmed this file for the right reason
  (94% of the corpus was a Python venv) and silently took the SQL dialect rules with it. When
  editing a workspace `ekos.toml`, diff every removed section against what it was doing.
- **Columns are a property, not objects.** `parse_ddl_structural` stores columns as a `columns`
  JSON array (`{name, data_type}`) on the `Table` object — there is no `Column` `ObjectKind`.
- **`ekos resolve` hard-errors on cross-kind name conflicts now**; `--force` is the documented
  continue-anyway. A large Elixir codebase reliably hits a few (`Foo` the module vs `foo` the
  function).
- **`llama3:latest` doesn't reliably emit the SQL analyzer's strict JSON schema** → `SQL002`
  "LLM enrichment parse failed", structural extraction still applied. Tables + columns are
  complete; only the LLM's PascalCase business-entity names are missing.

---

## Follow-ups (not done here)

- `SQL001` should be **louder and actionable** when the file contains `CREATE TABLE` text but
  parsed to zero tables — that combination is almost always a dialect misconfiguration, and it
  currently reads like "this file has no schema." Added to TODO.md.
- Ecto `schema "events" do … end` / `create table(:events)` migration → `Table` promotion has no
  Elixir equivalent of the Python analyzer's SQLAlchemy support (RFC 0091). Redundant where a
  `structure.sql` exists, but it's the only source of Ecto association metadata.
- `pg_dump` foreign keys (`ALTER TABLE ONLY … ADD CONSTRAINT … FOREIGN KEY`) are separate
  statements `parse_ddl_structural` doesn't attach to the table — cross-table `ForeignKey` edges
  from a Postgres dump are currently 0.

---

## Files Changed

| File | Change summary |
|---|---|
| `README.md` | Note that `[recover.sql]` dialect rules are required for schema-dump files and that a wrong dialect fails silently |
| `docs/generated/ekos-self-documentation.html` | Same note in the Knowledge-recovery section |
| `TODO.md` | Follow-up: make `SQL001` actionable when `CREATE TABLE` text is present but 0 tables parsed |

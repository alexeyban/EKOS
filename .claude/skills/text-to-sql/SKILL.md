---
name: text-to-sql
description: >-
  Write SQL (or a dbt model's SELECT) for a question about a database whose schema EKOS has
  compiled, using the ekos MCP tools to ground every table, column, join and value, and
  ekos_sql_check to catch mistakes before answering. Use whenever the user asks for a query,
  report SQL, a metric definition or a dbt model against tables in an EKOS project — "write SQL
  that…", "how many X per Y", "build a model for…" — even if they never say "EKOS".
---

# Text-to-SQL over an EKOS project

EKOS compiles a database's DDL, comments and foreign keys into its ledger. Use the ledger as
the schema: never guess a table, column, join or literal value you have not seen there or in
the data. Measured on BIRD Mini-Dev (RFC 0241/0242), most wrong answers ran fine and looked
plausible; the errors were in meaning — a value spelled differently in the data, rows repeated
by a join, a sentinel `0` averaged in, integer division, NULLs sorted first. The procedure below
targets exactly those.

## Procedure

0. **Learn the engine's rules.** Call `ekos_sql_check` with only `dialect` (or none, for the
   project's): it returns that engine's rules — row limits (`FETCH FIRST` on Oracle, `TOP` on SQL
   Server), integer division, quoting, case sensitivity, dates. Follow them while drafting.
1. **Find the tables.** `ekos_search` with the question's nouns (2–3 keywords), or
   `ekos_ekl` `FIND Object WHERE kind = 'Table'` for a small schema. Read each candidate with
   `ekos_state` (its `columns` property holds names, types, keys and descriptions).
2. **Find the join path.** Join only on foreign keys the ledger records
   (`ekos_neighborhood` on the table shows its `ForeignKey` edges). Joining two child tables
   on a shared parent key multiplies rows — aggregate each side first, or count `DISTINCT`.
3. **Check every literal.** A value in the question ("Germany", "retail", "2017") must match
   the stored spelling and format. If a database is available, look: `SELECT DISTINCT col …
   WHERE col LIKE '%Germany%' LIMIT 10`. Text may be a list (`'Artifact,Creature'`) → `LIKE`,
   or part of a longer value (`'Berlin, Germany'`) → `LIKE`; dates are often text in a fixed
   format → compare in that format or with `substr`/`strftime`.
4. **Decide the grain.** What is one row of each table, and what does the question count —
   entities, events, rows? Count `DISTINCT` entities when a join repeats them.
5. **Draft the SQL**, returning exactly the columns asked, in the order asked.
6. **Run `ekos_sql_check`** with the draft (and `dialect` if not the project's). Fix every
   `error`; read every `warn` and either fix it or say why it does not apply:
   - `L001` unknown name → use the suggested real name.
   - `L002` join off the foreign keys → use the path given, or justify the join.
   - `L003` `=` on a list column → `LIKE '%x%'`.
   - `L005` integer division → multiply the numerator by 1.0 (or cast it) for ratios and percentages.
   - `L006` aggregate over a sentinel → filter the sentinel out.
   - `L007` ascending `ORDER BY … LIMIT` on a nullable column → `IS NOT NULL`.
   - `L008` date literal format → match the stored format.
   - `L009` row-limit form, `L010` `ILIKE`, `L011` identifier quoting → the dialect's own form.
7. **Preview when you can.** A person (not the MCP server) can run `ekos sql check q.sql
   --database <name>` (or `--sqlite <file>`): the same checks plus read-only probes on the live
   database — the result, value spellings, rows repeated by joins, ties. `ekos sql generate
   "<question>" --database <name>` runs this whole procedure as an agent. With any read-only
   connection: an empty result, a single NULL, or two
   rows tied at the top of an `ORDER BY … LIMIT 1` all mean the query needs another look.
8. **Answer** with the SQL, the tables and join path it uses, and any assumption you made
   about a value or definition the ledger does not state.

## dbt models

The same steps apply to a model's `SELECT`. Reference upstream models and sources with
`{{ ref('…') }}` / `{{ source('…', '…') }}` names that exist in the ledger (dbt models are
`Table` objects recovered from the project). Check the SELECT with `ekos_sql_check` before
templating it, with `dialect` set to the warehouse's (`trino`, `postgres`, `snowflake`, …).

## What not to do

- Do not invent a column because the question uses a word for it — search, then ask if absent.
- Do not trust an empty findings list as proof: the check is static and cannot see values or
  row counts. Steps 3, 4 and 7 cover what it cannot.
- Do not run anything that writes. EKOS's tools are read-only; so must your previews be.

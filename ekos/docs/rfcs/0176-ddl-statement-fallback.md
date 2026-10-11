# RFC 0176 — Structural DDL parsing degrades per statement

| | |
|---|---|
| **Status** | Implemented (2026-10-11) |
| **Depends on** | RFC 0031 (SQL dialects), RFC 0146 (PostgreSQL preprocessing, the shared lexer) |

## Problem

`parse_ddl_structural` parses a file with one `Parser::parse_sql` call. One statement the grammar does not know discards every table in the file. The only trace is `SQL001: no tables found in <file>`, which reads as "this file declares no schema".

It has cost a whole schema twice: `analytics` lost 56 tables to a missing dialect rule (devlog_177), and LedgerSMB recovered 0 of 103 tables under the right dialect (RFC 0146). RFC 0146 fixed the statements it met; the next unknown statement has the same effect.

Separately, `pg_dump` declares foreign keys as `ALTER TABLE … ADD CONSTRAINT … FOREIGN KEY` after the tables. Those statements were ignored, so a dump produced tables and no `ForeignKey` edges.

## Design

**Per-statement fallback.** When the whole file fails to parse, and the existing missing-`;` repair fails too:

1. Split the text with `ekos_sql_dialect_sdk::lex::statement_spans`, which never ends a statement inside a dollar-quoted body, a string or a comment.
2. Parse each statement alone. One that fails is replaced by spaces, keeping its line breaks.
3. Parse the blanked file as a whole.

Blanking, not concatenating the survivors, keeps every line number where it was, so evidence spans, seed-row lines and comment lines stay right, and nothing downstream changes. A file that parses whole takes the old path and pays nothing.

**It reports what it skipped.** `parse_ddl_structural_report` returns the graph with a `DdlParseReport`: statements seen, and for each one skipped its line, its first words and the parser's error. `parse_ddl_structural` stays as it was for callers that do not need the report.

- `SQL005` (warning): `<file>: N of M statements were not parsed and skipped`, with the first three as `line L: <first words> — <error>`. A skipped `CREATE TABLE` is named as such, because that is a lost table.
- `SQL001` says why. When the file contains `CREATE TABLE` but no table was recovered: `no tables recovered from <file>, though it contains N CREATE TABLE statement(s): none parsed under dialect <name>. Check [[recover.sql.dialect-rules]]`, with the first error. A file with no `CREATE TABLE` keeps the old line.

**`ALTER TABLE … ADD … FOREIGN KEY`** on a table the same file creates becomes a `ForeignKey` relationship, by the same rule and id scheme as a constraint inside `CREATE TABLE`. A reference to a table declared in another file is still skipped, as it is for inline constraints.

The pass version goes to `v4`.

## Not in this RFC

- `ALTER TABLE … ADD COLUMN` and the rest of `sql/changes/` applied on top of the base schema. That is a model of schema history, not a parsing fix.
- Table inheritance as a relationship kind.
- The same fallback for `sql_transform_analyzer`, whose splitter is still `split(';')`.

## Tests

- A file with one unknown statement between two `CREATE TABLE`s yields both tables and reports one skipped statement on the right line.
- A `;` inside a dollar-quoted body does not split a statement.
- Seed rows after a skipped statement cite their real line.
- A file that parses whole reports nothing skipped.
- A file whose only `CREATE TABLE` fails yields `SQL001` naming the dialect and the error; a file with no `CREATE TABLE` keeps the plain message.
- `ALTER TABLE ONLY a ADD CONSTRAINT … FOREIGN KEY (b_id) REFERENCES b(id)` yields the edge; a reference to an undeclared table yields none.

# RFC 0210 — Oracle SQL Dialect, PL/SQL Procedural IR, Schema History from Migration Scripts

**Status:** Accepted
**Date:** 2026-10-22
**Depends on:** RFC 0031 (`SqlDialectParser`), RFC 0146 (preprocess transforms, shared SQL lexer),
RFC 0163 (`ProcedureIr`, `PlPgSqlAnalyzerPass`), RFC 0169 (`View`), RFC 0172 (`ConflictingEvidence`),
RFC 0176 (per-statement fallback, blanking keeps line numbers), RFC 0209 (ticket anchors)
**Plan:** `oracle-idempiere-plan.md` v3, Part I.4–I.5
**Testbed:** iDempiere `release-13` @ `3355eb5fc9403c3827fcd8caf8eb92f78319430e`

---

## 1. Motivation

EKOS has no Oracle dialect. `init --detect` already scores Oracle markers (`varchar2`, `number(`,
`nvl(`, `dual`) and writes `default-dialect = "oracle"`, but no crate answers to that name. The SQL
is then read by the ANSI baseline, which fails on SQL\*Plus commands, PL/SQL units and a handful of
Oracle-only clauses.

Measured on the pinned testbed, before this RFC:

| What | Count |
|---|---|
| Oracle migration scripts (`migration/*/oracle/*.sql`) | 1,084 |
| of those named `<timestamp>_<description>.sql` | 1,076 |
| statements in them (the shared splitter, `;` outside quotes and comments) | 58,328 |
| statements `GenericDialect` rejects | 5,317 |
| — causes: `… ENABLE NOVALIDATE` constraint states, SQL\*Plus `SET SQLBLANKLINES ON` / `SET DEFINE OFF`, PL/SQL units cut at their inner `;`, `ALTER TABLE … ADD (…)` / `MODIFY (…)` / `DROP (…)` column lists | |
| statements hidden from the splitter by `CREATE$JAVA$LOB$TABLE` (read as a PostgreSQL `$JAVA$` dollar quote) | ~4,000 |
| PL/SQL files in `db/oracle` (functions, procedures) | 61 |
| older ADempiere scripts (`migration-historic/*/oracle`) | 1,666 |
| `ConflictingEvidence` after a whole-tree commit | 2,372 |
| — of those, an Oracle script against its PostgreSQL twin | **2,372** |

That last row is the important finding. iDempiere ships every migration twice, once per engine
(`migration/iD13/oracle/202507311636_IDEMPIERE-6640.sql` and
`migration/iD13/postgresql/202507311636_IDEMPIERE-6640.sql`). Both create the same table, one with
`NUMBER(10)` and one with `NUMERIC(10)`, one with `VARCHAR2(36)` and one with `UUID`. RFC 0172
compares them as two claims about one table and reports every column as a disagreement. They are
not a disagreement: they are two databases.

The migration scripts are also the only place the schema's **history** is written: which ticket
added which column, when, in which release. Read as competing definitions they produce noise; read
as dated change sets they answer "when and why did this column appear".

## 2. Goals / Non-goals

**Goals**

1. `plugins/sql-dialect-oracle`: an `oracle` dialect that parses what Oracle schemas and SQL\*Plus
   scripts contain, by line-preserving preprocessing over `GenericDialect`.
2. Definitions record the dialect they were read under; conflict detection compares claims within
   one dialect only. `init --detect` writes per-directory dialect rules for engine-named sibling
   directories (`oracle/`, `postgresql/`, …).
3. `crates/plsql`: a PL/SQL front end producing RFC 0163's `ProcedureIr` — functions, procedures,
   triggers; package specs and bodies at signature level — with computed fidelity.
4. `PlSqlAnalyzerPass`: the same `Procedure` / `ProcedureStatement` / `Trigger` objects
   `PlPgSqlAnalyzerPass` writes, so `procedure_lineage`, docs, MCP and the console need no change.
5. Schema history: every migration script named `<timestamp>_<description>.sql` becomes a
   `SchemaChangeSet` with its timestamp, release, engine and ticket, and every DDL statement in it a
   `SchemaChange` (operation, table, column, type) linked to the table at commit.

**Non-goals**

- Lowering PL/SQL into the Transformation IR (RFC 0164 has not lowered PL/pgSQL either). The
  statements carry their text and footprint; `ekos_transformation_diff` of the `db/oracle` and
  `db/postgresql` twins waits for that lowering.
- Checking script claims against a live catalog — RFC 0211.
- Application Dictionary DML (`INSERT INTO AD_Column …`) as schema change: those rows name tables by
  numeric id, resolvable only against the dictionary data. Counted per change set, not interpreted.
- A cross-engine type equivalence table. Two engines are two databases; their types are not
  compared at all.

## 3. Design

### 3.1 The `oracle` dialect (`plugins/sql-dialect-oracle`)

Base grammar: `GenericDialect` (measured: it already parses Oracle DML, `DECODE`/`NVL`/`NVL2`,
`ROWNUM`, `FROM dual`, `TO_DATE`/`TO_TIMESTAMP`, `CONNECT BY … START WITH`, `MERGE`,
`MODIFY col type`, `VARCHAR2(n CHAR)`, `DEFERRABLE INITIALLY DEFERRED` in either order).

`preprocess` applies, in order, on code only (strings, quoted identifiers and comments are never
touched — an Oracle-rules scanner in `ekos_plsql::script`: `''` escapes, a backslash is a plain
character, `q'…'` quoting):

| # | Construct | Action |
|---|---|---|
| 1 | `q'[…]'`, `q'{…}'`, `q'<…>'`, `q'(…)'`, `q'X…X'` alternative quoting | rewritten to an ordinary literal of the same length, contents blanked |
| 2 | SQL\*Plus commands at statement start: `SET`, `PROMPT`, `SPOOL`, `WHENEVER`, `EXIT`, `QUIT`, `REM[ARK]`, `DEFINE`, `UNDEFINE`, `SHOW`, `@file`, `@@file`, and a `/` alone on a line | blanked, to the end of the line |
| 3 | PL/SQL units: `CREATE [OR REPLACE] [EDITIONABLE] FUNCTION | PROCEDURE | TRIGGER | PACKAGE [BODY] | TYPE [BODY]`, anonymous `DECLARE …`/`BEGIN …` blocks | blanked up to their terminating `/` line (they are read by §3.3, not by the DDL parser) |
| 4 | constraint state `ENABLE`, `DISABLE`, `VALIDATE`, `NOVALIDATE`, `RELY`, `NORELY` | blanked |
| 5 | `ALTER TABLE t ADD (c1 T1, c2 T2)` / `MODIFY (…)` | rewritten to `ADD c1 T1, ADD c2 T2` / `MODIFY c1 T1, MODIFY c2 T2` |
| 6 | `n BYTE` length semantics in a type argument | `BYTE` blanked (`n CHAR` already parses) |
| 7 | `CREATE [OR REPLACE] [NO]FORCE [EDITIONABLE] VIEW` | the modifiers blanked |
| 8 | `DROP TABLE t CASCADE CONSTRAINTS [PURGE]`, `DROP … PURGE` | the trailing clauses blanked |
| 9 | `CREATE SEQUENCE s <options>` | options blanked (the sequence itself kept) |
| 10 | `ALTER VIEW | TRIGGER | FUNCTION | PROCEDURE | PACKAGE x COMPILE | ENABLE | DISABLE` | the statement blanked |
| 11 | `(+)` outer-join marker | blanked; the predicate stays, so lineage still sees both tables |
| 12 | `MINUS` set operator | rewritten to `EXCEPT` (identical meaning) |
| 13 | `MODIFY c DEFAULT x` / `c NOT NULL` / `c NULL` (attribute only) | rewritten to `ALTER COLUMN c SET DEFAULT x` / `SET NOT NULL` / `DROP NOT NULL` — as `MODIFY`, `sqlparser` rejects `DEFAULT` and reads `NOT`/`NULL` as a type name |
| 14 | `ALTER TABLE t DROP (c1, c2) [CASCADE CONSTRAINTS]` | `DROP COLUMN c1, DROP COLUMN c2` |
| 15 | physical attributes: `TABLESPACE`, `STORAGE (…)`, `PCTFREE`, `LOGGING`, `USING INDEX …`, `ONLINE`, … | blanked |
| 16 | an identifier holding `$` or `#` (`CREATE$JAVA$LOB$TABLE`) | double-quoted; unquoted, the shared splitter reads `$JAVA$` as a dollar quote and hides the rest of the file |
| 17 | `ALTER INDEX x REBUILD | UNUSABLE` | the statement blanked |

Blanking replaces every character but line breaks with a space (RFC 0176), so diagnostics and
evidence keep the file's own line numbers. Rules 5, 12, 13, 14 and 16 lengthen a line; none adds
one.

The dialect is registered as `oracle` (alias `plsql`) in `sql_dialect_registry`.

### 3.2 Dialect on definitions, conflicts within one dialect

`SqlAnalyzerPass` stamps `dialect` (the resolved registry name) on every `Table` it writes.
`conflicts::duplicate_definitions` groups the claims about one id by dialect and compares only
claims in the same group; a claim read under `generic` is compared with every group (its engine is
unknown, so silence would hide real disagreements). Two engines' definitions of one table remain one
`Table` object, as before — what changes is that their type spellings are no longer reported as a
dispute.

`init --detect` already guesses one default dialect. It now also writes one
`[[recover.sql.dialect-rules]]` entry per engine-named directory found holding `.sql` files —
`oracle`, `postgresql`/`postgres`/`pg`, `mysql`, `mssql`/`sqlserver`, `snowflake`, `clickhouse` —
e.g. `path-glob = "**/oracle/**"`, `dialect = "oracle"`, with a comment giving the file count.

### 3.3 PL/SQL (`crates/plsql`)

PL/SQL and PL/pgSQL share their statement grammar (both descend from Ada): `IF/ELSIF/ELSE`,
`LOOP`/`WHILE`/`FOR … IN … LOOP`, `EXIT WHEN`, `RETURN`, `:=`, `SELECT … INTO`, cursor
`OPEN/FETCH/CLOSE`, `BEGIN … EXCEPTION WHEN … END`. They differ in the routine header, the
declaration section, and a few statement forms. So `ekos-plsql` is a front end, not a second parser:

- **Lexing and splitting.** Oracle quoting (`'…'` with doubled quotes, `q'…'`, `"…"`), `--` and
  `/* */` comments; units end at a `/` alone on a line (SQL\*Plus), or at end of file.
- **Header.** `CREATE [OR REPLACE] [EDITIONABLE] FUNCTION|PROCEDURE name [(params)] [RETURN type]
  [AUTHID …] [DETERMINISTIC] [PIPELINED] IS|AS` → `ProcSignature { name, arguments, returns,
  language: "plsql" }`. Parameters keep their mode (`IN`, `OUT`, `IN OUT`, `NOCOPY`).
- **Declarations.** Everything between `IS|AS` and the unit's top-level `BEGIN`: variables,
  constants, `%TYPE`/`%ROWTYPE`, `CURSOR c [(params)] IS <query>;` (a declaration whose query is
  kept), nested `PROCEDURE`/`FUNCTION` definitions (kept as `Unrecovered` declarations with a reason,
  never dropped), `TYPE`/`SUBTYPE`/`PRAGMA`/exceptions.
- **Body.** The `BEGIN … END [name];` block is handed to `ekos_plpgsql::parse_body` with a new
  `Flavor::PlSql` on `Origin`, which adds, only under that flavor:
  - a bare call `name(args);`, `pkg.name(args);` or `name;` → `ProcStmt::Perform` (it is a call);
  - `EXECUTE IMMEDIATE expr [INTO …] [USING …]` → `ProcStmt::DynamicExecute`;
  - `RAISE_APPLICATION_ERROR(code, message)` → `ProcStmt::Raise { level: "exception" }`;
  - `FORALL i IN a..b <dml>` → `Loop { ForRange }` around the DML;
  - `SAVEPOINT x` → `Sql`; `GOTO l` → `Unrecovered` with the reason "goto" (honest: control flow
    this IR does not model).
  PL/pgSQL parsing is unchanged (the default flavor), so RFC 0163's corpus results cannot move.
- **Triggers.** `CREATE [OR REPLACE] TRIGGER name {BEFORE|AFTER|INSTEAD OF} {INSERT|UPDATE [OF
  cols]|DELETE} [OR …] ON table [REFERENCING …] [FOR EACH ROW] [WHEN (cond)] [DECLARE …] BEGIN …
  END;` → a `ProcedureIr` for the body plus trigger facts (table, timing, events, level, `WHEN`).
  `:NEW.col` / `:OLD.col` are read the way PL/pgSQL's `NEW.col` / `OLD.col` are.
- **Packages.** `CREATE PACKAGE` spec: one `ProcedureIr::signature_only` per declared routine,
  named `pkg.routine`. `PACKAGE BODY`: each routine read in full, like a standalone one (amended:
  the nested-subprogram reader parses them anyway).
- **Nested subprograms** in a declaration section are routines of their own, named `outer.inner`.
- **Comments are masked by Oracle's rules first** (block comments do not nest; PostgreSQL's do), so
  `ekos-plpgsql`'s scanner never sees a comment.
- **Fidelity** is computed by `ProcedureIr::new` as for PL/pgSQL; nothing is asserted.

### 3.4 `PlSqlAnalyzerPass`

`plpgsql_analyzer::recover_routines_in` is generalised to take the routine reader (PL/pgSQL or
PL/SQL), so both passes emit identical object shapes: `Procedure` (with `language: "plsql"`),
`ProcedureStatement` per statement with footprint (`reads`/`writes`/`calls` from
`statement_footprint_in` after the §3.1 preprocessing), `Trigger` with the trigger facts, `Contains`
edges from `File` and between statements. The pass runs on files resolved to the `oracle` dialect.
`semantic::procedure_lineage` then links routines to tables at commit unchanged.

### 3.5 Schema history

A migration script is a `.sql` file named `<timestamp>_<description>.sql` with a 12- or 14-digit
`yyyyMMddHHmm[ss]` timestamp. Its engine is its resolved dialect; its release is the nearest
ancestor directory that is not an engine name (`migration/iD13/oracle/…` → `iD13`). Tickets are the
`KEY-NNNN` ids in the description (RFC 0209's key list, extended to the description's own prefix).

`SchemaHistoryPass` (per file, deterministic) writes:

| Kind | Key | Properties |
|---|---|---|
| `SchemaChangeSet` | file path | `applied_at` (ISO, from the name), `release`, `engine`, `tickets`, `description`, `ddl_statements`, `dictionary_statements` (DML on `AD_*` tables), `other_statements` |
| `SchemaChange` | file path + ordinal | `operation`, `table`, `column`, `data_type`, `not_null`, `default`, `constraint`, `references`, `line`, the statement as evidence |

Operations: `create_table`, `drop_table`, `rename_table`, `add_column`, `modify_column`,
`drop_column`, `rename_column`, `add_constraint` (`primary_key` / `foreign_key` / `unique` /
`check`), `drop_constraint`, `create_index`, `drop_index`, `create_view`, `drop_view`,
`create_sequence`, `drop_sequence`. A statement that parses but is none of these is counted, not
emitted; one that does not parse is counted as `unparsed` with its line.

Edges: `File` —`Contains`→ `SchemaChangeSet` —`Contains`→ `SchemaChange`. At commit,
`semantic::schema_history::link_changes` adds `SchemaChange` —`Changes`→ `Table` (or `View`) by
**unique** name, the rule `procedure_lineage` uses; an ambiguous or missing name is counted.

Script facts are **claims**: the iDempiere wiki notes that failed statements are logged into
migration scripts too. Nothing here asserts that a change was applied; RFC 0211 checks the
replayed claims against the catalog.

## 4. Registries and guards

`SchemaChangeSet`, `SchemaChange`: rows in `custom_kinds::REGISTRY`, `structurally_keyed = true`
(both are keyed by path and ordinal). A test asserts the rows exist (the kinds are built from
constants, as in RFC 0209). `procedure_lineage` needs nothing new: `Procedure` and `Trigger` are
already registered. Identity: `SchemaChange`/`SchemaChangeSet` names (`ALTER TABLE … ADD …`) never
form a cross-namespace group with tables because they are keyed kinds.

## 5. Measurement and acceptance

Re-measured on the testbed and recorded in the devlog:

1. Oracle migration statements rejected under `oracle` vs `generic` (baseline 5,317 of 58,328).
2. `db/oracle` routines: count by fidelity, statements recovered / unrecovered by reason.
3. Whole-tree `ConflictingEvidence`: the 2,372 cross-engine items gone; any remaining listed by cause.
4. Schema history: change sets, changes by operation, `Changes` edges, ambiguous and missing names.
5. Model calls: 0.

Phase 1 result (`plugins/sql-dialect-oracle/tests/corpus.rs`): the 1,084 Oracle migration scripts
read as 62,251 statements (the ~4,000 previously hidden now counted), **9 rejected** — 4 foreign
keys with no referenced column list (Oracle: the primary key; `sqlparser` has no form for it, and
naming a column would invent one), 3 `CHECK (… IS JSON)`, 2 `RENAME CONSTRAINT`. Across all 2,831
Oracle-path files including `migration-historic`, 10,458 → 1,129 rejected, of which 1,098 are the
same column-less foreign key.

Phase 2 result (`crates/plsql/tests/corpus.rs`): `db/oracle` — 61 routines, 60 at `statements`,
1 `partial`; 991 of 992 statements recovered, the one gap a `GOTO`. Oracle migration scripts — 39
routines, all `statements` (587 statements), 25 anonymous blocks counted.

Phase 3 result (`schema_history_corpus`): 2,154 change sets, 26,060 changes (Oracle 21,993,
PostgreSQL 4,067), 0 unrecognized `ALTER TABLE` operations. iDempiere's PostgreSQL scripts change
columns by inserting into `t_alter_column` (406 statements); the classifier reads those rows as
`modify_column` (amendment: one family convention read as DDL, like RFC 0209's recognizer).

Whole tree (devlog_287): `ConflictingEvidence` 2,372 → 3 (two real within-engine redefinitions,
one zero-scale spelling since normalised); 3,356 change sets and 31,620 changes including
`migration-historic`; 11,748 changes linked at commit, 980 ambiguous, 17,171 naming tables no
`CREATE TABLE` in the tree defines (iDempiere's base schema ships as a dump — RFC 0211).

## 6. Phases

1. Oracle dialect plugin, registry, `q'…'` in the shared lexer, `dialect` on `Table`, conflicts
   within one dialect, `init --detect` directory rules.
2. `crates/plsql` + `Flavor::PlSql` in `ekos-plpgsql` + `PlSqlAnalyzerPass`.
3. Schema history pass + commit-time links.
4. Docs: devlog, TODO, README, `CLAUDE.md`, capabilities page.

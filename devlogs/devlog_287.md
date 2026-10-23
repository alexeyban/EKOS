# Devlog 287 — Oracle dialect, PL/SQL, schema history from migration scripts (RFC 0210)

**Date:** 2026-10-22
**PRs:** none (local branch)
**Branch:** `feat/rfc-0209-java-analyzer` (not merged; RFC 0209 and RFC 0210 together)

---

## Summary

Second step of the Oracle/iDempiere plan. EKOS gets an `oracle` SQL dialect, a PL/SQL front end
producing RFC 0163's procedural IR, and a schema-history pass that reads migration scripts as dated
change sets. On iDempiere `release-13`, the Oracle migration scripts go from 5,317 of 58,328
statements rejected to 9 of 62,251 (about 4,000 statements were hidden from the shared splitter by
one table name). All 61 routines in `db/oracle` are recovered, 60 of them completely (991 of 992
statements; the gap is a `GOTO`). The two engines' copies of every migration stop being reported as
2,372 conflicts (3 remain, real), and become 2,154 change sets with 26,060 classified changes and none unrecognized.

---

## RFC 0210 — Oracle dialect, PL/SQL, schema history

### Problem / motivation
`init --detect` already scored Oracle markers and wrote `default-dialect = "oracle"`, but no crate
answered to that name. The ANSI baseline rejected SQL*Plus commands, PL/SQL units cut at their inner
`;`, `ENABLE NOVALIDATE` constraint states and Oracle column lists. And the 2,372 open
`ConflictingEvidence` items from the RFC 0209 run were all one thing: an Oracle script against its
PostgreSQL twin, `NUMBER(10)` vs `NUMERIC(10)`.

### What was built

| Component | What it does |
|---|---|
| `crates/plsql/src/script.rs` | Oracle script scanner: `''` escapes, backslash is plain, `q'[…]'`, non-nesting block comments; units = statements / PL/SQL units ending at a `/` line / SQL*Plus lines; `neutralize_quotes`, `mask_comments` (same length) |
| `plugins/sql-dialect-oracle` | `OracleDialectParser` (`oracle`, alias `plsql`): `GenericDialect` + 17 line-preserving rewrites (RFC §3.1) |
| `ekos-plpgsql` `Flavor::PlSql` | bare calls → `Perform`, `EXECUTE IMMEDIATE`, `RAISE_APPLICATION_ERROR`, `FORALL`, `SAVEPOINT`, `GOTO` as an explicit gap, labels as `GOTO` targets — only under the flavor |
| `crates/plsql/src/parse.rs` | headers, declaration sections (cursors keep their query), nested subprograms as `outer.inner`, triggers (`:NEW` → `NEW`, same length), packages (`pkg.routine`), call specs at `Signature` |
| `recovery/src/plsql_analyzer.rs` | `PlSqlAnalyzerPass` through the new `plpgsql_analyzer::recover_parsed_routines`; an Oracle trigger = `Procedure` (body, `returns: trigger`) + `Trigger` (binding) |
| `recovery/src/schema_history.rs` | `SchemaHistoryPass`: `SchemaChangeSet` per `<yyyyMMddHHmm[ss]>_<desc>.sql`, `SchemaChange` per DDL operation, token classifier for both engines, iDempiere's `t_alter_column` rows |
| `semantic/src/schema_history.rs` | `link_changes` at commit: change → table/view (`Changes`), unique name only |
| `sql_analyzer` + `conflicts` | `dialect` on every `Table`; claims compared within one dialect (`generic` compared with all) |
| `cli/src/detect.rs` | `init --detect` writes a dialect rule per engine-named directory |
| `cli/src/commands/recover.rs` | `SqlStatsHandles`, `register_file_analysis_passes` (was complexity 34), PL/SQL and history summary lines; `TriggerAnalyzerPass` skips Oracle files |

### Measured on iDempiere `release-13` @ `3355eb5fc9`

| What | Before | After |
|---|---|---|
| Oracle migration statements (1,084 files) | 58,328, 5,317 rejected | 62,251, **9 rejected** |
| all 2,831 Oracle-path files incl. `migration-historic` | 308,187, 10,458 rejected | 308,911, 1,129 rejected (1,098 = one FK form) |
| `db/oracle` routines | not read | 61: 60 `statements`, 1 `partial`; 991/992 statements |
| PL/SQL in migration scripts | not read | 39 routines, all complete (587 statements); 25 anonymous blocks counted |
| change sets / changes (both engines) | — | 2,154 / 26,060 (Oracle 21,993, PostgreSQL 4,067), 0 unrecognized |
| PostgreSQL `modify_column` | — | 7 as DDL → 407 with `t_alter_column` rows |
| model calls | 0 | 0 |

### Whole tree (scratch `EKOS_HOME`, release build, `init --detect` config, no LLM)

| What | RFC 0209 run | This run |
|---|---|---|
| `ConflictingEvidence` | 2,372 | **3** |
| `TransformNode` (of which `Unmapped`) | 190,353 (169,062) | 52,192 (30,824) |
| `Procedure` / `ProcedureStatement` | 275 / 3,591 | 453 / 6,633 |
| `SchemaChangeSet` / `SchemaChange` (`migration` + `migration-historic`) | — | 3,356 / 31,620 |
| `Changes` links at commit | — | 11,748 linked, 980 ambiguous, 17,171 to tables no `CREATE TABLE` in the tree defines |
| `JavaType` / `JavaMethod` / `ModelTable` | 4,820 / 31,434 / 763 | unchanged |
| objects / relationships | 236,640 objects | 134,332 / 133,201 |
| `commit` | 58m21s (debug) | 22m18s (release) |

The `Unmapped` drop is the old fallback disappearing: a whole Oracle file that failed to parse was
split on every `;`, and each `INSERT`/`UPDATE` became an `Unmapped` node. Mapped nodes are stable
(Join +64, Sink +34, Filter −52). The 17,171 unlinked changes name tables whose definition is not
in the tree — iDempiere's base schema ships as a database dump; RFC 0211 compares them with the live
catalog and with RFC 0209's `ModelTable`s. Of the 3 conflicts left, 2 are real within-engine
redefinitions (`T_MoveClient` and `pp_wf_node_asset` re-created with different columns); the third,
`number(10)` vs `number(10,0)`, is the same type — `normalize_type` now drops a zero scale.

`resolve` still reports one identity conflict, `dual` as Table and View. It is real history, now
recorded: `migration-historic/360lts-i1.0a/postgresql/901_IDEMPIERE-420.sql` creates a PostgreSQL
table `dual`, and `migration/iD11/postgresql/202406072107_IDEMPIERE-6166.sql` drops it and creates a
view. The run used `resolve --force`; the conflict is left visible rather than narrowed away.

The first whole-tree attempt failed in `recover` (exit 101) — see *Knowledge Captured*.

The 9 remaining: 4 foreign keys with no referenced column list (Oracle means the primary key;
`sqlparser` has no form for it, and naming a column would invent one), 3 `CHECK (… IS JSON)`,
2 `RENAME CONSTRAINT`. Schema history reads all of them at token level anyway.

### Implementation details worth remembering
- **One Oracle scanner.** Both the dialect and the PL/SQL front end split files through
  `ekos_plsql::script`, so they never disagree about where a unit ends.
- **Spans never move.** Every rewrite of PL/SQL text before parsing (`q'…'`, backslashes, comments,
  `:NEW`) keeps the byte length, so IR spans index the file as written.
- **`recover_parsed_routines`** is the one routine writer; `recover_routines_in` (PL/pgSQL) and
  `recover_plsql` only differ in how they parse.
- **Change-set names are not file names** (`2024-02-02T09:15 IDEMPIERE-6028 (oracle)`): a `File`
  object already carries the file name, and the engine twins share it.

### Decisions
- **Dialect separation over a cross-engine type table.** Two engines' definitions describe two
  databases; mapping `VARCHAR2(36)` to `UUID` would be a claim nobody made.
- **Token classifier for history, not the AST.** `sqlparser` 0.53 reads `MODIFY c NOT NULL` as a
  column of type `NOT` and rejects `MODIFY c DEFAULT x`. The dialect rewrites those for DDL parsing;
  history reads tokens and counts what it does not recognise.
- **Package bodies are read in full** (the RFC said signatures only): the nested-subprogram reader
  already parses them, so withholding the statements would have been extra code for less.
- **Anonymous blocks are counted, not recovered**: one-off data fixes in migration scripts.

---

## Knowledge Captured
- **PostgreSQL block comments nest; Oracle's do not.** iDempiere's `AD_Column_Sync.sql` comments out
  code as `/** … ELSE … END IF; /**/`. A PL/pgSQL scanner reads `/**/` as a nested opener and loses
  the rest of the routine. Mask Oracle text with Oracle rules before handing it to `ekos-plpgsql`.
- **A `$` in an Oracle name hides statements.** `CREATE$JAVA$LOB$TABLE` reads as a PostgreSQL
  `$JAVA$` dollar quote to the shared splitter (`ekos_sql_dialect_sdk::lex`), which then swallows
  everything to the next `$JAVA$` — about 4,000 statements in one iDempiere release. The dialect
  quotes such names.
- **A PL/SQL `<<label>>` is a `GOTO` target**, not only a loop label; read as a loop it produced
  "LOOP body not found" on four iDempiere routines.
- **`ekos-plpgsql`'s lexer panicked on a multibyte character at a token start** (`&src[i..i + 2]` on a
  byte-order mark). Latent since RFC 0163; the first whole-tree iDempiere run with PL/SQL reading
  reached it and `recover` exited 101. Operators are now compared as bytes and a non-ASCII character
  is one token (`lex::tests::a_multibyte_character_is_one_token_and_never_a_panic`).
- iDempiere's PostgreSQL scripts change columns with `INSERT INTO t_alter_column values(table,
  column, type, null clause, default)` (406 statements): DDL written as DML.
- Measure before writing a number: the first probe walked both engines' directories and reported
  109,358 statements; the RFC was corrected to the Oracle-only 58,328.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0210-oracle-dialect-plsql-schema-history.md` | new RFC |
| `ekos/crates/plsql/` | new crate: `script.rs`, `parse.rs`, corpus test |
| `ekos/plugins/sql-dialect-oracle/` | new dialect crate + corpus test |
| `ekos/crates/plpgsql/src/{parse.rs,lib.rs,lex.rs}` | `Flavor`, PL/SQL statement forms; multibyte panic fixed |
| `ekos/crates/recovery/src/{plsql_analyzer.rs,schema_history.rs}` | new passes |
| `ekos/crates/recovery/src/{plpgsql_analyzer.rs,sql_analyzer.rs,sql_dialect_registry.rs,lib.rs}`, `Cargo.toml` | shared routine writer, `dialect` stamp, registry, exports |
| `ekos/crates/semantic/src/{schema_history.rs,conflicts.rs,triggers.rs,lib.rs}` | change links, per-dialect conflicts, zero-scale numeric normalised, PL/SQL triggers classified |
| `ekos/crates/kir/src/custom_kinds.rs` | `SchemaChangeSet`, `SchemaChange` |
| `ekos/crates/cli/src/{detect.rs,commands/recover.rs,commands/commit.rs}` | directory dialect rules, wiring, summaries |
| `ekos/Cargo.toml`, `Cargo.lock` | workspace members |

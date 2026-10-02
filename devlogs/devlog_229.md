# Devlog 229 — RFC 0163: `PlPgSqlAnalyzerPass` — stored procedures reach the ledger

**Date:** 2026-10-02
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The PL/pgSQL parser (devlog_220, hardened in devlog_228) had no consumer. `ekos recover` now runs a
`PlPgSqlAnalyzerPass` on every PostgreSQL `.sql` file. It writes one `Procedure` object per routine,
carrying a fidelity label the parser computed. It also writes one `ProcedureStatement` per statement
at every depth, each citing its own source text and line, joined by `Contains` edges that record
which branch each statement sits in.

Run end to end on LedgerSMB's `sql/` (259 files, mock LLM, no metered calls): **556 routines, 254
PL/pgSQL (251 complete, 3 partial), 1556 statements**. The new kinds add zero compile warnings. After
a second full `recover` → `commit` on unchanged input, **each of the 2,112 objects still has exactly
one ledger version**.

---

## What was built

| Component | Change |
|---|---|
| `plpgsql/src/source.rs` | `routines(src)`: every `CREATE [OR REPLACE] FUNCTION\|PROCEDURE` in a file, split on lexer tokens, **with its byte offset**; `line_of`. Moved here from the corpus test so the pass and the test share one splitter |
| `recovery/src/plpgsql_analyzer.rs` | `PlPgSqlAnalyzerPass` + pure `recover_routines(path, sql) -> (KirGraph, PlPgSqlStats)`; 8 tests |
| `kir::custom_kinds::REGISTRY` | `Procedure`, `ProcedureStatement`, both `structurally_keyed: true` |
| `cli/commands/recover.rs` | Pass registered per SQL file when `applies_to(dialect, text)`; summary line `PL/pgSQL routines: N (M plpgsql: C complete, P partial), S statements, U unrecovered` |
| RFC 0163 | Registry criterion ticked; *Amendment 2026-10-02 (b)*, the pass as built |
| README, capabilities page, CLAUDE.md | New "PL/pgSQL stored procedures" section; the "nothing consumes the parser" notes corrected |

### The objects

**`Procedure`**: `language`, `arguments`, `returns`, `fidelity` (`statements` / `partial` /
`signature`), `statements_recovered`, `statements_unrecovered`, `unrecovered_lines`,
`dynamic_sql_sites`, `eligible_for_reconstruction`, `declarations`, `source_path`, `line`, and the
byte span. Evidence is the routine header.

**`ProcedureStatement`**: named `routine#N` (N is the pre-order index). Carries `procedure`, `index`,
`parent_index`, `depth`, `order`, `branch`, `line` and span, plus the statement's own semantics:
`stmt` (`sql`, `if`, `loop`, …), `sql`/`into`, `conditions`/`has_else`, `handlers`, the loop form,
and so on.

---

## Decisions

- **Only `postgres`-dialect files, or files that name `plpgsql`.** The parser defaults to PL/pgSQL
  when there is no `LANGUAGE` clause. A T-SQL `CREATE PROCEDURE p AS BEGIN … END` would come out as a
  `Partial` routine whose "gaps" are really a different language. That is a false finding, and worse
  than none.
- **The file as written, not the preprocessed text.** The other two SQL passes get
  `dialect_parser.preprocess(&sql)`, which rewrites the file (RFC 0146). Spans and line numbers must
  cite the real file, so this pass gets the redacted original.
- **Properties derived from the IR's serde form**, minus spans and nested bodies. A new `ProcStmt`
  variant is carried without this pass changing, and there is no hand-written field list to drift.
- **Branch recorded on the child and on the edge.** Without it, a stored `IF` would say only "these
  statements are inside", not which arm. Control flow is what this IR exists to keep.
- **Compound evidence is the first line only.** A leaf's evidence is its full text. Citing a block's
  whole body at every nesting level would store a deep routine once per level.
- **Redefinition in the same file: the last wins.** This is what `CREATE OR REPLACE` does. A
  `Procedure` keyed on (path, name, arguments) would otherwise appear twice with one id (`SEM002`).
- **No table or routine edges yet.** Linking needs the embedded SQL lowered (RFC 0164) and a
  whole-graph, unambiguous-name match (RFC 0075's pattern). A per-file pass has neither. Faking it
  with a regex over the statement text would produce exactly the kind of guessed lineage RFC 0075
  refuses to create.

---

## Verification

- `recover_routines` unit tests check: overloads stay distinct; fidelity per routine, including
  `unrecovered_lines`; a `LANGUAGE sql` routine gets no statements; every statement's evidence equals
  its span text at the right line; IF/ELSE and handler branches are recorded and match on the edge;
  ids are deterministic and depend on the path; last definition wins; an unlexable file is reported,
  not crashed on; the dialect gating works; one artifact per pass.
- Registry guard mutation-checked: with the two rows removed, `every_pipeline_custom_kind_is_registered`
  fails and names both kinds.
- **Real pipeline** on a scratch copy of LedgerSMB `sql/`. The LLM key variable pointed at a name
  that does not exist, and the real key variables were unset, so all LLM work went to the mock.
  - `ekos ekl "FIND Object WHERE kind = 'Procedure'" COUNT` → 556; for `ProcedureStatement`, 1556.
  - The 3 `SEM002 duplicate object id` warnings are all `Table`s from `SqlAnalyzerPass`, the same table
    in the schema and in a migration. They predate this work.
  - `trigger_parts_short` (pre-8.0 quoted body) is stored at `statements`; its `NOTIFY parts_short`
    statement is at line 74 in branch `then:0`, which matches the file.
  - `ekos ledger audit` on all 2,112 objects after the second commit: 2,112 × "1 version". The
    re-run's 275 rewrites were all rollup `Contains` edges and the duplicate tables.
- `cargo test` for plpgsql, recovery, kir, identity and the `ekos` CLI: all green. `clippy -D warnings`
  and `fmt --check` are clean.

---

## Knowledge Captured

- **A re-commit on unchanged input is not a no-op today, and that is not this pass.** On LedgerSMB
  a second `commit` wrote 6 objects, 2 relationships and 275 versions overall. The rollup step
  re-writes its `Contains` edges, and the three `Table`s that share an id across schema and migration
  files alternate between versions. Neither comes from the new kinds; per-object `ekos ledger audit`
  is what proves it. `ekos diff` truncates its listing ("… and 221 more"), so it cannot.
- **EKL filters on `kind` and `name` only, not on properties.** `fidelity = 'partial'` silently
  returns 0 rows rather than an error. To find partial routines, list `Procedure`s and read the
  property, or use the `recover` summary line.
- **EKL's count form is `FIND … COUNT`, not `COUNT …`.**
- **Never run a pipeline in the LedgerSMB checkout itself.** Its `ekos.toml` points at a metered
  provider (OpenCode Zen). A scratch workspace with `api-key-env` set to a nonexistent variable, plus
  `env -u` of the real key variables, is the safe way to get a structural-only run. Without a
  configured provider the fallback tries Anthropic whenever `ANTHROPIC_API_KEY` is set.
- **Seven downstream registries still don't know these kinds**: docs-gen entity pages and API
  grouping, `llm_description`, `doc_links` (RFC 0147's list). The objects are queryable but get no
  generated pages yet. This is deliberate and recorded in TODO.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/plpgsql/src/source.rs` | New: `routines`, `RoutineSource`, `line_of`, 1 test |
| `ekos/crates/plpgsql/src/lib.rs` | Export `source` |
| `ekos/crates/plpgsql/tests/ledgersmb_corpus.rs` | Uses the crate's `routines` |
| `ekos/crates/recovery/src/plpgsql_analyzer.rs` | New pass, 8 tests |
| `ekos/crates/recovery/src/lib.rs`, `Cargo.toml` | Module + export; `ekos-plpgsql` dependency |
| `ekos/crates/kir/src/custom_kinds.rs` | `Procedure`, `ProcedureStatement` rows |
| `ekos/crates/cli/src/commands/recover.rs` | Pass wiring + summary line |
| `ekos/docs/rfcs/0163-plpgsql-procedural-ir.md` | Criterion ticked; Amendment (b) |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md` | Capability documented |
| `TODO.md` | Pass ticked; linking + downstream registries added |

# Devlog 231 — RFC 0169: views become ledger objects

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

EKOS had no object for a view. A view's *logic* existed, as Transformation IR nodes whose `Sink`
carried the view's name as a string, but the view itself didn't. Every reference to one dead-ended,
and on LedgerSMB that made views the largest class of names RFC 0163's linker could not resolve
(devlog_230).

RFC 0169 (written and accepted this session) adds `Custom("View")`. Every `CREATE [OR REPLACE]
[MATERIALIZED] VIEW` in an observed `.sql` file becomes one, with its columns, flags, exact source
and line, and what its query reads and calls. The existing linkers treat views as relations: a view
`DependsOn` what it reads, and routines and transformations that read a view link to it.

LedgerSMB `sql/` end to end (mock LLM): **17 views, all parsed**. **24 view references that devlog_230
could not resolve now link.** The one left is `cash_impact`, which is defined in two files and is
correctly ambiguous. Procedure links went from 1,852 to 1,931 and data-lineage links from 466 to
483. There are 0 identity conflicts, and a second `commit` writes nothing new.

---

## What was built

| Component | Change |
|---|---|
| RFC 0169 | New, accepted: design, keying decision, scope, alternatives |
| `plpgsql/src/source.rs` | `statements()`: every top-level statement with its offset; `routines()` is now a filter over it; `head_words` |
| `recovery/src/plpgsql_footprint.rs` | `statement_footprint_in(dialect, …)` (any dialect) and `query_footprint(&Query)` (a view's body without its own name) |
| `recovery/src/view_analyzer.rs` | New `ViewAnalyzerPass` + pure `recover_views`; 5 tests |
| `kir::custom_kinds::REGISTRY` | `View`, `structurally_keyed: true` |
| `semantic::procedure_lineage` | Views are link targets (relation index) and link sources (`DependsOn`, `Calls` from their own footprint); +1 test |
| `semantic::data_lineage` | Views are link targets for `TransformNode` `Source`/`Sink`; +1 test |
| `identity` | `is_expected_view_routine_pair`; 2 tests |
| `cli/commands/recover.rs` | Pass wiring + `Views: N (P parsed, U unparsed — still recorded)` |

---

## Decisions

- **Keyed by (file, name), like `Procedure`, not by name like `Table`.** A view is a definition in a
  file. Name-only keys are what make three LedgerSMB tables flip between versions on every commit
  (duplicate ids across files, devlog_229/230). The cost is that a view defined in two files is two
  objects and a bare reference to it is ambiguous. In LedgerSMB that is 2 of 15 names
  (`cash_impact`, `transactions_reversal`), and only `cash_impact` is referenced.
- **Its own pass, not `SqlTransformAnalyzerPass`.** The transform pass gets the dialect-preprocessed
  file, so it has no exact lines. It also drops into a per-fragment fallback when *any* statement in
  the file fails. A view must not depend on its neighbours parsing.
- **One definition at a time, never dropped.** A view whose body `sqlparser` can't parse is still
  emitted: named from the tokens after `VIEW`, flags read from its words, `footprint: unparsed`, and
  the parser's reason kept.
- **`Custom("View")`, not a new `ObjectKind` variant.** This is the documented extension path, and
  the identity guard enforces it. Both serialize to `"View"`, so promoting it later stays
  wire-compatible.
- **No new linker.** Both existing linkers needed only to admit `View` into their relation index,
  and `procedure_lineage` links a view from its own footprint exactly as it links a routine.

---

## Found on real data

- **An identity conflict, the only one in the run.** LedgerSMB has a view `employee_search` and two
  routines named `employee_search` (the functions that query it). SQL keeps relations and routines in
  separate namespaces, so this is legal and deliberate, but `ekos resolve` refuses to continue on any
  conflict and exited 1. I followed RFC 0093's precedent and **narrowed** the detector for exactly
  `{View, Procedure}`. `View` beside `Table` still conflicts, since views and tables share the
  relation namespace, and `Table` beside `Procedure` is not excluded until it is observed. Both
  outcomes are pinned by tests.
- **A token-fallback bug, found by the analyzer's own test.** The fallback name reader took every
  word in a row, so `create view weird as select …` came out as `weirdasselect`. A dotted name now
  alternates word, `.`, word.

---

## Knowledge Captured

- **A new object kind can create the first conflict a workspace has ever had.** `View` was correct,
  and still made `ekos resolve` fail on LedgerSMB because of a legal name sharing across SQL
  namespaces. Run the full pipeline, `resolve` included, on a real schema before calling a new kind
  done. Unit tests can't see this.
- **`statements()` in `ekos-plpgsql` is a general SQL statement splitter.** It is lexer-based, keeps
  each statement's offset, and handles dollar quotes. Use it for any per-statement analysis of a
  `.sql` file rather than splitting on `;`.
- **`query_footprint(&Query)`, not `statement_footprint` of the `CREATE VIEW`.** The visitor
  reports the view's own name as a relation of the enclosing statement, so footprinting the whole
  statement would make every view read itself.

---

## Verification

- 11 new tests (5 view analyzer, 2 linker, 2 identity, plus the registry guard covering `View`
  through the existing CI test). LedgerSMB corpora are unchanged: parser 212/212, embedded SQL
  1137/1141.
- Real pipeline on a fresh scratch copy of LedgerSMB `sql/` (no LLM key reachable): 17 views, 0
  conflicts, 1,931 procedure links, 483 data-lineage links; second run writes 0 procedure links.
- `cargo test` for plpgsql, recovery, semantic, kir, identity and the `ekos` CLI: green. `clippy -D
  warnings` and `fmt --check` are clean. No dependency changes, so no lockfile changes.

---

## Still open

- Downstream registries for `View`: docs-gen Data Stores and entity pages, `llm_description`. Not
  CI-enforced, as for `Procedure`.
- Views from live catalogs (RFC 0157) and dbt models materialized as views stay out of scope.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0169-view-objects.md` | New RFC, accepted; implementation note |
| `ekos/crates/plpgsql/src/source.rs`, `lib.rs` | `statements`, `SqlStatement`, `head_words` |
| `ekos/crates/recovery/src/view_analyzer.rs` | New pass, 5 tests |
| `ekos/crates/recovery/src/plpgsql_footprint.rs` | Dialect-generic + query footprint |
| `ekos/crates/recovery/src/lib.rs` | Module + export |
| `ekos/crates/kir/src/custom_kinds.rs` | `View` row |
| `ekos/crates/semantic/src/procedure_lineage.rs`, `data_lineage.rs` | Views as link targets/sources; 2 tests |
| `ekos/crates/identity/src/lib.rs` | `is_expected_view_routine_pair`; 2 tests |
| `ekos/crates/cli/src/commands/recover.rs` | Wiring + summary |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented |

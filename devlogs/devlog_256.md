# Devlog 256 — Sonar findings: closures, 57 functions under the complexity limit, a local checker

**Date:** 2026-10-12
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

SonarCloud listed 59 open issues, all code smells, from an analysis of 2026-09-29. The code has moved since, so the list was stale: measured on today's tree, 58 Rust functions were over the cognitive-complexity limit, not 35. 57 are now under it, with no behaviour change. One is left: `business_semantics::synthesize`.

No SonarCloud token is available in this environment, so nothing here was confirmed by a new scan. A small checker that reproduces Sonar's number was written and is in the repository.

---

## What SonarCloud listed

| Rule | Count | What was done |
|---|---|---|
| `rust:S3776` cognitive complexity over 30 | 35 | 57 of the 58 over the limit today refactored |
| `rust:S1612` closure that only calls a method | 19 | All 19, and every other site clippy's `redundant_closure_for_method_calls` finds |
| `python:S7503` `async` without async features | 4 | Scoped out in `sonar-project.properties`; each is async on purpose |
| `typescript:S3776` complexity over 15 | 1 | `Graph.tsx` split into components |

---

## A checker that matches Sonar

### Problem / motivation

Without a scan there is no way to know whether a refactor brought a function under the limit, or which functions written since the last scan are over it.

### What was built

`scripts/cogc/` is a 150-line `syn` visitor. `scripts/cognitive-complexity.sh [min]` lists every function at or above `min` (default 31).

### Implementation details worth remembering

- **The rules that reproduce Sonar's number:** +1 and +nesting for `if`, `match`, `for`, `while`, `loop`; +1 for `else` and `else if`; +1 per run of the same boolean operator; +1 for a labelled `break` or `continue`; +1 for direct recursion. A closure or nested `fn` raises nesting. `?`, `let … else` and macro bodies count nothing.
- **Calibration:** on revision `c0723c4`, 31 of the 35 functions SonarCloud flagged got exactly its score. The other four were low by 1, 1, 2 and 8. Nothing was over 30 that Sonar had not flagged.
- **A score within a few points of 30 is not safe.** The largest miss was on the largest function.

---

## The refactors

### What was built

| Shape | Functions | What replaced it |
|---|---|---|
| Quote-aware scanners repeated per clause | 8 in the ClickHouse and PostgreSQL dialect plugins | One driver per plugin; each clause is a function from position to "continue from here" |
| One long `match` with a body per arm | CLI dispatcher, MCP tool dispatcher, plan execution | One function per subcommand group, per tool, per plan step |
| A lexer loop with every token inline | PL/pgSQL `lex`, `mask_comments`, Perl `sql_strings`, `neutralise` | Token helpers; a scanner struct for the Perl one |
| A pass with all its phases inline | `build`, `commit`, `recover`, dbt, crate topology, local docs, Elixir, Perl | A function per phase |
| A page renderer with every section inline | `render_architecture`, object pages, data architecture | A function per section |
| A report printed by forty `if`s | `recover` | A summary type; one helper for the repeated count line |

### Decisions

- **Mechanical extraction, not redesign.** A block moves into a function with the same statements in the same order. Where two functions held the same loop, they now share it.
- **The Python findings were not "fixed".** The four functions are async so FastAPI runs them on the event loop, or because they are part of an awaited interface. Each already said so in a comment, and the inline `# NOSONAR(python:S7503)` was not honoured. The exception is in `sonar-project.properties`, for those four files only.
- **`synthesize` was left.** It is 1,100 lines of interleaved state with borrowed maps, and its output order is tested. It needs a designed decomposition.

---

## Follow-up: the quality gate failed on Reliability

The scan of this work (revision `165c818`) confirmed the complexity numbers: open issues 59 → 24, one Rust function over the limit. The gate still failed, on one condition: Reliability on new code was C and must be A. Six bugs, all in code added since the previous scan and none in the refactors.

| Finding | Where | Fix |
|---|---|---|
| Blocking file I/O in an `async fn` (2, major) | `commands/semantics.rs::commit_step` | `tokio::fs::create_dir_all` / `tokio::fs::write` |
| Blocking file write in an `async` function (1, major) | `web/api/app/semantics_write.py::import_linkml` | The temp-file write runs in `asyncio.to_thread` |
| A list row with a click handler and no keyboard access (3, minor) | `Conflicts.tsx`, `Semantics.tsx` (two lists) | The row's name is a `<button>`; its `::after` covers the row |

The Python one mattered most: an edited schema of up to 8 MiB was written on the console's event loop. `ruff.toml` allows small inline file operations on purpose; this one is not small.

The rows could not be opened from the keyboard at all. A click anywhere on a row still selects it. The checkbox in a row sits above the button's overlay, so it no longer needs `stopPropagation`.

Thirteen smells tagged with a low reliability impact went with them: label text followed directly by its field (8, the text is now in a `<span>`), `replace(/_/g, …)` (2, now `replaceAll`), and the three handlers on `<li>`.

---

## Knowledge Captured

- **A gate can fail on the code beside the work.** The refactor commits were the first scan since the console's Semantics and Conflicts tabs landed, so their findings counted as new code. Read the gate's conditions, not only the issue count.
- **Any bug on new code keeps Reliability below A**, whatever its severity. Minor ones alone give B.

- **A scan's issue list ages with the code.** The public issues API works without a token and reports the analysed revision; compare it with `HEAD` before treating the list as the work.
- **A blind rename during extraction can shadow.** Moving a block into a function whose parameter was renamed `path` put it under a local `let path = …`. A test on section names caught it. Name the parameter differently from every local in the block.
- **Extraction drops side effects that sit between two statements you kept.** `declare_package` lost `package_count += 1` because the counter lived inside the `if seen.insert(..)` that moved. Read the cut block for every write to something outside it.
- **`cargo clippy --fix` stops at the first crate whose fix does not compile.** One bad suggestion (`rustpython_parser::rustpython_ast::Identifier::as_str`, a path that is not nameable) hid 45 others downstream. Fix that one by hand and rerun.
- **Sonar's S1612 flags closures clippy's lint does not** (`.and_then(|s| s.to_str())` on an `OsStr`). A pattern rewrite with a compile-and-revert loop covered them.
- **A method named `from_*` that takes `&mut self` trips `clippy::wrong_self_convention`.**
- **`web/ui/node_modules/.vite` is owned by root here.** `vitest` reports `EACCES` after the tests pass; the 78 tests still ran.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/plugins/sql-dialect-{clickhouse,postgres}/src/lib.rs`, `ekos/crates/sql-dialect-sdk/src/lex.rs` | Shared quote-aware drivers |
| `ekos/crates/cli/src/app.rs`, `commands/{mcp,build,commit,recover,docs,ask,export,import,migrate,migrate_logic,semantics}.rs` | Dispatchers and phases split |
| `ekos/crates/recovery/src/{perl_sql,perl_analyzer,elixir_analyzer,local_docs_analyzer,crate_topology_analyzer,dbt_analyzer,sql_analyzer,view_analyzer,trigger_analyzer}.rs` | Analyzers split at their phases |
| `ekos/crates/plpgsql/src/{lex,parse}.rs` | Token and keyword helpers |
| `ekos/crates/docs-gen/src/lib.rs` | A function per page section |
| `ekos/crates/semantic/src/{lib,triggers,procedure_lineage}.rs`, `ekos/crates/identity/src/lib.rs` | Helpers |
| `ekos/crates/runtime/src/{lib,reason,retrieval}.rs`, `ekos/crates/ledger/src/search.rs`, `ekos/crates/ekl/src/parser.rs`, `ekos/crates/migrate-logic/src/{lower,v5,invention}.rs` | Helpers |
| 56 files | Method references for single-method closures |
| `web/ui/src/pages/Graph.tsx` | Toolbar, sidebar and status components |
| `ekos/crates/cli/src/commands/semantics.rs` | Manifest written with `tokio::fs` |
| `web/api/app/semantics_write.py` | Temp-file write off the event loop |
| `web/ui/src/pages/{Conflicts,Semantics,SemanticsItemPanel,Linkml}.tsx`, `web/ui/src/index.css`, two test files | Rows selectable from the keyboard; label text in spans; `replaceAll` |
| `sonar-project.properties` | `python:S7503` exception for four files |
| `scripts/cogc/`, `scripts/cognitive-complexity.sh` | New: local checker |
| `CLAUDE.md`, `TODO.md` | Devlog number; open items |

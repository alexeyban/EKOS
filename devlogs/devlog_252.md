# Devlog 252 — Silent wrong results, per-statement DDL fallback, and RFC 0164 Phase 1

**Date:** 2026-10-11
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

A review of every open item in `TODO.md` and every RFC status line found three kinds of gap: lists that no longer matched the code, places where a command succeeded with a wrong or short answer, and the unbuilt part of EKOS Migrate. This session closed the first two and built the first phase of the third.

- **RFC 0175:** four silent wrong results fixed (EKL property filters, inferred-key attribution, `DQ.UNIQ.001` on foreign keys, treasury paging).
- **RFC 0176:** structural DDL parsing degrades per statement. LedgerSMB went from 184 to 197 tables and 218 to 229 foreign keys.
- **RFC 0164 Phase 1:** a new crate, `ekos-migrate-logic`, lowers PostgreSQL views to ClickHouse by rule, checks candidates for invention, gates routines by fidelity and proposes a redesign per trigger class.

---

## List cleanup

| Item | What was true |
|---|---|
| RFC 0154–0162 status "Draft" | All shipped and verified live in September. Each now states what is implemented and which `TODO.md` section holds its open items. |
| "Re-run `ekos eval run`" open | Runs on the clean corpus exist from 2026-09-09 and 09-14 (local), then 70 → 79 → 87/101 on Zen. |
| "MCP tools ignore unknown arguments" open | Fixed in devlog_225; the same item was ticked elsewhere in the file. |
| "A table defined in two files flips between versions" open | Deterministic since devlog_242, reported as `ConflictingEvidence` since RFC 0172. |
| Six binary-decompiler items open | Closed by RFC 0150 in the private repo. |
| "Env-var-only connector secrets" open | Half done: Migrate aliases have no password field. A literal `password = "…"` is still ignored instead of refused, so the item stays open with that note. |

Open items: 139 → 128, with 12 new ones written down.

---

## RFC 0175 — four places that returned a wrong answer without saying so

### Problem / motivation

Each of these exits 0 and prints something plausible.

### What was built

| Area | Before | After |
|---|---|---|
| EKL | `WHERE status = 'confirmed'` returned 0 rows: only five built-in columns existed | Properties are read for `WHERE`, `RETURN`, `ORDER BY`, `GROUP BY`; `a.b` is a nested path; a field no candidate has is `EklError::UnknownField` |
| Inferred keys | A join key pair was assigned to the left and right operand by written order | `Source` nodes keep their alias; each side goes to the table its qualifier names, or the observation is dropped |
| `DQ.UNIQ.001` | Blocked on `acc_trans.trans_id` | Skipped for a column on the referencing side of a declared foreign key |
| Treasury | One request per explorer action, cut at 10,000 rows | Paged by block window; a history that cannot be read whole is an error |

### Implementation details worth remembering

- **EKL copies only the properties a query names** into its rows (`property_fields`). Copying every property of every object would make a full scan of a large ledger allocate the whole ledger.
- **"Unknown field" has no schema to check against**, because properties are open. The rule is: not built-in, and no candidate row has it. With zero candidates the result is empty, not an error.
- **Join keys:** `TransformNode::Source` gained `alias`, skipped in serde when absent, so old artifacts read. The scope is per graph. An alias that names two tables in one graph is ambiguous and attributes nothing. `source_object_name` now stops at a `Join` node, because following one of its two inputs picks a table by accident.
- **The pager starts the next request at the last block seen, not after it.** The row cap can cut a block in half. Rows of that block already returned are skipped by count, not by set, so two identical transfers in one transaction both survive.

### Decisions

- **A dropped observation over a wrong one.** A missing inferred-key candidate is a gap. A wrong one is measured, costs a query, and looks derived.
- **Inferred keys do not suppress `DQ.UNIQ.001`.** They are candidates until measured.

---

## RFC 0176 — structural DDL parsing degrades per statement

### Problem / motivation

`parse_ddl_structural` made one `Parser::parse_sql` call per file. One unknown statement discarded every table in it, behind `SQL001: no tables found`. It has cost a whole schema twice (devlog_177, RFC 0146).

### What was built

| Component | Change |
|---|---|
| `parse_statement_by_statement` | After the whole-file parse and the missing-`;` repair both fail: split with the shared lexer, parse each statement alone, blank the failing ones, parse the blanked file whole |
| `DdlParseReport` | Statements seen, and line, first words and parser error for each one skipped |
| `SQL005` | "N of M statements were not parsed and skipped", naming a skipped `CREATE TABLE` as a lost table |
| `SQL001` | Names the dialect and the first error when the file states a `CREATE TABLE` and none was recovered |
| `ALTER TABLE … ADD … FOREIGN KEY` | Becomes a `ForeignKey` edge, same id scheme as an inline constraint |

### Implementation details worth remembering

- **Blank, do not concatenate.** Replacing a failing statement with spaces and keeping its line breaks leaves every later line number unchanged. Seed-row lines, comment lines and evidence spans need no adjustment, and nothing downstream changed.
- **`CREATE TABLE` is counted on code only**, through the lexer's statement spans, so a string that mentions it does not make `SQL001` claim a lost table.

### Measured on LedgerSMB

| | Before | After |
|---|---|---|
| `Table` objects | 184 | 197 |
| `ForeignKey` relationships | 218 | 229 |
| Files with `SQL001` | 251 | 236 |
| Files reporting skipped statements | (silent) | 24 |

What is still skipped is now visible: `REVOKE … ON FUNCTION`, `OVERRIDING SYSTEM VALUE`, `DROP AGGREGATE`, `EXCLUDE` constraints, psql `:'var'`.

---

## RFC 0164 Phase 1 — lowering by rule

### Problem / motivation

RFC 0164 was a draft with no code. Its hard prerequisite (RFC 0163's procedural IR) has been in place since devlog_233.

### What was built

| Piece | Where |
|---|---|
| View lowering and round trip | `ekos/crates/migrate-logic/src/lower.rs` |
| Anti-invention check | `invention.rs` |
| Fidelity gate | `gate.rs` |
| Trigger proposals | `triggers.rs` |
| `ekos migrate lower`, `ekos migrate check-candidate` | `ekos/crates/cli/src/commands/migrate_logic.rs` |
| Live ClickHouse suite | `ekos/crates/migrate-logic/tests/clickhouse_semantics.{rs,sql,expected}` |

### Implementation details worth remembering

- **The walker is a whitelist.** Every `match` over the parsed statement ends in an arm that records a blocker. A new SQL feature costs a lowering; it cannot pass through with a changed meaning. Blockers accumulate, so a view reports all its reasons at once.
- **One walker, two readings.** `Side::Postgres` applies the rewrites. `Side::ClickHouse` accepts only what the module itself emits. The round trip is: emit, parse with the ClickHouse grammar, walk again as ClickHouse, compare bytes. A source spelling (`sum`, a non-Nullable cast) is refused on the ClickHouse side, so text this module did not write cannot pass as a read-back.
- **Settings belong to a `SELECT` block.** `SETTINGS` after a `UNION` binds to the last block only, so a member that needs any is parenthesized with its own clause. A derived table passes its needs up to the block that contains it.
- **Every lowering is run through the anti-invention check** against its own source before it is shown.
- **Definitions are re-read from source files by recorded span**, so the command applies RFC 0043's exclusion and redaction itself. The spans were measured on redacted text. A span that no longer starts with `CREATE` and names the object is reported as "re-run `ekos recover`", never lowered.

### Measured on LedgerSMB

| | |
|---|---|
| Views | 17: 7 lowered, 10 blocked |
| Blockers | 7 function without a rule, 7 query shape, 4 cast type, 3 `/`, 2 expression |
| Triggers | 42: 34 proposals, 8 left to a person |
| Routines | 556: 233 eligible, 21 ineligible, 302 not PL/pgSQL |

### Decisions

- **Block `/`.** PostgreSQL divides two integers as integers and ClickHouse as floats. The operand types are in the catalog, not in the view text.
- **Block `avg`.** Exact numeric in PostgreSQL, float in ClickHouse.
- **Block every materialized view.** A ClickHouse materialized view is an insert trigger. The refreshable form needs a schedule and a target table, which is a design decision.
- **Print, do not persist.** `MigrationProposal` objects and their R3 approval are the next step.
- **Not started:** reconstruction by a model, and V5 differential execution.

---

## Evals

`ekos eval run --agent ollama` under this repo's cloud `[llm]` config asked the local Ollama for `deepseek-v4-flash` and failed every scenario. A switched provider now drops the configured model and base URL (`with_agent`). The local model here is `llama3`, set through `OLLAMA_MODEL`.

A full local run was started for a current baseline. The self-workspace ledger is from 2026-09-23, so the run measures today's retrieval and answer code against September's knowledge. A run comparable with the published 87/101 needs a rebuild and the Zen model, which is metered.

---

## Knowledge Captured

- **A round trip through the target's parser finds real defects cheaply.** On its first run it caught an escaped backtick the ClickHouse grammar does not read (a doubled backtick is what both ClickHouse and the parser accept) and type names the parser prints in upper case.
- **The parser agreeing is not the engine agreeing.** Eight assumptions behind the rewrites were run on ClickHouse 24.8 with `clickhouse-local` in Docker, no server needed. All held, including the two that are easy to doubt: a `SELECT` alias shadows a column in `WHERE` by default (`SELECT id + 100 AS id … WHERE id = 1` returns no row), and `sum` over no rows of a non-Nullable column is `0`.
- **`sum` over a Nullable column already returns `NULL` on no rows.** The `-OrNull` form matters for non-Nullable columns. A probe on a Nullable column would have "shown" the rewrite was unnecessary.
- **`groupArray` skips `NULL`.** A probe for null ordering built on it shows nothing; wrap the value in `ifNull(toString(x), 'null')`.
- **`--agent` style overrides must drop settings that belong to the provider they replace.** A model name and a base URL are provider-specific.
- **`ekos eval run` prints `Status: PASS` when every scenario errored.** Written down as an open item.
- **Three TODO items were open and already fixed.** The file is long enough that a fix gets ticked in one place and stays open in another. Search the whole file for the symptom before starting on an item.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ekl/src/{interpreter,parser}.rs` | Property fields, dotted paths, `UnknownField`, 7 tests |
| `ekos/crates/semantic/src/transform_ir.rs` | `Source.alias` |
| `ekos/crates/recovery/src/sql_transform_analyzer.rs` | Records the alias; pass version `v3` |
| `ekos/crates/recovery/src/sql_analyzer.rs` | Per-statement fallback, `DdlParseReport`, `SQL005`, `SQL001` message, `ALTER TABLE` foreign keys, pass version `v4`, 7 tests |
| `ekos/crates/recovery/src/{pentaho,python}_analyzer.rs`, `ekos/crates/cli/src/commands/mcp.rs` | `alias: None` at `Source` construction sites |
| `ekos/crates/cli/src/commands/migrate.rs` | `JoinScope`, per-side key attribution, `constraint_columns`, 2 tests |
| `ekos/crates/migrate-dq/src/{model,catalog}.rs`, `tests/fixtures.rs` | `references_another_table`, rule and test |
| `ekos/plugins/treasury/src/lib.rs` | `collect_pages`, 3 tests |
| `ekos/crates/migrate-logic/` | New crate: `lower`, `invention`, `gate`, `triggers`, 24 unit tests, 1 live test |
| `ekos/crates/cli/src/commands/migrate_logic.rs`, `app.rs`, `mod.rs` | `ekos migrate lower`, `ekos migrate check-candidate`, 3 tests |
| `ekos/crates/cli/src/commands/eval.rs` | `with_agent`, 1 test |
| `ekos/docs/rfcs/0175-*.md`, `0176-*.md` | New |
| `ekos/docs/rfcs/0164-*.md`, `0154`–`0162` | Phase 1 section; status lines corrected |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented; lists cleaned |

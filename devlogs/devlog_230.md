# Devlog 230 — RFC 0163: routines linked to the tables they touch, and four bugs the SQL parser found in the IR

**Date:** 2026-10-02
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Stored procedures were in the ledger (devlog_229), but disconnected: nothing said which tables a
routine touched. Now every `ProcedureStatement` and `Procedure` records what its embedded SQL
**reads**, **writes** and **calls**, taken from a real `sqlparser` AST walk. `ekos commit` turns
those names into edges: `ReadsFrom`/`WritesTo` per statement, `DependsOn` routine → table, and
`Calls` routine → routine. Impact analysis on a table now finds the routines that touch it. On
LedgerSMB, 50 routines depend on `acc_trans`, and the two spot-checked against the source are real.

LedgerSMB `sql/` end to end (mock LLM): **1,852 edges** (409 reads, 308 writes, 989 depends-on,
146 calls). Only 4 names were too ambiguous to link, and every unresolved name has a reason. A second
`commit` on unchanged input writes **0**.

Parsing the IR's *own text* as SQL was also the strongest check the parser has had. It found **four
more bugs that corrupted IR text under a `Statements` label**.

---

## What was built

| Component | Change |
|---|---|
| `recovery/src/plpgsql_footprint.rs` | New. `statement_footprint` / `expression_footprint`: parse with `PostgreSqlDialect`, walk with `sqlparser`'s `Visitor`. Writes = DML targets; reads = other relations minus CTE names; calls = functions, table functions included. Unparseable → `footprint: unparsed` + the parser's error. 7 tests |
| `recovery/src/plpgsql_analyzer.rs` | Per-statement footprint of the statement's *own* SQL/expressions (not its children's); routine = union + declaration defaults; `LANGUAGE sql` routines get their body's footprint. Properties `reads`, `writes`, `calls`, `footprint`, `footprint_errors`. +1 test |
| `semantic/src/procedure_lineage.rs` | New. `link_procedures`: unique-name resolution (exact, else an unambiguous qualified↔unqualified match, recorded as `match`), one edge per id. 4 tests |
| `cli/commands/commit.rs` | `commit:procedure-lineage` step after RFC 0075's lineage; summary line |
| `plpgsql/src/parse.rs` | Four IR-text bugs fixed (below), 4 regression tests |
| `recovery/tests/plpgsql_ledgersmb.rs` | New corpus ratchet: 1137 of LedgerSMB's statements' SQL must parse |
| `ekos/Cargo.toml` | `sqlparser` `visitor` feature (adds `sqlparser_derive`, same upstream) |
| `benchmark/`, `tests/integration/` `Cargo.lock` | Refreshed — **already stale since cb20215** (see Knowledge) |

---

## Four parser bugs, all hidden behind `Statements`

Each produced IR text that *looked* plausible, and each was found only because that text then failed
to parse as SQL, or parsed to the wrong tables.

| # | Symptom | Cause | Fix |
|---|---|---|---|
| 1 | `INSERT INTO journal_line (…) SELECT id FROM account` stored as `INSERT FROM account`, with targets `journal_line (account_id) SELECT id` | `find_into` took the first `INTO` as PL/pgSQL's binding | In DML the binding `INTO` only follows `RETURNING`. The statement's main verb decides (for `WITH`, the first top-level verb), so `SELECT … FOR UPDATE` is not DML |
| 2 | `FOR r IN select … WHERE id = any(ids) LOOP` stored as `… any(ids` | `trim_matches('(' \| ')')` stripped parentheses from both ends unconditionally | `unwrap_parens`: remove one pair only if it wraps the whole query |
| 3 | `SELECT INTO a, b SUM(x), SUM(y) FROM …` gave the targets `["a", "b SUM(x)", "SUM(y)"]` | The target list ran to the next clause keyword | Targets are identifiers separated by commas and end after the last one |
| 4 | `EXECUTE $sql$…$sql$ INTO t\nUSING a, b` kept `USING a, b` in the expression | `" USING "` was matched with surrounding spaces, so a newline before it missed | `find_kw(rest, "USING")` |

Bug 1 is the worst kind. Every plain `INSERT INTO t SELECT …` in every routine had its table
recorded as a variable and its select list dropped, while the routine still reported full
recovery.

---

## The idempotency bug, and how it was found

The first real run looked right, but a second `commit` on unchanged input reported **6 new**
procedure links. To find them, every edge id was recomputed in Python from the compiled model,
using the same UUIDv5 seed as `KirRelationship::deterministic`. The recomputed set had exactly 1,855
ids, matching the linker's own count. `ekos ledger audit` on each one showed 1,849 edges at one version
and **3 ids at 4 versions, each listed twice**.

Cause: LedgerSMB's `pg_temp.f_insert_*` read both `defaults` and `[% slschema %].defaults` (a
template placeholder). Both resolve to the `defaults` table, so the linker emitted the same edge id
twice, once with `match: exact` and once with `match: unqualified`. The ledger, which versions by
content, flipped between them on every commit. Fix: one edge per id, `exact` preferred, and a test
named for it. Re-verified on real data: run 4 wrote the 3 corrected edges once (reads 412 → 409),
and run 5 wrote **0**.

---

## Decisions

- **Edge kinds.** RFC 0163 said "`Calls` to tables". But `callers` traverses `Calls` as the routine
  call graph, and `dependents`/`impact` traverse `DependsOn`/`Calls`/`ForeignKey`/`References`, not
  RFC 0075's `ReadsFrom`/`WritesTo`. Routine → table is therefore `DependsOn` (with `access`), so
  impact analysis finds it. Statement → table is `ReadsFrom`/`WritesTo`, the precise citation.
- **Unique names only.** The 4 ambiguous routine names are each defined in more than one file
  (module and migration). Which definition wins depends on load order, which EKOS doesn't know.
  Linking to all of them would create a call that may not exist, so none are linked.
- **No placeholder substitution.** RFC 0163 *Parsing* planned to replace variables with typed
  placeholders before parsing. It isn't needed: in expression positions the parser reads a variable
  as a column reference, which names no relation.
- **`RAISE` and `GET DIAGNOSTICS` are not parsed.** `RAISE SQLSTATE '22012'` and `USING ERRCODE`
  are not SQL expressions, and `GET DIAGNOSTICS` is not SQL. Attempting them would only add
  `unparsed` noise.
- **Dynamic `EXECUTE` contributes calls only.** The constructed statement's tables are unknown, and
  claiming them from the string-building expression would invent lineage. A test pins that
  `format('DELETE FROM %I', 'secret_target')` writes nothing.

---

## Knowledge Captured

- **Parse the IR's text back.** A computed fidelity label proves no statement was *unrecognised*,
  not that the recognised text is right. Running the stored text through an independent parser found
  four corruption bugs in one pass. Keep `plpgsql_ledgersmb.rs`'s floor as a ratchet.
- **Unresolved names on LedgerSMB, by cause:** built-in functions (`coalesce`, `currval`, `now`, …),
  system catalogs (`pg_roles`, `information_schema.*`), temp tables created inside routines, the
  `[% slschema %]` template, and **views** (`account_heading_tree`, `periods`, `cash_impact`, …).
  EKOS has no view object kind, so neither this linker nor RFC 0075's can reach a view. That is now
  a TODO item.
- **`ekos diff` truncates ("… and 225 more"), and its `--from` is easy to get wrong.** `stat`'s mtime
  of an output file is when it was last written, not when the step started, and subtracting a margin
  reached back into the previous commit. To find which entities churn, recompute the deterministic ids
  and run `ekos ledger audit` on each.
- **A path-dependent workspace's lockfile goes stale when an `ekos/` crate gains a dependency.**
  cb20215 added `ekos-plpgsql` to `recovery` and left `benchmark/` and `tests/integration/` locks
  stale, because `scripts/audit.sh` wasn't run. `[skip ci]` hides this. The fix is `cargo update
  --workspace` in each, which added exactly `ekos-plpgsql` and `sqlparser_derive`.
- **sqlparser 0.53 grammar gaps seen on real PostgreSQL:** `ON CONFLICT (expression)`, a
  data-modifying CTE (`WITH x AS (DELETE … RETURNING *)`), and `INSERT … OVERRIDING SYSTEM VALUE`.
  These are the 4 remaining unparsed statements.
- **`KirId` is not `Ord`.** Key ordered collections by the inner `Uuid` (`id.0`). Ordering is what
  keeps the edge output order deterministic.

---

## Verification

- 16 new tests (7 footprint, 1 analyzer, 4 linker, 4 parser). Mutation-checked: counting CTE names
  as tables, counting a table function as a table, not removing writes from reads, and dropping the
  `UPDATE` target are each caught by a named test.
- LedgerSMB corpora: 212/212 routines (parser); 1137/1141 statements' SQL parses (footprint).
- Real pipeline on a scratch copy of LedgerSMB `sql/` (no LLM key reachable): 1,852 edges; second
  commit writes 0; `acc_trans` has 50 `DependsOn` routines, 2 checked against the source.
- `cargo test` for plpgsql, recovery, semantic, kir, identity and the `ekos` CLI: green. `clippy -D
  warnings` and `fmt --check` are clean. `scripts/audit.sh` exits 0 on all 3 workspaces;
  `tests/integration` and `benchmark` compile.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/plpgsql_footprint.rs` | New: SQL footprint via AST visitor, 7 tests |
| `ekos/crates/recovery/src/plpgsql_analyzer.rs` | Footprints per statement/routine, `LANGUAGE sql` bodies, 1 test |
| `ekos/crates/recovery/src/lib.rs` | Module |
| `ekos/crates/recovery/tests/plpgsql_ledgersmb.rs` | New: embedded-SQL parse ratchet (1137) |
| `ekos/crates/semantic/src/procedure_lineage.rs`, `lib.rs` | New linker, 4 tests |
| `ekos/crates/cli/src/commands/commit.rs` | Linking step + summary |
| `ekos/crates/plpgsql/src/parse.rs`, `tests/parse.rs` | `find_into` (DML + target list), `unwrap_parens`, `USING` keyword search; 4 tests |
| `ekos/Cargo.toml`, `ekos/Cargo.lock`, `benchmark/Cargo.lock`, `tests/integration/Cargo.lock` | `sqlparser` `visitor`; locks refreshed |
| `ekos/docs/rfcs/0163-plpgsql-procedural-ir.md` | Amendment (c) |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented; View-kind gap added |

# Devlog 247 — A CASE that rewrites a code is not a label

**Date:** 2026-10-10
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0172's label-mismatch detector surfaced a real extractor bug. The `case_label` meaning source read `CASE WHEN (b.balance > 0 AND a.category = 'A') THEN 'L' … ELSE a.category END` (LedgerSMB `FinStatements.sql:690`, a balance-sheet sign flip) as "`'A'` means `L`". It did the same for `'L'` → `'A'`. The SQL predicate extractor now attaches a branch's literal as a label only when that literal is a function of the tested code alone. On LedgerSMB the conflicts drop from 5 to 3, and both false ones are gone. `account.category` keeps its labels from the column comment. The branches are still recorded as usage sites.

---

## The fix — `recovery/src/sql_predicates.rs::case_sites`

A `CASE` branch's literal result labels the code it tests only when both of these hold:

| Rule | Rejects |
|---|---|
| The branch condition (parentheses stripped) is **one test of one column**: `x = 'A'`, `x IN (…)`, `x = 'A' OR x = 'B'`, or a simple `CASE x WHEN …` | `WHEN balance > 0 AND category = 'A' THEN 'L'`: the result depends on the balance too |
| The `CASE` does **not rewrite the column into its own codes**: its `ELSE` is not that column, and no result literal is one of the codes tested against it | `… ELSE a.category END`, and `WHEN c = 'A' THEN 'L' WHEN c = 'L' THEN 'A'` |

The sites are still emitted with `label: None`, so usage counts, coded values and concept candidates are unchanged.

`PREDICATES_VERSION` is now `predicates/3`. The three passes with literal versions (`sql_transform_analyzer`, `dbt_analyzer`, `perl_analyzer`) were bumped with it, as their guard test requires, so no workspace serves cached predicates from before the fix.

## LedgerSMB, before → after

| | Before | After |
|---|---|---|
| Conflicts | 5 (1 duplicate definition, 4 labels) | 3 (1 duplicate definition, 2 labels) |
| `account.category = 'A'` meanings | `asset` (column comment), `L` (case_label) | `asset` (column comment) |
| `account.category = 'L'` meanings | `liability`, `A` | `liability` |

The two remaining label conflicts (`oe.oe_class_id` 1 and 2) are real. `Routes/ERP/API/Orders.pm:82` maps class 1 to `'customer'` and class 2 to `'vendor'`, naming the counterparty rather than the order type. That is a second facet of the same code, and a reviewer should dismiss it as both valid. It is not an extractor error.

In an existing workspace, the earlier reviews of `'A'`/`'L'` reopen as `needs_review` with `previous_review` kept, because their evidence changed. That is the RFC 0170 lifecycle working as intended.

## Decisions

- **Fix it at extraction, not in synthesis.** Synthesis could drop a `case_label` whose label is another code of the same column, but that would miss the conjunct case. It would also keep shipping a wrong `label` on the predicate site to every other consumer.
- **Under-read rather than misread.** A CASE that depends on another column could still carry a real meaning, for example `WHEN kind = 1 AND active THEN 'AP'`. Losing a 0.5-confidence label costs little. A wrong one costs a reviewer's time, or worse, a confirmed wrong enum in the exported LinkML.

---

## Knowledge Captured

- **A conflict detector doubles as an extractor test.** Both misreads were in the ledger since RFC 0170 Phase 3. They only became visible when two sources were compared on the same fact. Treat a new `label_mismatch` as a candidate extractor bug before assuming the sources disagree.
- **Re-running on an existing workspace reopens reviews whose evidence changed.** Any extractor fix that removes a meaning will do this. Mention it when shipping such a fix.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/sql_predicates.rs` | `case_sites` rules, `single_test`, `predicates/3`, 2 tests |
| `ekos/crates/recovery/src/{sql_transform_analyzer,dbt_analyzer,perl_analyzer}.rs` | pass versions `+predicates/3` |
| `ekos/docs/rfcs/0170-*.md`, `0172-*.md` | the `case_label` rule; the misread is now fixed |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `TODO.md` | LedgerSMB 5 → 3 conflicts, TODO item ticked |
| `demo/ledgersmb-linkml/README.md`, `output/` | `AccountCategory.A` from the column comment only; re-recorded run |

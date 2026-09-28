# Devlog 226 — EKOS Migrate on a real ERP: 14 defects the fixtures never reached

**Date:** 2026-09-28
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The LedgerSMB analytics demo (`../ledgersmb-analytics-demo`) ran EKOS Migrate (RFC 0154–0162)
end to end against a real ERP schema: 168 tables, 503 functions, 175 applied schema changes, and 18
months of synthetic but LedgerSMB-posted data (41,248 journal lines).

It broke in 14 places. Every one is fixed here, each with a regression test that fails without its
fix; the ones that could hide wrong data were also mutation-checked. The worst two were a
**self-approval bypass** in the human-only approval layer, and a **validator that could not see
cents** on unconstrained `numeric` money columns. That second one is the exact false green RFC 0155
exists to prevent.

End state: 30 of 30 migrated tables pass V1/V2/V3, and a planted one-cent change is caught at V3.

---

## The defects, in the order the run hit them

| # | Where | Defect | Fix |
|---|---|---|---|
| 1 | `pg-live` catalog | `discover` died on `unknown pg_constraint.contype "t"`: LedgerSMB has a constraint trigger | `t` is left to the trigger pass, which already records it; other unknown types still fail |
| 2 | `migrate load` | Chunking assumed an integer key: `COALESCE types text and integer` on `account` (text key); bounds were parsed with `unwrap_or(0)` | Integer keys are range-chunked; others load in one whole-table statement (stated as unbounded); an unparseable bound is an error |
| 3 | policy file | `[thresholds] blast-radius` (kebab, like the rest of the file) → "missing field `blast_radius`" | Both spellings accepted; the serialized form is unchanged (thresholds sit in recorded facts) |
| 4 | **approval** | **Self-approval bypass.** `--as cli:legion` was labelled `human:cli:legion`; `identity_of` stripped one prefix → `cli:legion` ≠ requester `legion` → approved | Strip every *EKOS* channel scheme (`human`/`cli`/`agent`/`console`), never an identity's own colons |
| 5 | approval evidence | Findings keyed by name; two rules on one column collided, and read order decided which won → "evidence changed" on raise-then-approve | Key includes the rule; hashes stable across reads |
| 6 | approval evidence | Unit matched by substring: `public.entity` pulled in `entity_employee`, `entity_note`, `entity_to_location` | Exact match: the table or `table.column` |
| 7 | type map | `Nullable(LowCardinality(String))`, which ClickHouse rejects at CREATE | `LowCardinality(Nullable(String))` |
| 8 | P1 profile | `TABLESAMPLE` unseeded → DDL differed run to run → approved artifact hash never matched the load; small tables often unmeasured | `REPEATABLE(seed)`; tables ≤ 128 pages read whole |
| 9 | **validator** | **Unconstrained `numeric` hashed at scale 0**: cents invisible to V1–V3, and PG rounds while CH truncates | Scale from the source declaration, else the *deployed* type (from `system.columns`), else skip and name the column |
| 10 | validator | NULL guard asymmetric: PG coalesced the *rendered* value, and bool renders NULL as 'f' | PG decides NULL on the column, as CH does |
| 11 | validator V2 | `length()` = characters (PG) vs bytes (CH): any non-ASCII text diverged | `octet_length` on PG |
| 12 | validator V2 | Empty table: CH min/max/sum return defaults, PG returns NULL | `aggregate_functions_null_for_empty = 1` |
| 13 | validator reader | Raw TSV prints SQL NULL as `\N`, the same bytes as the canonical NULL sentinel | `format_tsv_null_representation=` on reads |
| 14 | `migrate validate` | Printed "Validation failed" and **exited 0** | Non-zero exit on any failed tier |

---

## Found and not fixed (tracked in TODO.md)

- **`resolve` stops on cross-language homonyms**: Table `gl` vs PerlPackage `LedgerSMB::GL`, and 14
  more like it. `--force` only continues; nothing is merged. Needs an RFC 0093-style narrowing.
- **EKOS compiles only LedgerSMB's base DDL**, not the 175 `ALTER`s in `sql/changes/`, so its
  repository view is the pre-upgrade schema (316 drift findings, e.g. `acc_trans.amount` vs
  `amount_bc`). Real drift, but the "repo" side is stale by construction.
- **`DQ.UNIQ.001` blocks foreign-key columns** ("looks like a key, not unique") such as
  `acc_trans.trans_id`. That's noise on a real schema.
- **Join harvesting misattributes aliases**, e.g. `country.country_id → entity.id` where `country`
  has no such column. Measurement ("not measurable") stops a false claim, but the candidates are
  wrong.
- **No transform disposition**: `tax.validto = infinity` was correctly flagged BLOCK, but a load is
  `SELECT *`, so a human's "infinity → NULL" decision has nowhere to live in EKOS. The demo applies
  it in Python.
- **CLI `--as` is an unauthenticated claim**: the four-eyes rule holds against the honest mistake,
  not a determined user on one machine. Authenticated approval belongs to the console (OIDC).
- **`jsonb` has no canonical rule**, so those columns are skipped from V3, and named when they are.

---

## Knowledge Captured

- **A green validator is only as good as its canonical form.** Before fix 9, `acc_trans` "passed" V3
  while comparing amounts with the cents removed. The only reason it surfaced is that `parts` had a
  value ≥ .5 that the engines rounded differently. The planted one-cent control is now part of the
  demo; a green without a fired control proves nothing.
- **Anything an approval pins must be reproducible.** The DDL was a function of a random sample.
  Pinning a hash to a non-deterministic artifact makes approvals unfulfillable (the failure mode we
  saw), or worse, accidentally fulfillable.
- **Label schemes compose.** A prefix added at one layer (`human:`) on top of one typed by the user
  (`cli:`) defeated a check that assumed exactly one. Test the path with the real inputs (`--as
  cli:legion`), not only well-formed ones.
- **Fixtures are ASCII, non-empty, NOT NULL and integer-keyed; real schemas are none of those.**
  Defects 2, 7, 10, 11 and 12 all sit on one of those axes.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/pg-live/src/catalog.rs` | `constraint_kind`; `t` skipped; test |
| `ekos/crates/pg-live/src/profile.rs` | `SAMPLE_SEED`, `sample_clause`, `effective_percent`; 2 tests |
| `ekos/crates/migrate-target-clickhouse/src/{execute,typemap,lib}.rs` | `is_range_chunkable`, `whole_table_insert`, `nullable_of`; 3 tests |
| `ekos/crates/migrate-approval/src/{request,risk,policy,lifecycle}.rs` | channel-scheme `identity_of`; kebab aliases; 3 tests |
| `ekos/crates/migrate-validate/src/dialect.rs` + snapshot | NULL guard, byte length, empty-set setting; 2 tests |
| `ekos/crates/cli/src/commands/migrate.rs` | load planning, evidence keys and matching, deployed-scale rule, TSV nulls, exit code; 4 tests |

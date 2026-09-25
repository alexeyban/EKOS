# Devlog 212 — Column-level drift, and the two normalizations that make it usable

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos migrate discover` now compares the live catalog against the repository's own DDL down to the
column: tables present on one side only, columns present on one side only, and columns whose type
changed. Each is a `MigrationDrift` fact carrying a `structural` flag for RFC 0158 to gate on.

The comparison itself is ten lines. Everything else in `migrate/src/drift.rs` exists because a naive
version of those ten lines produces nothing but noise, for two independent reasons — and a drift
report full of false positives is worse than no drift report, because it teaches people to skip the
section.

---

## Why the naive comparison fails

### Names

The live catalog always qualifies a table: `ekos_drift.orders`. A repository's DDL sometimes does
and sometimes does not — `ekos_recovery`'s SQL analyzer keys a `Table` object on whatever
`CREATE TABLE` wrote (`sql_analyzer.rs:340`, `ct.name.to_string()`).

So a full-string comparison against a repository that writes `CREATE TABLE orders` reports **every
table twice**: once as live-only, once as repo-only. The previous devlog shipped exactly that
comparison, and it was only ever exercised against an empty repository side, where it could not be
wrong.

The analyzer's own comment-matching code already notes the same mismatch one layer down — table
matching there "is case-insensitive and ignores schema qualification, because DDL recovery keys
`Table` objects on the bare name while `COMMENT ON` frequently qualifies it". The same problem, and
now the same answer.

`TableRef::parse` folds case, strips quotes and splits on the last dot. A qualified repository name
must match schema *and* table; a bare one matches on table alone — and when that hits more than one
live table it is reported as **ambiguous** rather than resolved, because picking the first would
silently compare the wrong table's columns.

### Types

`format_type()` renders `character varying(50)`. `sqlparser` renders `VARCHAR(50)`. The same column,
two spellings, and a textual comparison marks every column in the database as drifted.

`normalize_type` reduces both to a common base plus parameters. Two details in it are load-bearing:

- **`serial` normalizes to `integer`, `bigserial` to `bigint`.** A `serial` column *is* an `integer`
  once deployed. Without this, every table with a surrogate key reports a drifted primary key.
- **An absent parameter never equals a stated one.** `numeric` and `numeric(12,2)` are genuinely
  different, and that difference is precisely what RFC 0159 needs in order to offer a
  `narrowing-safe` mapping rather than `Decimal(76, 20)`.

---

## The governing rule: cannot-compare is not a difference

`types_match` returns `Option<bool>`, and `None` — at least one rendering unrecognized — produces
**no finding**. A domain, an enum, a PostGIS type: none of those is evidence that anything changed.

The same rule applies a level up. A repository `Table` fact with no `columns` property contributes a
table with *no columns*, which reconciliation treats as "nothing to compare" rather than "every
column was deleted". And `discover` prints `Drift: not checked` when the ledger holds no compiled
`Table` objects at all, naming the commands that would produce them — reporting "0 drift" from an
empty comparison is a clean bill of health nobody earned, the same failure shape as a validation tier
reporting green with no controls.

---

## Verified end to end

Not against injected facts — through the real pipeline. A workspace with one `schema/orders.sql`,
run through `ekos build → recover → resolve → compile → commit`, then `ekos migrate discover` against
a live schema built to trip both normalizations:

| | repository DDL | live | expected |
|---|---|---|---|
| table name | `orders` (bare) | `ekos_drift.orders` | match, no finding |
| `id` | `BIGSERIAL` | `bigint` | match, no finding |
| `status` | `VARCHAR(50)` | `character varying(50)` | match, no finding |
| `total` | `NUMERIC(12,4)` | `numeric(12,2)` | **type differs** |
| `only_in_repo` | present | — | **repo-only** |
| `added_live` | — | present | **live-only** |
| `audit_log` | — | present | **table live-only** |

Four findings, all real, zero false positives — and each of the three "no finding" rows is a case the
naive comparison would have reported. Re-running produces the same four facts rather than eight:
drift ids are deterministic over `(project, object, kind)`.

---

## Knowledge Captured

**A comparison shipped against an empty other side has not been tested.** The name-only
reconciliation from devlog_211 passed its tests and ran clean in the end-to-end demo, because that
workspace had no compiled `Table` objects — so the repository side was always empty and the bug had
nothing to be wrong about. It took a fixture with facts on *both* sides to expose it. Any reconciler,
differ or matcher needs a test where both inputs are non-empty and deliberately disagree.

**When two systems describe the same thing, budget for the normalization, not the comparison.**
`drift.rs` is ~200 lines of which the actual set difference is a handful. The rest is name parsing
and a type alias table, and without them the feature is unusable rather than merely imperfect. This
is the same shape as RFC 0155's canonical form one layer up: the comparison is easy, agreeing on what
is being compared is the work.

**`Option<bool>` is the right return type for "are these the same?" across two vocabularies.** A
plain `bool` forces a guess in exactly the cases where a guess is most expensive. `None` reads as
"cannot tell", the caller reports nothing, and the gap is visible as an unrecognized type rather than
as a false finding someone has to dismiss.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate/src/drift.rs` | New — `TableRef`, `normalize_type`, `types_match`, `reconcile`, 14 tests |
| `ekos/crates/migrate/src/profile_facts.rs` | Drift writer takes the richer findings, records `structural`; name-only reconciliation removed |
| `ekos/crates/cli/src/commands/migrate.rs` | `columns_of`, `repo_table_shapes`, drift summary output, 2 tests |
| `ekos/crates/migrate/src/lib.rs` | Module wiring and re-exports |
| `TODO.md` | Phase 2 complete |

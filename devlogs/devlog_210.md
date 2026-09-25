# Devlog 210 — RFC 0157 profiling: a budget that refused nothing, and a "free" tier full of row data

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

Profiling completes the connector: P0 from catalog and planner statistics, P1 from a bounded
`TABLESAMPLE`, P2 behind an `EXPLAIN` cost check, plus PII classification and the replica-lag guard.
24 unit tests and 19 live tests; 141 test binaries green.

Two findings are the substance of this one, and both were bugs in controls that looked like they
worked:

- the P2 budget compared against the wrong number and would have let a billion-row scan through;
- the "free" P0 tier reads literal row values out of `pg_stats`, so costing nothing does not mean
  exposing nothing.

---

## The budget that refused nothing

`exact_row_count` asks the planner before scanning and refuses above a row budget. The first version
read `Plan Rows` off the root of the `EXPLAIN` output — and the root of `SELECT count(*)` is an
Aggregate node returning exactly **one** row.

So a sequential scan of a billion rows reported `estimated_rows = 1`, sailed under every budget, and
ran. The control was decoration, and it looked correct in review and in a unit test using a
hand-written plan.

It surfaced because a live test set the budget to `1.0` and asserted the scan was *refused* — the
negative assertion. `estimate_cost` now walks the whole plan tree and takes the maximum `Plan Rows`,
because the question a budget asks is how many rows the plan **touches**, not how many it returns.

The general shape: a guard needs a test that proves it *refuses*, not only one that proves it allows.
An allow-path test passes just as happily when the guard is inert.

## `pg_stats` is not free of row data

P0 is "free" in cost. It is not free in exposure. `pg_stats.most_common_vals` and
`histogram_bounds` are **literal values sampled from the table** — the planner keeps them so it can
estimate selectivity. A profiler that treats P0 as safe because it issues no scan copies real
customer data into an append-only ledger at zero cost, which is about the worst trade available.

So the bounds go through the same redaction and the same PII suppression as a sampled value, and two
narrower rules fell out of thinking about it:

- **A text column keeps no bounds at all**, PII or not. A min or a max of a text column *is* a value
  from that column. Numeric and date bounds are kept because RFC 0158's range rules need them — a
  date outside `Date32`, a numeric beyond precision 38 — and a number at the edge of a range is a
  fact about the range.
- **A P1 pattern match withdraws a P0 bound.** A column named `ref` that turns out to hold email
  addresses had its bounds recorded at P0 under a name heuristic that said nothing. When the value
  signal arrives at P1, the earlier bound is removed rather than left standing.

---

## Knowledge Captured

**`n_distinct` is negative when it means a fraction.** PostgreSQL stores `-1` for "every row is
distinct" and `-0.5` for "half the rows are". Read as a count — which is what a naive `as i64` does —
`-1` becomes "one distinct value", and RFC 0159 cheerfully offers a `LowCardinality(String)` mapping
for a primary key. `distinct_count` applies the convention against a known row count, and a test
asserts a unique column resolves to ~rowcount rather than to 1.

**`reltuples` is `-1` before the first `ANALYZE`, not 0.** Reporting -1 rows is worse than reporting
0 and both are wrong, so it is clamped and `row_count_is_exact` stays false — the honest signal being
the flag, not the number.

**`pg_stats.histogram_bounds` is `anyarray` and cannot be cast to `text[]`.** `::text` on the whole
array is the only route, giving `{a,b,c}`. Splitting that on commas is safe *only* because the caller
restricts bounds to numeric and date columns, whose rendered values contain no commas or quotes — it
would be wrong for text, which is the category whose bounds are withheld anyway. The two decisions
happen to protect each other, which is worth noticing rather than relying on.

**Luhn, not length, for card detection.** A 16-digit order id matches "looks like a card" on length
alone, and a false PII classification suppresses a column nobody needed suppressed. A test asserts a
16-digit non-Luhn sequence is *not* classified.

**No confidence threshold on suppression.** `Classification::suppresses_values()` returns true for
every classification regardless of confidence, and the doc comment says why: RFC 0060 already
established for identity resolution that no cutoff reliably separates real cases from false ones, and
here the failure directions are asymmetric — over-suppressing costs a little insight, under-
suppressing writes personal data into a ledger with no delete.

**A pattern outranks a disagreeing name.** A column called `phone` holding email addresses is
classified as email, because a name is a convention and a pattern is evidence. The method is recorded
either way, so a human reviewing it can see which signal fired.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/pg-live/src/profile.rs` | New — P0/P1/P2 tiers, `EXPLAIN` costing, budget refusal, `n_distinct` and `reltuples` conventions, bounds handling |
| `ekos/crates/pg-live/src/pii.rs` | New — name and value classification, Luhn, unconditional suppression |
| `ekos/crates/pg-live/src/session.rs` | `guard_replica_lag` |
| `ekos/crates/pg-live/src/lib.rs` | Module wiring, `ReplicaLagTooHigh` |
| `ekos/crates/pg-live/tests/live_profile.rs` | New — 9 live tests, incl. the PII-yields-nothing and budget-refuses tests |
| `TODO.md` | Phase 2 profiling/PII/lag ticked; drift reconciliation and fact persistence remain |

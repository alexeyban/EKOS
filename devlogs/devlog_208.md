# Devlog 208 — RFC 0156: tiers, bisect, and a test that was not testing its own claim

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0156's core is implemented in `ekos-migrate-validate`: the tier enum, V1–V4, the read seam that
makes the independent-oracle rule auditable, bisect from a failed bucket to exact keys, key masking,
divergence classification, and the control gate that stops a tier reporting green when a planted
defect slipped past it.

Verified live against PostgreSQL 16 and ClickHouse 24.8: a 5,000-row table loaded into both, defects
planted in the target one at a time, and each caught by the tier RFC 0156 says should catch it.
44 unit tests plus 7 live tests.

The most useful thing that happened was a test failing to test its own name. See *Knowledge
Captured*.

---

## What was built

| Module | Role |
|---|---|
| `tiers.rs` | `Tier` V0–V6, `TierOutcome`, `ControlResult`, `UnitPlan`, `run_v1`/`run_v2`/`run_v3` |
| `bisect.rs` | `BisectPolicy`, `bisect_bucket`, `run_v4`, `diff_key_hashes`, `mask_key` |
| `divergence.rs` | `Class` (expected / explained / unexplained), `ExplanationRule`, `classify` |
| `reader.rs` | `EngineReader` seam + `MockReader` |
| `dialect.rs` | `bucket_expr`, `sub_bucket_checksum_query`, `key_hash_query`, `count_query`, `aggregate_query` |

---

## Implementation details worth remembering

### The control gate has two conditions, not one

`TierOutcome::passed()` requires zero blocking divergences **and** every control fired. "Found
nothing" and "cannot see anything" produce the same divergence list, and only the control
distinguishes them. `verdict()` therefore never renders "passed" when a control was missed, and it
names the missed controls, because "controls failed" is not actionable.

One honest gap, documented in a test: `controls_all_fired()` is vacuously true for an empty control
list, so `passed()` alone cannot distinguish "ran the controls" from "ran none". The verdict states
the count, which is what RFC 0162's report renders beside the result — that is the mechanism that
makes a thin run *look* thin.

### V2 aggregates over the canonical form, not the raw column

Every aggregate is taken over the canonical expression. Two reasons: a V2 disagreement then means the
same thing a V3 disagreement means, so the tiers cannot contradict each other over a rendering
difference; and `min`/`max` over canonical text side-steps collation entirely, because the canonical
form is binary-comparable by construction.

V2 reports the column and which aggregate moved, never the value. A min or a max *is* row data.

### An absent bucket and a zero-count bucket are the same thing

Engines differ on whether a `GROUP BY` emits an empty group. Treating an absent bucket as
`BucketChecksum::default()` on both sides removes a whole class of false positives that would
otherwise appear only on certain data shapes — the worst kind of flake to diagnose.

### Bisect refuses to conclude "no divergence" on re-measurement

If a failed bucket stops differing when it is re-bucketed, the data moved under the run — a
concurrent write on a live source. Reporting "no divergence" there would be a false green, so
`BisectError::VanishedUnderRemeasurement` says what happened and tells the caller to re-run against a
fixed snapshot at a recorded LSN.

### Keys are masked in facts

`mask_key` renders `customer-4815162342` as `c··2#a1b2c3d4`: first character, last character, and
eight hex of the hash. Enough to find the row again with the source at hand, not enough to be a leak
on its own. Keys of two characters or fewer reveal nothing at all rather than most of themselves.

---

## Knowledge Captured

**A test named for a claim it did not test.** `two_same_typed_columns_swapped_is_caught_only_at_v3`
passed on the first run. It asserted V1 was blind and V3 caught the swap — and never checked V2. The
fixture swapped `name` and `note`, whose value multisets differ, so measuring it showed V2 catching
the swap on three separate aggregates. The test's name asserted something the fixture could not
support, and RFC 0156's tier-table claim was therefore untested while looking verified.

Fixed by adding two columns built for it: `tag_a` and `tag_b` hold `x0`..`x9`, 500 of each, offset by
five, so their multisets are **identical**. Swapping them changes every row and moves no aggregate.
The test now asserts V1 blind, **V2 blind**, V3 catches — and the V2 assertion is the load-bearing
one, because without it the test would pass again on a fixture that does not isolate the tier.

The general lesson, and it applies to the whole control suite: a control test must assert the tiers
*below* the claimed one are blind. Otherwise "caught at V3" is only "caught somewhere", and the tier
table stops being a specification.

**`ALTER TABLE … UPDATE` requires a `WHERE` on ClickHouse.** `WHERE 1 = 1` for an unconditional
mutation. Usefully, ClickHouse evaluates every assignment against the *original* row, so
`UPDATE a = b, b = a` is a true simultaneous swap rather than two sequential writes — which is what
makes the aggregate-invariant fixture possible in one statement.

**`SETTINGS mutations_sync = 2` is mandatory in a test that reads after a mutation.** ClickHouse
mutations are asynchronous by default, so without it the test reads the pre-mutation table and passes
for the wrong reason. This is the same class of bug as the eventual-dedup problem RFC 0156 already
warns about, arriving through a different door.

**A blunt mock is the right mock here.** `MockReader` answers by *exact* SQL match. A mock that
matched a similar query would let the tiers drift away from the SQL they actually build while the
tests kept passing. Several test setups are more verbose as a result, which is the correct trade.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-validate/src/tiers.rs` | New — tiers, `TierOutcome`, control gate, V1/V2/V3 |
| `ekos/crates/migrate-validate/src/bisect.rs` | New — bisect, V4, key diffing, key masking |
| `ekos/crates/migrate-validate/src/divergence.rs` | New — classification, explanation rules |
| `ekos/crates/migrate-validate/src/reader.rs` | New — `EngineReader` seam, `MockReader` |
| `ekos/crates/migrate-validate/src/dialect.rs` | Bisect/count/aggregate query builders + tests |
| `ekos/crates/migrate-validate/src/lib.rs` | Module wiring and re-exports |
| `ekos/crates/migrate-validate/tests/live_tiers.rs` | New — 7 live tests, incl. the corrected swap test |
| `TODO.md` | Migrate Phase 1 tiers ticked; V0 and the fact/CLI wiring listed with what blocks them |

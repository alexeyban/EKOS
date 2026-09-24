# RFC 0156 — Validation tiers, bisect, divergence classification and planted controls

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation), RFC 0155 (canonical serialization — the layer below this one),
RFC 0150 (planted-control discipline, private implementation), RFC 0159/0165 (target designs this
validates against), RFC 0162 (the report that consumes these results)

---

## Summary

Defines the validation tiers V0–V6, the bisect algorithm that turns a failed checksum into exact
divergent keys, the classification that decides whether a divergence blocks a unit, and the
planted-defect control suite that decides whether a tier is allowed to report green at all.

This is the differentiator. Everything else in EKOS Migrate is available elsewhere in some form;
a validator that proves it can detect the defects it claims to detect is not.

## Motivation

Every migration tool reports success. Almost none of them can answer "how do you know?" — and the
ways a migration silently loses data are well known and boring: a chunk boundary off by one, a
decimal truncated by an implicit cast, a timezone applied twice, `NULL` arriving as empty string,
duplicate rows from a retried load, a column pair swapped because both are `text`.

None of those raise an error. All of them produce a load that looks complete. The only defence is a
validator that has been shown a copy of each defect and caught it.

## Design

### Tiers

Each tier is independently runnable and each subsumes the ones below it in cost, not in meaning —
a passing V3 does not make V1 redundant, because V1 failing tells you something different.

| Tier | Checks | Method | Cost |
|---|---|---|---|
| **V0** Structural | Tables, columns, types, nullability, ordering, constraints match the approved `MigrationTargetDesign` | Target catalog read compared to the design fact. No data. | free |
| **V1** Counts | Row count per table, and per partition or chunk | `count(*)` both sides; source on a replica | low |
| **V2** Aggregates | Per column: null count, min, max, sum (numeric), sum of lengths (text), approximate distinct; floats with tolerance | Generated per-dialect aggregate packs, one pass per side | low–medium |
| **V3** Checksums | Order-independent bucketed row hashes (RFC 0155) | `(count, sum)` per bucket, both sides | medium |
| **V4** Row diff | The exact divergent keys and columns | Bisect from failed V3 buckets | medium, proportional to divergence |
| **V5** Logic equivalence | Migrated views, matviews and functions produce the same results | IR diff plus differential execution on identical fixtures (RFC 0164) | medium |
| **V6** Business invariants | User-declared assertions (balances net to zero, every order has a customer) | Declarative assertions compiled per dialect | varies |

A unit's required tier is policy. The default for sign-off is V4 for every table and V5 for every
translated logic object.

### The independent oracle rule

**The validator must not read the target through the thing that wrote it.** A Spark job that
mis-serialized a decimal on write will mis-serialize it identically on read, and the comparison
passes.

- **Delta:** validation reads Parquet through `delta-rs` directly (RFC 0165), not through the Spark
  session that performed the load.
- **ClickHouse:** validation uses a separate read-only user, and dedup-aware reads (`FINAL`, or an
  explicit argMax pattern) for `ReplacingMergeTree`, because "the merge has not happened yet" and
  "the load duplicated rows" are indistinguishable otherwise — and only one of them is fine.
- **PostgreSQL:** read from a replica where one exists, with the replica-lag guard from RFC 0157
  asserted *before* the run, and the source LSN recorded on the `MigrationValidationRun` fact so a
  later reader knows exactly which source state was compared.

### Bisect

V3 gives failed buckets. V4 turns each into keys:

```
1. failed bucket B at fan-out N
2. re-bucket B's rows at N × 16 on both sides, compare (count, sum) per sub-bucket
3. repeat until a sub-bucket holds ≤ threshold rows (policy, default 1000)
4. pull (pk, row_hash) pairs for that sub-bucket from both sides into the validator
5. diff in memory → three sets: source-only keys, target-only keys, keys whose row_hash differs
6. for differing keys only, pull the full rows, masked by PII policy, and diff per column
```

Step 6 is the only point where source row values enter the validator process. They are held in
memory, compared, reported as **column names and a classification**, and discarded. They are never
persisted to the ledger — enforced by the ledger-scan test from RFC 0154.

The fan-out of 16 is chosen so that a single divergent row in a 100M-row table is isolated in about
six rounds rather than sixteen, each round being two cheap aggregate queries.

### Divergence classification

Every divergence lands in exactly one class. Only the third blocks.

| Class | Meaning | Example |
|---|---|---|
| `expected` | Matches an approved `MigrationDisposition` | Duplicate rows deduplicated by an approved `ReplacingMergeTree` design |
| `explained` | Matches a rule that proves why, deterministically | Trailing-space trimming where the approved mapping is `char(n)` → `String` and the rule shows the delta is exactly the padding |
| `unexplained` | Everything else | Blocks the unit |

A unit reaches `validated` only at **zero unexplained divergences**. `expected` and `explained` both
require a fact to point at: an approved disposition, or a named rule that produced the explanation.
"The developer looked at it and it seemed fine" is not a class.

### Planted-defect controls

This is the part that makes the rest mean something.

A **control run** takes a known-good migrated unit, applies one synthetic defect to a scratch copy
of the target, runs the tier, and asserts the tier reports failure. The suite:

| Control | Lowest tier that must catch it |
|---|---|
| Dropped row | V1 |
| Duplicated row | V1 (count) — and V3, because dedup-aware reads could hide it |
| Off-by-one chunk boundary | V1 |
| Truncated decimal scale | V2 (sum) and V3 |
| Timezone shifted by one hour | V3 (V2 min/max may miss a uniform shift inside the range) |
| `NULL` → empty string | V2 (null count) and V3 |
| Trailing-space trimming | V3 |
| Microsecond precision loss | V3 |
| Two same-typed columns swapped | V3 — V1 and V2 cannot see it, and this is why V3 exists |
| Row present with all-null non-key columns | V2 and V3 |
| Sub-threshold corruption: exactly one wrong byte in one row of 10M | V3, and V4 must name the key |

Rules, both enforced in code:

1. **A tier that misses its control reports `failed`, not `passed`.** The control result is part of
   the `MigrationValidationRun` fact (`MigrationControlResult`), and RFC 0162's report gate refuses
   to sign off a run whose controls did not all fire.
2. **CI always runs the full suite.** Not sampled, not opt-in. A pull request that weakens a tier
   fails on the control, which is the only cheap moment to catch it.

The controls are generated from a declarative catalog so adding one is a data change, and each
carries the tier it targets, so "we added a control nothing catches" is visible rather than quiet.

### False positives

A validator that cries wolf is abandoned, and an abandoned validator proves nothing. The complement
of the control suite is a **clean-run assertion**: a full V0–V4 run over every corpus (RFC 0154's
LedgerSMB and Pagila, plus TPC-H) after a known-correct migration must produce **zero** divergences
of any class. A single unexplained divergence on a clean run is a release blocker with the same
weight as a missed control.

## Testing

- Golden: every control fires at its declared tier and at every tier above it.
- Negative: every control is *not* reported by tiers below its declared tier (so the table is a
  specification, not a wish).
- Clean-run: zero divergences across all corpora after a correct migration.
- Bisect: a seeded single-row corruption in a 10M-row fixture is isolated to the exact key, and the
  number of round-trips is asserted to stay within the expected `log16` bound.
- Isolation: a ledger scan after a full V4 run finds no source row values.

## Alternatives considered

- **Row-by-row comparison of full tables.** Correct and unaffordable; it also requires both sides
  sorted, which reintroduces collation dependence.
- **Sampling instead of full checksums.** A sample cannot bound "one wrong row in ten million",
  which is precisely the case people buy this for. Sampling is offered at V2 for huge tables with
  the statistical guarantee stated in the report — never as a substitute for V3 at sign-off.
- **Trusting the load engine's own row counts.** Rejected by the independent-oracle rule: it is the
  same process reporting on itself.
- **Making controls opt-in for speed.** Rejected. The one run where they are skipped is the run that
  needed them.

## Open questions

- [ ] Bisect threshold and fan-out defaults against a real 100M-row table.
- [ ] Should V6 assertions be authored in EKL, in SQL, or in a small declarative DSL compiled per
      dialect?
- [ ] Retention for per-bucket facts on large tables — summarize at unit level and keep per-bucket
      detail only for failed buckets? (Shared with RFC 0154's open questions.)

## Acceptance criteria

- [ ] Every control in the catalog fires at its declared tier; none fires below it.
- [ ] A tier that misses a control reports `failed`, asserted by test.
- [ ] Clean runs on all corpora produce zero divergences.
- [ ] Bisect isolates a single corrupted row in a 10M-row fixture to the exact key.
- [ ] No source row values reach the ledger, asserted by a ledger scan after a full V4 run.

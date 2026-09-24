# RFC 0159 — Type mapping registry and ClickHouse target design

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation), RFC 0157 (profiles drive specialization), RFC 0158 (lossiness
findings), RFC 0155 (approved scale/precision feeds canonical serialization), RFC 0056 (ClickHouse
connector), RFC 0160 (emits and executes the DDL), RFC 0165 (the Delta counterpart)

---

## Summary

The `ekos-typemap` registry — PostgreSQL type → ClickHouse type, specialized by measured data, with
every mapping carrying a lossiness class — and the target design pass that chooses table engine,
`ORDER BY`, `PARTITION BY`, `LowCardinality` and codecs from evidence rather than from habit.

Every choice is a `MigrationTargetDesign` or `MigrationTypeMapping` fact carrying the evidence that
motivated it, so "why is this table ordered that way" is answerable a year later by someone who was
not there.

## Motivation

Two failure modes bracket this work.

A **naive mapping** (`numeric` → `Decimal(76,20)`, every column `Nullable`, `ORDER BY` the primary
key because that is what PostgreSQL had) produces a ClickHouse database that is correct, enormous
and slow — and it is what a type-lookup table alone gives you. The PK is frequently the *worst*
`ORDER BY` in ClickHouse, because it is unique and therefore useless for skipping granules on the
filters people actually run.

An **aggressive mapping** (`numeric` → `Decimal(18,2)` because the first thousand rows fit) silently
truncates in month three.

Both are avoided by the same thing: measured data. RFC 0157 already profiled the actual scale, the
actual cardinality, the actual ranges, and `pg_stat_statements` already knows the actual filters.

## Design

### Registry entries

```
source_type        e.g. "numeric", "numeric(p,s)", "timestamptz", "inet"
target_type        a template, possibly parameterized by profile
condition          optional predicate over the column's profile
lossiness          exact | widening | narrowing-safe | lossy
rule_id            stable, cited by every MigrationTypeMapping fact it produces
requires           optional: a disposition class needed before this mapping may be approved
```

Lossiness classes, which are the point of the registry:

- **`exact`** — every source value round-trips. `uuid` → `UUID`.
- **`widening`** — the target holds strictly more. `int4` → `Int64`, `enum` → `String`.
- **`narrowing-safe`** — the target is narrower, **and the profile proves no source value is
  affected**. Unconstrained `numeric` → `Decimal(18,2)` where the measured max precision is 14 and
  max scale is 2. The proof is a fact: the profile that established the bound is cited by the
  mapping, so a later re-profile that breaks the bound invalidates the mapping rather than silently
  outliving it.
- **`lossy`** — values will be changed or lost. Requires a `MigrationDisposition` and R3 approval.
  A lossy mapping can never be auto-selected, even when it is obviously what the user wants.

`narrowing-safe` is the class that earns the profiler its cost, and the invalidation rule is what
keeps it honest: it is a claim about data at a point in time, and the fact records which point.

### Profile-driven specialization

| Column | Naive | With profile |
|---|---|---|
| `numeric` (unconstrained), max precision 14, max scale 2 | `Decimal(76,20)` | `Decimal(18,2)` — narrowing-safe |
| `text`, 12 distinct values over 40M rows | `String` | `LowCardinality(String)` |
| `varchar(50)`, never null | `Nullable(String)` | `String` — non-nullable, real storage and query win |
| `int8` identity, max observed 2.1M | `Int64` | `Int32` — narrowing-safe, but see below |
| `timestamptz`, whole-second values only | `DateTime64(6,'UTC')` | `DateTime64(6,'UTC')` — kept; precision reduction is lossy for future writes |

The last two rows are the interesting ones and pull in opposite directions. Narrowing an identity
column by observed maximum is **narrowing-safe for today's data and unsafe for tomorrow's**, because
the column keeps growing. The registry therefore distinguishes *bounded* columns (a natural domain:
country code, status) from *growing* ones (identity, sequence-backed, monotonic per RFC 0157's
monotonicity metric) and refuses to narrow the latter on profile evidence alone. Timestamp precision
is treated the same way: the profile describes the past, not the schema's future.

### ClickHouse target design

**Engine.**

| Situation | Engine |
|---|---|
| Append-only fact table | `MergeTree` |
| Mutable entity table (PostgreSQL row updated in place) | `ReplacingMergeTree(version)` with an explicit version column |
| Pre-aggregation explicitly chosen by a human | `SummingMergeTree` / `AggregatingMergeTree` |

`ReplacingMergeTree` is the default for anything with observed updates (`pg_stat_user_tables`
`n_tup_upd > 0`), and it comes with a **mandatory** RFC 0158 compatibility finding, because dedup is
*eventual*: a `SELECT` before a merge returns duplicates. That surprises people badly, it is
invisible on load day, and it changes how RFC 0156 must read the table. The version column is chosen
from a real update-time column where one exists, and where none does, that is a finding — not an
invented `now()`.

**`ORDER BY`** is derived, in priority order, from:

1. Filter and join predicates in `pg_stat_statements`, weighted by call count.
2. Filter predicates in views, ETL and application code from the CKM.
3. Cardinality from profiles — low-cardinality, high-selectivity columns first.
4. The primary key last, for uniqueness, only if needed.

The rationale and the queries that motivated each column are attached to the `MigrationTargetDesign`
fact. Where no workload evidence exists at all, the design says so and proposes the PK with an
explicit "no query-shape evidence available" note, rather than presenting a guess as a derivation.

**`PARTITION BY`** only when the table is large enough for it to pay and a low-cardinality time
column exists — by default `toYYYYMM(ts)`, with a guard that the estimated partition count stays
within policy. Over-partitioning is the single most common self-inflicted ClickHouse wound, so the
design refuses a configuration projected to exceed the threshold and reports why.

**Codecs.** `Delta` + `ZSTD` for monotonic integers and timestamps, `ZSTD` for text, `T64` for
bounded integers, `DoubleDelta` for slowly-varying series — selected from the profile's monotonicity
and cardinality metrics, each with the metric cited.

**Nullability.** `Nullable(T)` only where the profile shows nulls or the source column is nullable
*and* the code writes nulls. Every `Nullable` removal is a narrowing that requires the source column
to be `NOT NULL` or the profile to prove zero nulls — and it is recorded as such, because a null
arriving later becomes a load failure rather than a silent zero.

### Wave planning

Units are ordered from the dependency graph — declared FKs, inferred FKs (RFC 0158), view and
function dependencies, ETL edges — leaves first. Cycles are grouped into a single unit rather than
broken arbitrarily. Wave assignment is a fact, so "why is this table in wave 3" is answerable.

## Testing

- Registry: every entry has a fixture column whose mapping is asserted, including its lossiness
  class, and every `narrowing-safe` entry has a paired fixture that *violates* the bound and is
  reported `lossy` instead.
- Specialization: the table in *Profile-driven specialization* is a test, row by row.
- Growing-column guard: an identity column with a small observed maximum is **not** narrowed.
- Design: a fixture with a known `pg_stat_statements` workload produces the expected `ORDER BY`,
  and the design fact cites the queries.
- Partition guard: a configuration projected past the partition-count threshold is refused.
- `ReplacingMergeTree` selection always emits the eventual-dedup finding.
- No-evidence case: a table with no workload evidence yields a design that says so.

## Alternatives considered

- **A static type-mapping table with no profiling.** Rejected — produces the naive mapping described
  in *Motivation*, and cannot ever offer `narrowing-safe`.
- **LLM-chosen `ORDER BY`.** Rejected: the inputs are numeric and available, so a deterministic
  derivation is both better and auditable. An LLM may *explain* a design; it does not choose one.
- **Defaulting every table to `ReplacingMergeTree` for safety.** Rejected: it imposes eventual-dedup
  semantics and merge cost on append-only tables that never needed either.
- **Narrowing growing columns on observed maximum.** Rejected as above; the profile describes the
  past.

## Open questions

- [ ] Partition-count threshold default, and whether it should scale with table size.
- [ ] `LowCardinality` cutoff — ClickHouse guidance is roughly under 10k distinct, but the real
      trade-off depends on row count. Derive from both?
- [ ] When no update-time column exists for `ReplacingMergeTree`, is load batch sequence an
      acceptable version, or does that always need a human decision?

## Acceptance criteria

- [ ] Every registry entry is tested with a fixture, including a bound-violating counterpart for
      each `narrowing-safe` entry.
- [ ] A `lossy` mapping cannot be auto-selected, asserted by test.
- [ ] Every `MigrationTargetDesign` cites the evidence for engine, `ORDER BY` and partitioning, or
      states explicitly that no evidence exists.
- [ ] Approved DDL for LedgerSMB is generated and creates successfully in a ClickHouse sandbox;
      RFC 0156 V0 passes against it.

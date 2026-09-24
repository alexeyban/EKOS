# RFC 0165 — Delta Lake / Spark SQL as a second target

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation — ClickHouse-first decision), RFC 0155 (canonical form gains a
third engine), RFC 0156 (independent oracle), RFC 0158 (compatibility rule shape), RFC 0159
(the ClickHouse counterpart this parallels), RFC 0160 (execution)

---

## Summary

Adds Delta Lake via Spark SQL as a second migration target: a type mapping, a target design pass, a
dialect emitter, two execution backends, and — the reason Delta is genuinely easier to validate than
ClickHouse — an independent reader through `delta-rs` that does not go through Spark at all.

Deliberately scheduled after the validator is proven. Every abstraction here is asserted against by
a working ClickHouse implementation rather than guessed at in advance.

## Motivation

ClickHouse first was the right call for demo continuity and existing code (RFC 0154), but Delta is
the larger enterprise destination, and it stresses different parts of the design:

- Spark's `DECIMAL` caps at precision 38 where ClickHouse reaches 76 — so `numeric` compatibility
  findings differ per target, which is the first real test of whether the RFC 0158 rule shape is
  genuinely target-parameterized or accidentally ClickHouse-shaped.
- Execution is remote and asynchronous over a REST API, not a synchronous HTTP call.
- The independent-oracle rule (RFC 0156) becomes straightforward rather than awkward, because Delta
  files can be read without the engine that wrote them.

## Design

### Type mapping deltas

The registry (RFC 0159) is keyed by target. The entries that differ materially:

| PostgreSQL | ClickHouse | Delta / Spark | Note |
|---|---|---|---|
| `numeric(p,s)`, p > 38 | `Decimal128/256` | **no representation** | lossy on Delta, exact on CH — the clearest proof the rules are per-target |
| `numeric` unconstrained | `Decimal(P,S)` from profile, max 76 | same, **max 38** | narrowing-safe only if the profile fits 38 |
| `timestamptz` | `DateTime64(6,'UTC')` | `TIMESTAMP`, session-TZ dependent | must pin `spark.sql.session.timeZone=UTC` |
| `timestamp` | `DateTime64(6)` | `TIMESTAMP_NTZ` (Spark 3.4+) | below 3.4 this is lossy; a version check, not an assumption |
| dates before 1900 | outside `Date32` | representable | but calendar rebase mode must be pinned |
| `uuid` | `UUID` | `STRING` | widening |
| `inet`/`cidr` | `IPv4`/`IPv6` | `STRING` | widening |
| `jsonb` | `JSON` or `String` | `STRING`, or `VARIANT` on Databricks | |
| enums, domains | `Enum8/16`, `LowCardinality` | `STRING` + `CHECK` | Delta enforces `CHECK`, ClickHouse does not |
| `bytea` | `String` | `BINARY` | |
| identity/serial | none | `GENERATED ALWAYS AS IDENTITY` | Delta is closer to the source here |
| PK/UNIQUE/FK | not enforced | informational on Unity Catalog; `NOT NULL`/`CHECK` enforced | still a behavioural finding |

Delta enforcing `CHECK` and `NOT NULL` where ClickHouse does not is worth stating plainly: some
source constraints survive a Delta migration and do not survive a ClickHouse one. That is a real
input to target choice and belongs in the report.

**Calendar rebase.** Spark's Julian/Gregorian rebase for pre-1582 dates is governed by
`spark.sql.parquet.datetimeRebaseModeInWrite` and its read counterpart. Left at `LEGACY` or
`CORRECTED` inconsistently between write and read, dates shift silently. Both are pinned explicitly
on every session, recorded on the `MigrationExecution` fact, and asserted by a fixture containing a
pre-1582 date — an obscure failure that is trivial to prevent and miserable to diagnose later.

### Target design

- **Partitioning** only for large tables with a genuinely low-cardinality partition column. The
  Delta small-file problem punishes over-partitioning at least as badly as ClickHouse does, and the
  same projected-partition-count guard from RFC 0159 applies.
- **Liquid clustering** (Databricks) or **Z-order** (OSS) for the access patterns
  `pg_stat_statements` reveals — the same evidence that drives ClickHouse `ORDER BY`, lowered
  differently. One derivation, two emitters.
- **Constraints**: `NOT NULL` and `CHECK` emitted wherever the source constraint survives and the
  profile supports it. This is the rare case where the target is *stricter* than ClickHouse, so a
  load can fail on a constraint — which is preferable to silent acceptance, and is why the profile
  must prove the constraint holds before it is emitted.
- **Column mapping mode** set to `name` by default, so renames do not rewrite data.
- **Identity columns** where the source used `serial`/`identity` and nothing downstream depends on
  value continuity — which impact analysis can answer.

### Execution backends

| Backend | Use | Mechanism |
|---|---|---|
| **Databricks SQL Statement Execution API** (primary) | Databricks workspaces | REST: submit, poll, fetch. Async by nature; the executor's chunk model already tracks long-running statements. |
| **Spark Thrift Server / HiveServer2** (secondary) | OSS Spark | Long-lived session, synchronous |
| **Spark Connect** | — | Evaluated, not depended on: Rust client support is immature. Revisit later. |

Both backends implement one `TargetExecutor` trait so RFC 0160's classifier, pinning and environment
rules apply unchanged. The classifier gains a Spark SQL dialect; the risk model does not change.

### Independent reader — `delta-rs`

The validator reads Delta tables through `deltalake` (delta-rs) directly from object storage:
transaction log, schema, per-file statistics and the Parquet data. No Spark session involved.

This satisfies RFC 0156's independent-oracle rule properly rather than approximately. A Spark job
that mis-wrote a decimal cannot mask it on read, because the reader is a different implementation
reading the files.

It also makes V0 and parts of V2 nearly free: the Delta transaction log carries per-file row counts
and per-column min/max/null-count statistics, so a structural check and a first-pass aggregate
comparison need no query engine at all. Where file statistics are used instead of a scan, the fact
records that — a statistic is a claim by the writer, and the report should not present it as an
independent measurement.

**Data movement**: Spark JDBC read from PostgreSQL with partitioned key ranges, `INSERT INTO` for
initial load, `MERGE INTO` by key for incremental. EKOS issues and tracks; Spark moves. The deferred
staged-Parquet path (RFC 0160) would land here first if it is ever justified, since object storage is
already in the picture.

## Testing

- Every differing registry row has a fixture, including the p > 38 case that is exact on ClickHouse
  and lossy on Delta — asserted on both targets in one test, which is what proves the rules are
  per-target.
- Timezone: a `timestamptz` fixture round-trips identically with the session TZ pinned, and
  demonstrably differs without it.
- Calendar rebase: a pre-1582 date fixture round-trips with both modes pinned.
- `TIMESTAMP_NTZ`: the Spark version check downgrades the mapping to lossy below 3.4.
- Canonical form (RFC 0155): every golden fixture's literal hash matches on Spark, making it a
  three-engine agreement.
- Independent reader: a deliberately corrupted Parquet file is detected by delta-rs validation where
  a Spark-side self-check would pass.
- Both execution backends run the same artifact set and produce identical results.
- Constraint emission: a `CHECK` whose profile does not support it is not emitted.

## Alternatives considered

- **Spark Connect as the primary backend.** Rejected for now: the Rust client ecosystem is immature,
  and betting the primary execution path on it would block the RFC on an external project.
- **Databricks-only.** Rejected: it would make EKOS Migrate unusable on OSS Spark, and the Thrift
  backend is a small addition once the trait exists.
- **Validating Delta through Spark.** Rejected by the independent-oracle rule — the specific failure
  it exists to prevent.
- **Trusting Delta file statistics for V2 entirely.** Rejected: they are writer-provided claims, and
  a writer bug is exactly the case being tested. Usable as a fast pre-filter, recorded as such.
- **Shipping Delta alongside ClickHouse from the start.** Rejected in RFC 0154: it doubles every
  surface before any of it is proven.

## Open questions

- [ ] Does `deltalake` cover the Delta protocol features real workspaces use — deletion vectors,
      column mapping, v2 checkpoints — at the version pinned? Verify before depending on it for V3.
- [ ] Unity Catalog governance: does EKOS need catalog-level permissions modelling, or is
      schema-level enough for v1?
- [ ] Is `MERGE INTO` performance acceptable for incremental sync at scale, or is the change-data-feed
      pattern required from the start (RFC 0166)?

## Acceptance criteria

- [ ] Every Delta-specific registry row has a fixture, with the p > 38 case asserted on both targets.
- [ ] RFC 0155 golden hashes match on Spark, giving three-engine agreement.
- [ ] Session timezone and calendar rebase are pinned and recorded on every execution fact.
- [ ] delta-rs validation detects a corruption that a Spark self-check misses.
- [ ] Both execution backends run the same artifacts to the same result.
- [ ] LedgerSMB migrates to Delta and validates to V4.

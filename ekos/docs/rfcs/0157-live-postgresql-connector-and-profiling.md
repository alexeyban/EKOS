# RFC 0157 — Live PostgreSQL connector, profiling tiers, and redaction at the live entry point

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation), RFC 0043 (redaction baseline), RFC 0146 (PostgreSQL dialect —
the file-based facts this connector is reconciled against), RFC 0158 (consumes profiles),
RFC 0159 (target design consumes query shapes and distributions), RFC 0135 Part B (provenance)

---

## Summary

Adds `ekos-pg-live`: a read-only live PostgreSQL connector that introspects the catalog, reconciles
it against the file-based DDL facts RFC 0146 already compiles, and profiles data in three escalating
cost tiers. It is the **third raw-content entry point** in EKOS, and like the other two it runs
RFC 0043 redaction before anything is held, hashed, logged or persisted.

No row values are ever persisted. Profiles are aggregates, hashes and classifications.

## Motivation

RFC 0146 gave EKOS an accurate picture of a PostgreSQL schema *as checked into a repository*. A
migration needs three things that a repository cannot provide:

1. **What is actually deployed.** Live schemas drift from their DDL. That drift is itself a finding
   — often the most valuable one in the whole exercise.
2. **What the data looks like.** Type mapping without profiles is guesswork: an unconstrained
   `numeric` is `Decimal(76, s)` or `Decimal(18,2)` depending entirely on what is in it.
3. **What the workload looks like.** ClickHouse `ORDER BY` and partitioning are chosen from query
   shapes, and `pg_stat_statements` knows them.

## Design

### Connection and session safety

A connection is opened from a `MigrationConnectionRef` (RFC 0154) resolved against an environment
variable, OS keychain entry or cloud secret reference. The ledger never sees a credential.

Every session sets, before any statement:

```sql
SET default_transaction_read_only = on;
SET statement_timeout = '<policy>';
SET lock_timeout = '<policy>';
SET idle_in_transaction_session_timeout = '<policy>';
SET work_mem = '<policy>';          -- deliberately low
SET application_name = 'ekos-migrate/<run_id>';
```

`application_name` carries the RFC 0135 `run_id`, so a DBA watching `pg_stat_activity` during an
unexpected load can attribute every statement to a specific `ekos migrate` invocation without
asking anyone.

The role is expected to be read-only at the server, not merely at the session. `ekos migrate
doctor` (folded into `ekos doctor`) verifies this by attempting a write to a scratch name and
asserting it fails; a role that *can* write is reported as an R1 finding rather than silently used.

**Replica preference.** If a replica is configured, all profiling and all V1–V4 source reads go
there. Before each run the connector records `pg_last_wal_replay_lsn()` and the lag; a run started
above the policy lag threshold is refused, and the LSN is recorded on the run fact so RFC 0156 knows
exactly which source state it compared.

### Catalog introspection

One pass over `pg_catalog` / `information_schema` producing facts for: schemas, tables, partitioned
tables and their partitions, columns with types/defaults/identity/generated, constraints (PK, FK
including `NOT VALID`, UNIQUE including partial and deferrable, CHECK, EXCLUDE), indexes, sequences,
views, materialized views, functions and procedures with language, triggers, extensions, enums,
domains, composite and range types, comments, grants and RLS policies.

This is the machinery behind RFC 0154's coverage requirement: the introspector's output is the
authoritative denominator for RFC 0158's completeness check. A catalog object kind that this pass
does not emit is a hole in the guarantee, so the pass asserts against `pg_class`/`pg_proc` counts
rather than trusting its own enumeration.

**Drift reconciliation.** Each live object is matched to its RFC 0146 file-based fact. Three outcomes
become findings: present live but not in the repository, present in the repository but not live, and
present in both with a structural difference (column added, type widened, constraint dropped). Drift
is reported per object with both definitions attached as evidence.

### Usage evidence

- `pg_stat_user_tables` — sequential/index scans, tuples inserted/updated/deleted, last autovacuum
  and autoanalyze. Feeds "archive, don't migrate" candidates and write-rate estimates for the
  incremental design.
- `pg_stat_statements` where installed — normalized query text, calls, total time, rows. Filter
  predicates and join keys extracted from these drive `ORDER BY` selection in RFC 0159. Query text
  from this view passes through redaction before storage, because constants are sometimes retained
  by configuration.
- The CKM itself — which application code, ETL and documents already reference each table and
  column, via `ekos_impact`. This is the part no competing tool has, and it is free here.

### Profiling tiers

| Tier | Method | Cost | Default |
|---|---|---|---|
| **P0** | `pg_class.reltuples`, `pg_stats` (from `ANALYZE`), catalog sizes, bloat estimate | ~free | always |
| **P1** | `TABLESAMPLE SYSTEM`/`BERNOULLI`, bounded by row count and wall-clock | low | on |
| **P2** | Full scans: exact counts, exact distinct, exact min/max | high | approval above a policy threshold |

Every P2 statement is costed with `EXPLAIN` first, and the estimate is recorded on the profile fact.
A P2 above the budget raises a `MigrationApprovalRequest` (R1) rather than running.

**Metrics per column:** null rate, approximate distinct (HyperLogLog), min/max, length distribution,
the numeric scale and precision *actually used* (not the declared one — this is what makes an
unconstrained `numeric` mappable), top-k as **hashes with counts**, pattern classes (email, UUID,
ISO date in a text column, phone, JSON-in-text), monotonicity (watermark candidates for RFC 0166),
and duplicate rate on declared and candidate keys.

### Redaction, and why top-k is not simply hashed

The other two raw-content entry points in EKOS — the `Observer` path and `recover.rs`'s direct file
reads — each call `ekos_common::redaction::redact` before anything is persisted. This connector is
the third, and calls the same function on every sampled value and on `pg_stat_statements` text
before that value is used for anything at all, including hashing.

Hashing is **not** anonymization. On a low-cardinality column (`status`, `country`, `gender`) an
attacker with the hash and the domain recovers every value by enumeration in microseconds. So:

- A column classified as PII (by name heuristic or by pattern class) has **no top-k persisted at
  all** — not values, not hashes. Only cardinality and null rate.
- A non-PII column's top-k is persisted as salted hashes, with the salt scoped to the migration
  project and not itself persisted, so hashes are comparable within a project and meaningless
  outside it.
- Sampled values exist only in process memory during a profiling call and are dropped before the
  fact is written.

The ledger-scan test from RFC 0154 asserts the outcome rather than the intent: after a full P1
profile of a fixture database seeded with recognizable values, none of those values appears anywhere
in the ledger.

### PII classification

Name heuristics (`email`, `ssn`, `dob`, `phone`, `first_name`, …) plus pattern classes measured on
samples, combined into a `MigrationPiiClassification` with method and confidence. Classification is
a *proposal* with respect to masking policy but is applied **conservatively immediately**: an
unconfirmed PII classification suppresses top-k straight away. Downgrading a classification is a
human action, because the failure direction matters — over-suppressing costs a little insight, and
under-suppressing writes personal data into an append-only ledger that cannot delete it.

## Testing

- Fixture database (Docker) with every catalog object kind, asserted round-trip to facts, with the
  count check against `pg_class`/`pg_proc`.
- Drift: a fixture whose live schema deliberately differs from its checked-in DDL in all three ways;
  each produces the right finding.
- Session safety: a write attempt on the profiling connection fails; a run against a lagging replica
  is refused.
- Redaction: seeded emails, card-shaped numbers and keys in a fixture do not appear in the ledger
  after a P1 profile; a low-cardinality PII column has no top-k fact at all.
- Cost: a P2 above budget raises an approval request instead of executing.

## Alternatives considered

- **`pg_dump --schema-only` and parse it.** Rejected: it is a second parser to maintain, it loses
  `pg_stat_*` entirely, and RFC 0146 already parses DDL — reconciling live against that is strictly
  more informative than replacing it.
- **Logical replication for the initial profile.** Wrong tool: it gives changes, not distributions.
  It arrives properly in RFC 0166.
- **Persisting sampled values for later analysis.** Rejected by RFC 0154's non-goal and by the
  append-only ledger: there is no way to un-commit a mistake here.
- **Exact distinct everywhere.** Rejected on cost; HLL with a stated error bound is enough to choose
  `LowCardinality` or a bucket count, which is what the number is for.

## Open questions

- [ ] `tokio-postgres` or `sqlx`? Decided by the RFC 0154 Phase 0 concurrency spike, since the
      answer interacts with the non-`Sync` `KnowledgeStore`.
- [ ] Is the project-scoped top-k salt worth the complexity versus simply omitting top-k for every
      column? What decision does top-k actually change downstream?
- [ ] Should drift findings block, or only inform? Proposed: block at R3 when the drift changes a
      type or a constraint, inform otherwise.

## Acceptance criteria

- [ ] Every catalog object kind in the fixture appears as a fact; counts reconcile with `pg_class`.
- [ ] LedgerSMB and Pagila fully discovered and profiled at P0+P1 within a configured time budget.
- [ ] Drift report produced against the repositories' own DDL.
- [ ] Ledger scan after profiling finds zero credentials and zero source row values.
- [ ] A P2 above budget requests approval rather than scanning.

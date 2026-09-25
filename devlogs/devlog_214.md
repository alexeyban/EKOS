# Devlog 214 — Inferred foreign keys: what the code says, and whether the data agrees

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos migrate assess` now infers undeclared foreign keys from real join predicates the compiler has
already recovered, and measures whether each one actually holds.

This is the piece no schema-only migration tool can do, and the reason is not cleverness: EKOS has
already compiled the views, SQL and ETL of the estate into a Transformation IR. Every
`JOIN … ON a.x = b.y` in it is a developer's claim that two columns reference each other — a claim
nobody wrote into the schema.

Live-verified on a fixture built with one of each:

```
Inferred keys (2 candidate(s) from real code joins):
  ekos_fk.orders.customer_id -> ekos_fk.customers.id   (ByConstraint)  relationship_with_orphans (99.60% of values match)
        2 orphan(s) of 500 non-null values; joined in: sql/reports.sql#0
  ekos_fk.shipments.order_ref -> ekos_fk.orders.id     (ByConstraint)  not_a_relationship (0.00% of values match)
        200 orphan(s) of 200 non-null values; joined in: sql/reports.sql#1

1 inferred relationship(s) recorded as findings.
```

The code says both are joins. The data says only one is a key. Both halves are necessary, and the
second one is why the first is safe to act on.

---

## Why an undeclared relationship matters twice

It orders the load: a child cannot land before its parent. And the targets enforce **nothing** — so
whatever integrity the application was maintaining on the source's behalf is, after the migration,
maintained by nothing at all. A schema-only tool cannot see either problem, because the schema does
not mention the relationship.

---

## Implementation details worth remembering

### Resolving a join back to its tables

The IR lowers each node to a `Custom("TransformNode")` object named `<source path>:<index>`. A
`Join` node carries its `keys` plus the **node indices** of its operands — indices that are only
meaningful inside one graph. So resolution indexes nodes by `(path, index)` and walks upstream
through `FeedsInto` edges from each operand until it reaches a `Source`, which carries the real
`object_name`. The walk is bounded at eight hops: a malformed graph with a cycle would otherwise
loop, and an operand that never reaches a `Source` is skipped rather than guessed at.

### Direction is decided by evidence, not by typing order

`a JOIN b ON a.x = b.y` and `b JOIN a ON b.y = a.x` are the same claim, so candidates are keyed on
the unordered pair. Which side is the *parent* is then decided separately: a foreign key points at
something unique, so the side whose column has a primary-key or unique constraint is the parent
(`Direction::ByConstraint`). When both sides are keyed the candidate is `Ambiguous` and says so
rather than picking one.

### Four verdicts, with a band that refuses to conclude

`CleanRelationship` (100% inclusion), `RelationshipWithOrphans` (≥99% — a real relationship *and* a
data-quality finding), `NotARelationship` (<50%), and `Unclear` in between. The middle band exists so
a marginal result is reported as marginal instead of being forced into one of the two confident
answers.

`NoData` is separate: an all-null column matches nothing and misses nothing, and reporting 100%
inclusion there would manufacture a relationship out of an empty set.

---

## Knowledge Captured

**Join predicates recovered from code carry bare table names, and scoping is therefore mandatory.**
The IR records whatever the query wrote — an alias, a schema qualifier, or neither — so candidates
can only be matched on bare names. The first version queried `pg_constraint` for declared foreign
keys **unscoped**, and a declared FK on a same-named table *in an entirely different schema*
silently suppressed the finding this fixture exists to produce. Both the declared-FK and
keyed-column queries are now scoped to the schemas under migration, and `schema_in` returns `false`
for an empty list rather than degenerating into "every schema".

This is the third time the same shape has appeared in this subsystem: bare-vs-qualified names in
drift reconciliation, unscoped completeness denominators, and now this. Anywhere a bare name from
recovered code meets a live catalog, scope is part of the query, not an afterthought.

**Opening a second ledger handle inside one process deadlocks.** The fact ledger allows exactly one
writable process, and `assess_inferred_keys` opened its own store while the caller still held one.
The error message is clear — *"another writable process already holds the ledger's write lock"* —
and the cause is not, because the second opener is in the same process. Pass the handle down.

**A self-join on the same column is a tautology; on different columns it is a relationship.**
`orders.id = orders.id` is dropped. `employees.manager_id = employees.id` is a real parent-child
relationship inside one table and must survive the same filter. One test each.

**Sources are deduplicated so confidence counts places, not sightings.** Three joins of the same
pair in one file is one place, not three. A pair joined in eleven files is a stronger claim than one
joined once — and neither is proof, which is what the inclusion check is for.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-dq/src/infer.rs` | New — `JoinObservation`, `FkCandidate`, `Direction`, `InclusionResult`, `Verdict`, 14 tests |
| `ekos/crates/cli/src/commands/migrate.rs` | `harvest_join_observations`, `source_object_name`, `assess_inferred_keys`, scoped `declared_foreign_keys`/`keyed_columns`, 3 tests |
| `TODO.md` | Inferred FKs ticked; `pg_stat_statements` seeding noted as the remaining source |

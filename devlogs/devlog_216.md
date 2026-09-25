# Devlog 216 — The workload as evidence: ORDER BY that is derived, not defaulted

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`pg_stat_statements` is now harvested, parsed and used twice: to derive ClickHouse `ORDER BY` from
the columns people actually filter on, and to seed foreign-key inference with joins that exist only
in queries the running application issues.

Both were the honest gaps left at the end of devlogs 214 and 215, and both are now closed with live
evidence.

---

## `ORDER BY` stops being a default

Before, every design reported *"no query-shape evidence available for this table, so this is the
primary key rather than a derivation"*. That was honest and useless. In ClickHouse the primary key is
frequently the **worst** ordering, because it is unique and therefore useless for skipping granules
on the filters people actually run.

After running 40 queries filtering on `customer_id` and 5 on `id` against the same table:

```
  order by : customer_id, id
             derived from observed filter predicates (customer_id in 40 call(s), id in 5 call(s));
             the primary key follows for uniqueness
```

The high-frequency filter leads; the primary key follows for uniqueness. Same table, same code — the
only thing that changed is that the design can now see the workload.

## Joins the repository never contained

`assess` already inferred foreign keys from joins in the compiled Transformation IR. That covers
views, SQL and ETL **in the repository**. It cannot see a join that exists only in a query the
application issues at runtime, and `pg_stat_statements` is the only place those are visible.

Live, with one join in a repository view, one only in the workload, and one in both:

```
  ekos_fk.orders.customer_id -> ekos_fk.customers.id   seen in 2 place(s) (ByConstraint)   relationship_with_orphans (99.60%)
        joined in: pg_stat_statements, sql/reports.sql#0
  ekos_fk.customers.id -> ekos_fk.shipments.id         seen in 1 place(s) (Ambiguous)      clean_relationship (100.00%)
        joined in: pg_stat_statements
  ekos_fk.shipments.order_ref -> ekos_fk.orders.id     seen in 1 place(s) (ByConstraint)   not_a_relationship (0.00%)
```

The first gained a second source and with it more confidence. The second exists only in the workload
and would have been invisible. The third is still correctly refused. The `Ambiguous` direction on
the second is the design working as intended: both sides are primary keys, so which is the parent
cannot be determined, and it says so rather than picking.

---

## Implementation details worth remembering

### Attribution is refused when it would be a guess

A **qualified** column (`o.tenant_id`) resolves through the alias map. An **unqualified** one is only
attributed when exactly one table is in scope. With two tables in a join, `WHERE status = $1` is
dropped rather than assigned.

That rule matters more than it looks: attributing a column to the wrong table produces an `ORDER BY`
naming a column that table does not have, which the DDL emitter then refuses — and a wrong ordering
is worse than a defaulted one precisely because it *looks* derived.

### Only granule-skipping operators count

`=`, `<`, `<=`, `>`, `>=`, `BETWEEN` and `IN` are collected. `<>` and `LIKE` are not, because
ordering by a column only ever used with them buys nothing. Skipping is the entire purpose of a
ClickHouse `ORDER BY`, so the operator is part of whether a predicate is evidence.

### A join predicate is not also a filter

`ON o.customer_id = c.id` resolves on both sides, so it is a join. A filter has a column on one side
and a parameter on the other. Counting an equi-join as a filter would rank join keys above the
columns people actually filter on, which is exactly backwards.

### Implicit joins are collected too

Older application SQL is full of `FROM a, b WHERE a.x = b.y`. The `WHERE` walker collects equi-pairs
as joins as well as filters, so that idiom is not invisible.

---

## Knowledge Captured

**`pg_stat_statements` is not a complete picture, and its absence is not evidence.** The view is
capped by `pg_stat_statements.max`, is cleared by `pg_stat_statements_reset()` and by some upgrade
paths, and only exists if the extension is installed. A column absent from it may simply have aged
out. So `harvest` returns an empty list rather than an error when the extension is missing, and the
design falls back to the primary key *and says it is doing that* — absence never reads as "nothing
filters on this".

**Normalization is not redaction.** `pg_stat_statements` replaces constants with `$1`, which looks
like it removes data — but utility statements and some shapes retain literal text. Every query string
goes through RFC 0043 redaction before it is used or stored, on the same principle as every other
raw-content entry point.

**Unparseable statements are skipped, never counted.** The view holds `VACUUM`, extension-specific
syntax and truncated text. None of it is a signal, and none of it may abort the harvest — a parse
failure drops one statement, not the run.

**Weighting by call count is the entire reason to read the view.** A predicate in a query run a
million times and one in a query run twice are not equal evidence, and a design that treats them
equally is barely better than the schema-only default it replaces.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/pg-live/src/workload.rs` | New — `harvest`, `analyze`, alias resolution, filter/join extraction, `filters_for`, 12 tests |
| `ekos/crates/pg-live/Cargo.toml` | `sqlparser` |
| `ekos/crates/cli/src/commands/migrate.rs` | `map` feeds `filter_columns` from the workload; `assess` seeds FK inference from it too |
| `TODO.md` | Both honest gaps from devlogs 214/215 closed |

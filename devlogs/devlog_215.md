# Devlog 215 — RFC 0159: mapping with evidence, and DDL that carries its reasoning

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos-migrate-target-clickhouse` and `ekos migrate map`: a type registry with lossiness classes, a
target designer that derives engine, `ORDER BY`, partitioning and codecs from measured evidence, and
a DDL emitter whose output carries its own reasoning in comments.

The generated DDL was executed against live ClickHouse 24.8 and `DESCRIBE`d back — every type, every
codec and the `LowCardinality` choice round-tripped.

---

## The two failure modes this sits between

A **naive mapping** — `numeric` → `Decimal(76, 20)`, every column `Nullable`, `ORDER BY` the primary
key because that is what PostgreSQL had — is what a type-lookup table alone produces. Correct,
enormous, slow. The `ORDER BY` in particular: in ClickHouse the primary key is frequently the *worst*
ordering, because it is unique and therefore useless for skipping granules on the filters people
actually run.

An **aggressive mapping** — `numeric` → `Decimal(18, 2)` because the first thousand rows fit —
silently truncates in month three.

Both are avoided by the same thing: measured data, plus a lossiness class that states what the
measurement does and does not prove.

---

## Three rules that run through all of it

**A `NarrowingSafe` mapping cites the profile that proves it.** It is a claim about data at a point
in time, not about the schema, so the mapping carries a `profile_ref` — and a later re-profile that
breaks the bound invalidates the mapping rather than it silently outliving its evidence. A `Lossy`
mapping cites nothing, because it has nothing to cite; a test asserts that.

**A growing column is never narrowed on measured data.** An identity column, a sequence-backed id, a
monotonic timestamp: the profile describes the past, and this column's future is larger by
construction. `ColumnEvidence::growing` is wired from the profiler's monotonicity measurement, and a
growing unconstrained numeric comes back `Lossy` with the reason in its rationale. This is the guard
that stops the profiler's biggest win from becoming its biggest mistake.

**No evidence is stated as no evidence.** With no observed query shapes, the design proposes the
primary key *and says that is what it is doing*:

> `order by: no query-shape evidence available for this table, so this is the primary key rather
> than a derivation. Populate pg_stat_statements, or compile the application's queries, and re-map
> before relying on it.`

Presenting a guess as a derivation is the same failure as a validation tier reporting green with no
controls.

---

## Implementation details worth remembering

### The worse lossiness wins when two decisions combine

A column gets both a type decision and a nullability decision. `interval` → `String` is `Lossy`;
dropping `Nullable` on a profiled column is `NarrowingSafe`. Combined, the result is `Lossy` — the
worse class, not the more recent one. Otherwise a nullability win would launder a type loss.

### `ReplacingMergeTree` always emits the eventual-dedup finding

Its dedup is *eventual*: a `SELECT` before a merge still returns duplicates. That is invisible on
load day, surprises people badly, and changes how RFC 0156 must read the table forever after. So
choosing the engine always produces the finding, never just the engine.

A missing version column is a separate finding, and explicitly **not** an invented `now()` — that
would make the load non-deterministic and therefore unvalidatable.

### The partition guard refuses rather than emitting

Over-partitioning is the most common self-inflicted ClickHouse wound. A design projecting more than
1,000 partitions produces no partitioning and a finding naming the number, rather than DDL somebody
regrets. Below 10M rows, partitioning costs more than it saves and is not proposed at all.

### The emitter refuses what it knows is wrong

A table with no `ORDER BY`, or an `ORDER BY` naming a column the mapping never produced, is a
`DdlError` — not a statement sent to the server to be diagnosed there. The second case is a design
and mapping that disagree, which is a bug worth failing on.

---

## Knowledge Captured

**`LowCardinality` is only safe to offer from a measurement, and the `n_distinct` convention is what
makes the measurement trustworthy.** A unique column resolves to roughly the row count; read naively
(PostgreSQL stores `-1` for "all distinct") it looks like *one* distinct value and
`LowCardinality(String)` gets offered for a primary key. The convention is handled in
`pg_live::profile::distinct_count`, and there is a test here asserting a unique column does **not**
get `LowCardinality`.

**ClickHouse `Decimal64(S)` fixes precision at 18 — the parameter is scale alone.** So the width
choice (`Decimal32`/`64`/`128`/`256`) *is* the precision decision, and a measured precision of 18
with scale 16 leaves exactly two integer digits. `DESCRIBE` renders it back as `Decimal(18, 16)`,
which is the clearest way to see what was actually chosen.

**Emitting the reasoning as SQL comments changes who can approve the DDL.** The alternative is a
reviewer holding a report in one window and a `.sql` file in another, and in practice that means
approving the file. `rationale_comment` puts the engine choice, the ordering derivation, the
partitioning decision and every `NEEDS A DECISION` line directly above the statement they explain.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-target-clickhouse/src/typemap.rs` | New — registry, lossiness classes, growing-column guard, 11 tests |
| `ekos/crates/migrate-target-clickhouse/src/design.rs` | New — engine, `ORDER BY`, partition guard, codecs, 12 tests |
| `ekos/crates/migrate-target-clickhouse/src/ddl.rs` | New — emitter with refusals, rationale comments, 5 tests |
| `ekos/crates/cli/src/commands/migrate.rs` | `map`, `report_mapping`, `column_nullability`, `project_target` |
| `ekos/crates/cli/src/app.rs` | `ekos migrate map --emit` |
| `TODO.md` | Phase 4 mapping/design ticked; query-shape seeding and RFC 0160 remain |

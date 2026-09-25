# Devlog 213 — RFC 0158: rules that count rows, and a sample that measured nothing as zero

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos-migrate-dq` and `ekos migrate assess`: a source-independent rule catalog, ten column rules and
three table rules, each measuring **affected rows** against the live source, every finding a fact
carrying the query that produced it. Plus the completeness check that makes RFC 0154's coverage
promise mechanical.

Running it against a live 1,000-row fixture found two bugs in my own code, one of them the worst
kind: a profile that reported a money column as `precision 0, scale 0`, which RFC 0159 would have
read as a licence to map it to `Decimal(1, 0)`.

---

## The bug: an empty sample measured as zero

`profile_columns_p1` measures the numeric precision and scale a column *actually uses* — the
measurement that lets RFC 0159 offer `narrowing-safe` instead of `Decimal(76, 20)`. It sampled with
`TABLESAMPLE SYSTEM (10)` and wrapped the aggregates in `COALESCE(max(...), 0)`.

`TABLESAMPLE SYSTEM` reads whole **pages**. On a table small enough to fit in a handful of them, a
10% page sample routinely selects *no pages at all*. Observed live, four consecutive samples of the
same 1,000-row table:

```
82, 82, 82, 0
```

With the `COALESCE`, that fourth draw becomes "the data uses precision 0, scale 0", and the rule
dutifully reported:

```
amount is unconstrained numeric; the measured data uses precision 0 scale 0,
so Decimal(1, 0) is narrowing-safe against that profile
```

`Decimal(1, 0)` for a money column, labelled **narrowing-safe**. That is the exact direction in which
narrowing-safe must never be wrong — the whole class exists to say "the target is narrower *and the
data proves nothing is lost*", and here the data proved nothing at all.

The fix is `sampled_or_none`: count the sampled rows first, no `COALESCE`, and return `None` below a
row floor. The absence of a measurement then stays absent all the way to the rule, which already
handles it correctly and says *"has not been profiled. Run `ekos migrate profile --tier p1` before
choosing a target type; guessing one is how a migration truncates money."* After the fix the same
column reports precision 18, scale 16.

A two-row sample is not evidence about a million-row column either, so the floor is a floor rather
than a zero check.

## The second bug: a padding check that could never fire

`COMPAT.CH.CHAR_PADDING` measured `col::text <> rtrim(col::text)`. PostgreSQL **strips a `bpchar`'s
padding on the cast to text**, so the comparison is always false and the rule measured zero on a
fixture with 200 deliberately-padded rows.

`octet_length(col) <> octet_length(col::text)` sees the stored value on one side and the trimmed one
on the other. After the fix: 20 rows, which is exactly the number planted.

Both bugs share a shape: a measurement that returns a plausible number while measuring nothing. The
first said zero because there was no data; the second said zero because the comparison was
degenerate. Neither would have been visible without running the rule against real data and knowing
what the answer should be.

---

## Design notes

**Rules are data, and the fixture requirement is enforced.** Each rule carries an `applies`
predicate, a SQL template and an `explain`. `every_rule_in_the_catalog_has_a_fixture` fails CI on a
rule with no fixture, and every fixture has a **negative** case as well as a positive one — the half
that catches an `applies` predicate matching everything. A rule that always fires is
indistinguishable from thorough until someone reads the report.

**`Lossiness::Behavioural` is its own class and always blocks.** Losing foreign-key enforcement,
losing uniqueness, `ReplacingMergeTree`'s eventual dedup, `char(n)` padding: none corrupts a row on
load day. They corrupt the database months later, and they are the findings people skip. A
behavioural finding blocks a unit even at `Warn` severity, asserted by test.

**Measured, unmeasured and zero are three different answers.** `affected_rows` is `Option<i64>`, the
report renders `None` as "not measured" rather than as zero, and a rule that matched but affects zero
rows is labelled as the cheapest disposition there is rather than hidden. A measurement that *fails*
leaves the finding unmeasured rather than recording zero.

**The completeness check reports a denominator, never a percentage.** "1/4 accounted for.
Unclassified: 1 extension, 1 foreign_key, 1 primary_key" is actionable; "94% complete" invites
somebody to call it good enough, and the missing 6% is where the triggers are. It is scoped to the
schemas of the assessed units — an unscoped introspection would be honest about the server and
dishonest about the migration.

---

## Knowledge Captured

**`TABLESAMPLE SYSTEM (n)` can return zero rows on a small table, reliably.** It samples pages, not
rows. Any aggregate over it needs the row count beside it, and `COALESCE(agg, 0)` over a sample is a
silent-corruption switch. `TABLESAMPLE BERNOULLI` samples rows and does not have this failure mode,
at higher cost — worth reaching for when the population matters more than the speed.

**PostgreSQL strips `char(n)` padding on a cast to `text`.** `col::text` on a `bpchar` gives the
trimmed value, so every "does this have trailing spaces" check written the obvious way is always
false. `octet_length(col)` sees the stored width; `octet_length(col::text)` sees the trimmed one.

**A rule catalog needs negative fixtures more than positive ones.** A positive fixture proves a rule
can fire. Only a negative one proves it discriminates — and an over-broad `applies` predicate
produces a report that is technically complete and practically ignored.

**Findings must be able to say "I did not measure".** Collapsing unmeasured into zero is the same
mistake as the sample bug one layer up, and it is tempting because `Option<i64>` is slightly more
annoying to render. The render cost is two lines; the alternative is a report that cannot distinguish
a clean column from an unexamined one.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-dq/src/model.rs` | New — rule model, `Lossiness` incl. `Behavioural`, `Finding` |
| `ekos/crates/migrate-dq/src/catalog.rs` | New — 10 column rules, 3 table rules, all measuring affected rows |
| `ekos/crates/migrate-dq/src/completeness.rs` | New — the coverage check, per-kind gaps |
| `ekos/crates/migrate-dq/tests/fixtures.rs` | New — fixture-per-rule enforcement, positive **and** negative |
| `ekos/crates/pg-live/src/profile.rs` | `sampled_or_none` — an empty sample is not a measurement of zero |
| `ekos/crates/migrate/src/profile_facts.rs` | `FindingFact` + writer |
| `ekos/crates/migrate/src/kinds.rs`, `kir/src/custom_kinds.rs` | `MigrationFinding` kind + registry row |
| `ekos/crates/cli/src/commands/migrate.rs` | `assess`, measurement, reporting, completeness |
| `ekos/crates/cli/src/app.rs` | `ekos migrate assess` |
| `TODO.md` | Phase 3 core ticked; inferred FKs and doc-vs-data remain |

# Devlog 206 — RFC 0155: the canonical form, and the one leg that is missing

**Date:** 2026-09-24
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0155 is implemented as a new `ekos-migrate-validate` crate: the per-type canonical text form,
the row hash, bucketing, the order-independent bucket checksum, and the PostgreSQL and ClickHouse
SQL that reproduces all of it inside the engines. 41 tests, including the nine serialization-layer
planted-defect controls from RFC 0156.

**RFC 0155's acceptance criterion is not met, and this devlog is explicit about that.** The
criterion is *three-way* agreement — PostgreSQL, ClickHouse and the Rust implementation all
matching a committed literal. Only the Rust leg is proven. Nothing in this workspace can execute a
query against either engine until RFC 0157 adds a driver, so the generated SQL is a carefully
reasoned, snapshot-pinned **draft**, not verified output. The crate docs say so at the top, and
TODO.md carries it as the first thing to do once the driver lands.

Shipping it this way is still the right call: the canonical form itself is now frozen and testable,
and the controls prove the checksum catches what it claims to. But "the tests pass" and "the two
engines agree" are different statements here, and only the first is true today.

---

## What was built

| Module | Role |
|---|---|
| `value.rs` | The neutral `Value` model every engine's rows map *into*, so the canonical form has one definition rather than one per engine |
| `canon.rs` | Per-type rules, `\N` sentinel, escaping, fixed-scale decimals, row join |
| `hash.rs` | `row_hash` (md5), `prefix60`, bucketing, `BucketChecksum` |
| `dialect.rs` | The PostgreSQL and ClickHouse expressions that reproduce all of the above in-engine |
| `fixtures.rs` | The 35-value awkward-case table, in the crate so tests and future engine loaders share one copy |

Tests: `golden.rs` (frozen literals + sentinel/separator invariants), `controls.rs` (9 planted
defects + clean-run + row-order), `sql_snapshot.rs` (pinned SQL), plus 22 unit tests.

---

## Implementation details worth remembering

### The escaping is what makes the sentinel sound

`\N` marks NULL. Without escaping, a text column literally containing the two characters `\N`
hashes identically to a NULL — a false green produced by ordinary data. So every text-like value
has its backslashes doubled, and a literal `U+001F` is escaped to `\x1f` so it cannot forge a column
boundary from inside a value.

Two tests assert the property directly rather than trusting the escaping code: every fixture that
is not NULL must not render as the sentinel, and no canonical form may contain a bare separator.

### Narrowing a decimal is refused, not rounded

`canon` renders at the **approved** scale from the type mapping, not the value's own scale. When
the approved scale is *lower* than the value's, it returns an error instead of rounding.

Rounding there would be self-defeating: "truncated decimal scale" is one of the planted defects the
validator must catch, and a serializer that silently performs the truncation cannot also detect it.

### The checksum is a `(count, sum)` pair because XOR hides duplicates

XOR is order-independent and overflow-free, which makes it the tempting choice. But a duplicated row
XORs to nothing — and duplicate rows are exactly what a `ReplacingMergeTree` target makes likely,
since its dedup is eventual. `count` catches it. There is a test for precisely this.

### Empty `text` and empty `bytea` collide, and that is fine

Both canonicalize to the empty string. A column has exactly one type, so the two can never occupy
the same position in a row. Rather than leave that as an unexamined coincidence, a test asserts
there is **exactly one** collision in the fixture set and that it is this one — so a new collision
introduced later fails loudly instead of hiding.

---

## Knowledge Captured

**ClickHouse's `reinterpretAsUInt64` is little-endian; PostgreSQL's `('x' || …)::bit(60)::bigint`
is big-endian.** The naive translation of the bucket expression reads the same md5 prefix as two
different integers, so every bucket on every table disagrees. `reverse()` on the unhexed bytes
fixes it.

This is the good kind of wrong — total and immediate rather than subtle — but it is a clean example
of why RFC 0155 insists the canonical form is pinned by literals on *each* engine rather than by
comparing two engines to each other. A guarded test now asserts `reverse()` is present, with the
reasoning in the doc comment, because the next person to simplify that expression will not know.

**15 hex characters is not a whole number of bytes.** `unhex` needs an even count, so the prefix is
padded with a leading `'0'`. The padding nibble becomes the high nibble, which is also what keeps
the value under 2^60 and therefore summable in an `i64` without overflow. Both facts are one edit
away from being broken by someone tidying the expression.

**A filter that counts occurrences of its own pattern counts itself.** The control-catalogue test
used `src.matches("fn control_").count()` over its own source — and the counting line contains
`fn control_`, so it read 10 where 9 exist. Fixed by matching only lines that *start* with
`fn control_`. This project has shipped this exact bug before (the `headless.sh` act filter that
compared `act` to itself), and the lesson is the same one: a self-referential filter needs a
negative case, and `include_str!("self.rs")` tests are where it hides.

**`sumWithOverflow` is a footgun with this shape of check.** ClickHouse's default `sum` over
`UInt64` can overflow silently on a large bucket, and the "fix" people reach for is
`sumWithOverflow`, which wraps deliberately. Both produce a checksum that can match when the data
does not. The generated query uses `toDecimal128(…, 0)` and a test asserts `sumWithOverflow` never
appears.

**Floats cannot be hashed portably and saying so is better than approximating.** There is no decimal
rendering of a `double` that is both lossless and identical across PostgreSQL, ClickHouse and Spark.
Rounding to N places just moves the false green to the rounding boundary. `canon` returns
`CanonError::FloatExcluded` with a message that names the two real options: validate at V2 with a
tolerance, or map the column to `numeric` if sign-off depends on exactness.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-validate/` | New crate: `value.rs`, `canon.rs`, `hash.rs`, `dialect.rs`, `fixtures.rs`, `lib.rs` |
| `ekos/crates/migrate-validate/tests/golden.rs` | 35 frozen literals + sentinel, separator and collision invariants |
| `ekos/crates/migrate-validate/tests/controls.rs` | 9 planted-defect controls, clean-run, row-order, catalogue completeness |
| `ekos/crates/migrate-validate/tests/sql_snapshot.rs` | Pinned per-dialect SQL; `every_column_rule_is_pinned` |
| `ekos/crates/migrate-validate/tests/snapshots/` | `postgres.sql.txt`, `clickhouse.sql.txt` |
| `ekos/crates/migrate-validate/tests/print_golden.rs` | Ignored generator for regenerating the literals deliberately |
| `ekos/Cargo.toml` | Workspace member, dependency entry, `md-5` |
| `TODO.md` | Migrate Phase 1 partly ticked, with the missing engine-agreement leg called out |

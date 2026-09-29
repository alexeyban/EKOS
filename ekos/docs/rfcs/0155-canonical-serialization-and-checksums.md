# RFC 0155 — Canonical value serialization and cross-engine checksums

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (EKOS Migrate foundation), RFC 0156 (validation tiers, the first consumer),
RFC 0159 (type mapping — supplies the approved scale/precision this RFC serializes against),
RFC 0031 (`SqlDialectParser`), RFC 0150 (planted-control discipline)

---

## Summary

Defines one canonical text form per value, per type, such that the same logical row hashes
identically in PostgreSQL and in every migration target. Defines the row hash, the bucketing
function and an order-independent bucket checksum built on them.

This is the smallest, most dangerous component in EKOS Migrate. Everything above it — counts,
aggregates, bisect, divergence classification, the sign-off — inherits its correctness, and its
failure mode is a **false green**: two databases that differ, agreeing. It therefore ships before
the live connector and before any data moves, proven against hand-written fixtures.

## Motivation

Comparing two tables across engines by hashing rows is only sound if both engines produce the same
bytes for the same value. They do not, by default:

- PostgreSQL renders `numeric` as `1.50`; ClickHouse renders `Decimal(18,2)` as `1.5` in some
  contexts and `1.50` in others depending on the function used.
- PostgreSQL `timestamptz` output depends on the session `TimeZone`. Spark `TIMESTAMP` depends on
  the session timezone too, and on a calendar-rebase setting for pre-1900 dates.
- `NULL` concatenated into a string makes the whole expression `NULL` in PostgreSQL, but an empty
  string in some engine string functions — so `('a', NULL)` and `('a', '')` collide unless a
  sentinel is used.
- Floating point has no stable decimal rendering across engines at all.

Each of these silently produces *equal* hashes for unequal data, or unequal hashes for equal data.
The first kind is a false green and is unacceptable; the second is noise that destroys trust in the
tool. A specification pinned by golden tests on every engine is the only defence.

## Design

### The rules

One rule per type family. Each is a specification first and a per-dialect SQL expression second.

| Type | Canonical form | Note |
|---|---|---|
| `NULL` | the two bytes `\N` | Distinct from every value. Must not be producible by any non-null value's rendering — see *Escaping*. |
| text, `varchar` | the value, unchanged, UTF-8 | Empty string stays empty and is therefore distinct from `\N`. |
| `char(n)` | trailing spaces **preserved**, never trimmed | PostgreSQL pads; targets may not. Padding difference is a real divergence and must be reported, not normalized away. |
| `bool` | `t` / `f` | |
| integers | base-10, no leading zeros, `-` for negative, no `+` | |
| `numeric` / `decimal` | fixed scale from the **approved** `MigrationTypeMapping`, always that many fractional digits, no exponent, no trailing-zero ambiguity, `-0` normalized to `0` | The approved mapping, not the observed value, decides scale. |
| `real` / `double` | **excluded from hashing** | Validated at tier V2 with a documented relative tolerance. See *Floats*. |
| `timestamptz` | UTC, `YYYY-MM-DDTHH:MM:SS.ffffff`, exactly 6 fractional digits, no zone suffix | Zone is dropped *because* it is pinned to UTC; carrying it invites per-engine spelling differences. |
| `timestamp` (no TZ) | same form, no conversion applied | A no-TZ column is a wall-clock reading; converting it would invent information. |
| `date` | `YYYY-MM-DD` | Out-of-range values are a compatibility finding (RFC 0158), not a serialization concern. |
| `time` | `HH:MM:SS.ffffff` | |
| `interval` | months, days and microseconds as three base-10 integers joined by `:` | PostgreSQL's own three-field model; avoids normalizing 1 month to 30 days. |
| `uuid` | lowercase, hyphenated | |
| `bytea` / binary | lowercase hex, no `\x` prefix | |
| `inet` / `cidr` | canonical text form, prefix length always present | |
| arrays | `{` element `,` element `}`, elements serialized by their own rule, `\N` for null elements | Nested arrays recurse. |
| `enum`, domains | the underlying text value | |
| `jsonb` | **V2 only** in this RFC: length and top-level key count | See *JSON*. |

### Escaping

The `\N` sentinel is only sound if no real value can render as `\N`. In every rule above, a literal
backslash in a text-like value is doubled before the column separator is applied. `\N` therefore
means null and `\\N` means the two characters. This is the one place where a "nobody would ever
store that" assumption would eventually cost a false green.

### Row hash

```
row_canonical = canon(c1) || US || canon(c2) || US || … || canon(cn)
row_hash      = sha256(row_canonical)       -- 256-bit, hex (md5 until 2026-09-29, see Amendment)
```

`US` is `U+001F` (ASCII unit separator), chosen because it cannot appear unescaped in any canonical
form above. Column order is the **approved target column order** from `MigrationTargetDesign`, not
the source catalog order, so a deliberate reordering does not read as a divergence.

The hash is SHA-256. It is a comparison function here, not a security primitive, but every engine
in the matrix has it natively, so there is no portability cost to avoiding a broken hash. (This RFC
originally chose `md5` for availability; see *Amendment 2026-09-29*.)

### Bucketing and the bucket checksum

```
bucket        = prefix_60(sha256(canon(pk))) % N      -- N from policy, default 4096
bucket_count  = count(*)
bucket_sum    = sum( prefix_60(row_hash) )            -- exact integer arithmetic
```

`prefix_60` takes the first 15 hex characters (60 bits) of the hash as an integer. 60 bits fits
in a signed 64-bit integer with room for summation, and every target engine can sum it exactly —
which is the point. The pair `(count, sum)` is **order-independent**, so neither side needs to sort,
and a same-count/same-sum pair is what lets bisect skip a bucket.

Summation must not overflow or silently become floating point. Per dialect:

- PostgreSQL: `sum(x::numeric)` — arbitrary precision.
- ClickHouse: `sum(toInt64(x))` promotes to `Int64`/`UInt64` accumulators; for large buckets,
  `sumWithOverflow` is **forbidden** and `toDecimal128` is used instead.

The exact expression per dialect is generated, never hand-written at a call site, and lives beside
the rule table so a dialect addition is one place to edit.

### Floats

`real` and `double precision` are excluded from `row_hash` entirely. There is no decimal rendering
that is both lossless and portable across three engines, and a lossy one produces false greens.

Float columns are instead validated at V2 with a documented relative tolerance (default `1e-9`), and
the RFC 0162 report states, per table, which columns were validated by tolerance rather than by
hash. A migration whose sign-off depends on float exactness must map those columns to `numeric`
first — which is a `MigrationTypeMapping` decision with its own approval, not something the
validator can paper over.

### JSON

`jsonb` is validated at V2 only in this RFC: byte length and top-level key count. Key-sorted
canonical form is deliberately deferred — PostgreSQL's `jsonb` already normalizes key order and
whitespace and drops duplicate keys, the targets do not, and "canonicalize both sides" is a
semantics decision (is a reordered object the same object?) that belongs to a disposition, not to a
serializer. A follow-up may add a V3-eligible form once all three engines can produce it identically.

## Testing

**Golden fixtures, on every engine.** One fixture table per type family, containing the awkward
values: `NULL`, empty string, a string that is literally `\N`, a string containing `U+001F`, `-0`,
a decimal with trailing zeros, `1e-9`, a `char(5)` with trailing spaces, a pre-1900 date, a
microsecond-precision timestamp, a null array element, a nested array, a non-ASCII string, an
emoji, a 1 MB text value.

For each fixture the expected canonical string and the expected `row_hash` are **committed as
literals in the test file**. A test asserts the value produced by PostgreSQL, by ClickHouse and by
the Rust implementation all equal that literal. Three-way agreement against a fixed expectation is
the property; two-way agreement between two engines is not enough, because both can be wrong the
same way.

**The Rust implementation is a fourth opinion, not the oracle.** `ekos-migrate-validate` canonicalizes
in Rust for the bisect path (where rows are pulled into the validator). That implementation is
tested against the same literals, so a drift between the pushed-down SQL and the in-process Rust is
caught rather than assumed away.

**Planted controls** (the RFC 0156 suite) that specifically target this layer: truncated decimal
scale, timezone shift by one hour, `NULL` → empty string, trailing-space trimming, microsecond
precision loss, column swap between two same-typed columns. Each must change the bucket checksum. A
control that does not is a bug in this RFC, not in the validator.

## Alternatives considered

- **Sort both sides and compare streams.** Rejected: requires a total order that is itself
  collation-dependent across engines — the same problem one layer down, plus a full sort of both
  tables.
- **XOR of row hashes instead of a sum.** Order-independent and overflow-free, but a duplicated row
  cancels itself out, so exactly the duplicate-row defect that `ReplacingMergeTree` makes likely
  becomes invisible. The `(count, sum)` pair catches it.
- **MD5 instead of SHA-256.** The original choice, for availability. Withdrawn 2026-09-29: the
  portability premise was wrong (see *Amendment 2026-09-29*).
- **Let each engine cast to text with its own default.** This is what naive comparisons do, and it
  is the source of every failure listed in *Motivation*.
- **Include floats via rounding to N decimal places.** Rejected: rounding boundaries differ between
  engines, so it moves the false green rather than removing it.

## Open questions

- [ ] Default bucket count `N` — 4096 is a guess pending the first real table. It must be policy,
      and the bisect fan-out factor (RFC 0156) interacts with it.
- [ ] Is `char(n)` padding preservation right in all cases, or should a *declared* target type of
      `String` make trimming an expected, explained divergence rather than a reported one?
- [ ] Collation: binary ordering is assumed everywhere. A source using a non-binary collation for a
      PK affects bucketing only through `canon(pk)`, which is binary — confirm no ordering
      dependency leaks in.

## Acceptance criteria

- [ ] Every rule in the table has a committed golden fixture with a literal expected hash.
- [ ] PostgreSQL, ClickHouse and the Rust implementation agree with the literal on every fixture.
- [ ] Every serialization-layer planted control changes the bucket checksum.
- [ ] Dialect expressions are generated from one table, not written per call site.
- [ ] `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` clean.

## Amendment 2026-09-29 — SHA-256 replaces MD5

The row hash and the bucket hash are now SHA-256 on every side:

| Side | Expression |
|---|---|
| Rust | `sha2::Sha256`, lowercase hex |
| PostgreSQL (11+) | `encode(sha256(convert_to(x, 'UTF8')), 'hex')` |
| ClickHouse | `lower(hex(SHA256(x)))` |
| Spark SQL (future Delta target) | `sha2(x, 256)` |

**Why.** SonarCloud's security rating was D on a single MD5 finding (`rust:S4790`) in
`migrate-validate/src/hash.rs`. The original rejection of SHA-256 rested on "weaker portability",
and that turned out to be wrong: every engine above has SHA-256 natively. Suppressing the
finding would have kept a broken hash in place for no benefit.

**What did not change.** Canonical forms, the unit separator, `prefix_60` (still the first 15 hex
characters), the `(count, sum)` bucket pair, and the endianness handling in `prefix60_expr`.
Earlier validation runs compared both sides within one run, so no stored comparison mixes the two
hashes.

**Verified.** The golden table was regenerated from the implementation. With
`EKOS_MIGRATE_LIVE=1`, both sandboxes (PostgreSQL 16, ClickHouse 24.8) reproduce the Rust row hash,
the 60-bit prefix, and a new non-ASCII case (`Ünïcödé 🦀`). The non-ASCII case guards the
`convert_to(…, 'UTF8')` step, which PostgreSQL needs because `sha256` takes `bytea`.

**Found alongside.** The change exposed a flaky assertion in `bisect::mask_key`'s test
(`!mask_key("7").contains('7')`). The masked form ends in 8 hex characters of the hash, and about
40% of 8-hex-digit strings contain any given digit, so the test passed under MD5 by luck. It now
uses a key made of non-hex characters.


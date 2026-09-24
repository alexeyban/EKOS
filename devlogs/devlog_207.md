# Devlog 207 — RFC 0155 verified live: three cross-engine defects review did not catch

**Date:** 2026-09-24
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The gap devlog_206 was explicit about is closed. RFC 0155's actual acceptance criterion —
three-way agreement between PostgreSQL, ClickHouse and the Rust implementation against committed
literals — is now verified against live engines: PostgreSQL 16 and ClickHouse 24.8 from
`docker-compose.migrate.yml`. 31 cases on PostgreSQL, 29 on ClickHouse, plus the row hash and the
60-bit bucket prefix.

**Running it found three real defects in SQL that had passed review and every unit test.** Each
would have produced a wrong answer in production, and none was visible without an engine. That is
the entire argument for RFC 0155 shipping before the connector, and it is now evidence rather than
an argument.

---

## The three defects

### 1. ClickHouse parses `'\N'` in a string literal as NULL

`SELECT '\N'` returns NULL on ClickHouse — `\N` is its NULL escape inside a string literal.
PostgreSQL, with `standard_conforming_strings` on (the default since 9.1), takes the same two
characters literally.

So the generated `if(isNull(c), '\N', …)` rendered **NULL for every null column**, and every row
containing a null would have hashed differently on the two sides. The fix is a per-dialect
`null_literal()`: `'\N'` for PostgreSQL, `'\\N'` for ClickHouse.

The sentinel is the one piece of the canonical form every other rule depends on, and it was the
first thing to break.

### 2. `toString` on a ClickHouse Decimal strips trailing zeros

`toString(toDecimal128(1.50, 2))` returns `"1.5"`. The whole point of the decimal rule is a fixed
scale with no trailing-zero ambiguity, so every decimal column disagreed with PostgreSQL's
`to_char`.

`toDecimalString(x, s)` renders at an exact scale — and, checked against the fixtures, also
normalizes `-0.00` to `0.00` exactly as `canon_decimal` does.

### 3. Endianness on the bucket prefix — already fixed, now actually confirmed

devlog_206 reasoned that `reinterpretAsUInt64` is little-endian where PostgreSQL's
`('x' || …)::bit(60)::bigint` is big-endian, and added `reverse()` on that reasoning alone. Live,
both engines now return `648541476951500027` for `md5('abc')`, matching Rust and Python.

Reasoning was right, but it was only reasoning. Two of the three defects above came from places
nobody thought to reason about.

---

## A fourth finding, which is not a defect

`toDate32('1850-06-15')` returns **1900-01-01** on ClickHouse 24.8. Both `formatDateTime` and
`toString` agree on the clamped value. It does not error; it silently substitutes a different date.

That is not a canonical-form bug — it is precisely the compatibility issue RFC 0158's
`COMPAT.CH` date rules exist to measure, now with live evidence and a version number attached.
The case is recorded in `UNMAPPED_LIVE_CASES` with that reasoning rather than deleted, and the
silence is the reason the rule has to count affected rows *before* any data moves: a migration that
relies on the engine to complain about an out-of-range date gets no complaint and wrong data.

---

## How the harness is built

- **It applies `canon_expr_of` to a typed literal**, so it exercises the exact expression tiers
  V1–V3 push down. A harness that rebuilt the expression would be testing a second copy of it.
- **Every assertion is against the committed literal from `golden.rs`**, never against the other
  engine. Two engines agreeing proves nothing if both are wrong the same way — and defects 1 and 2
  were each wrong on exactly one side, so an engine-to-engine comparison would have caught them,
  while a shared misunderstanding would not.
- **Exact case counts, not floors** (31 and 29). A case silently dropping off the live path is the
  failure this file exists to prevent.
- **`every_fixture_is_either_live_or_explained`** asserts both directions: every fixture is either
  exercised on both engines or named in `UNMAPPED_LIVE_CASES` with a reason, and no entry claims a
  gap that no longer exists.
- **Skipped without `EKOS_MIGRATE_LIVE=1`**, so the default `cargo test --workspace` stays green on
  a machine with no Docker.

---

## Knowledge Captured

**ClickHouse `'\N'` is NULL, not two characters.** Any string literal EKOS generates for ClickHouse
that contains a backslash needs the backslash doubled. `'\\x1f'` was already correct by accident —
verified live as 4 characters — but the sentinel was not.

**ClickHouse's default HTTP output format escapes backslashes.** `TabSeparated` renders the
two-character string `\N` as `\\N`, so the harness compared an escaped rendering against an
unescaped literal and disagreed for a reason having nothing to do with the canonical form. Any
tooling that reads ClickHouse over HTTP and compares exact strings needs `FORMAT TabSeparatedRaw`,
or it will chase phantom mismatches.

**`toString` is not a rendering contract on ClickHouse.** It is whatever the type's default
display happens to be, and for `Decimal` that means trailing zeros are dropped. Where an exact
textual form matters, use the function that names the form: `toDecimalString`, `formatDateTime`.

**ClickHouse `Date32` bottoms out at 1900-01-01 and clamps silently.** No error, no warning, a
different date. Versioned evidence: 24.8.14.39.

**Shell out from a *test* to avoid pre-empting a design decision.** The harness uses `psql` and
`curl` rather than a driver, because choosing `tokio-postgres` vs `sqlx` belongs to RFC 0157 and is
entangled with the still-open non-`Sync` `KnowledgeStore` question. Pulling that decision forward
just to get a test running would be the tail wagging the dog. RFC 0147's no-shell-out rule is about
the *recovery* path, where determinism and offline operation are the point — it does not apply to a
gated integration test, and saying which rule applies where is cheaper than either over-applying it
or quietly ignoring it.

**The ClickHouse healthcheck in `docker-compose.migrate.yml` was wrong from the start.** Inside the
image, `localhost` resolves to `::1` first and the server does not listen on IPv6, so the container
never reported healthy while the port served fine from outside. Fixed to `127.0.0.1`. A healthcheck
that can never pass is worse than none: it makes `--wait` hang and teaches people to ignore health
status.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-validate/src/dialect.rs` | Per-dialect `null_literal()`; `toDecimalString` for ClickHouse decimals; `canon_expr_of` so the harness can render a literal; guard tests for both fixes |
| `ekos/crates/migrate-validate/src/fixtures.rs` | `LiveCase` table of per-engine typed literals; `UNMAPPED_LIVE_CASES` with a reason per gap |
| `ekos/crates/migrate-validate/src/lib.rs` | Crate docs rewritten: "what is proven" now describes verified three-way agreement, not a draft |
| `ekos/crates/migrate-validate/tests/live_engines.rs` | New — the live harness, skipped without `EKOS_MIGRATE_LIVE=1` |
| `ekos/crates/migrate-validate/tests/snapshots/*.sql.txt` | Regenerated for the two dialect fixes |
| `docker-compose.migrate.yml` | ClickHouse healthcheck uses `127.0.0.1` |
| `TODO.md` | Three-way agreement ticked; RFC 0155's acceptance criterion met |

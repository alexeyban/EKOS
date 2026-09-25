# Devlog 209 — RFC 0157: the connector, and a driver returning empty strings

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos-pg-live` connects EKOS Migrate to a real PostgreSQL: guarded read-only sessions, catalog
introspection over 23 object kinds reconciled against the server's own counts, redaction at what is
now EKOS's third raw-content entry point, and a `PgSource` implementing RFC 0156's `EngineReader` —
so the tiers run over a real driver instead of a shell-out.

It also settles the question RFC 0154 Phase 0 left open, and it does so by removing it rather than
answering it. 8 unit tests, 11 live tests, 140 test binaries green.

The session found one bug worth the whole exercise: the first row-rendering implementation returned
an **empty string for every non-text column**, silently. See *Knowledge Captured*.

---

## Decision: the synchronous `postgres` crate

RFC 0154 Phase 0 left one item open — how a chunk-parallel executor coexists with a
`KnowledgeStore` that is not `Sync`, in a CLI whose async work runs under `block_on` and is never
spawned. TODO.md carried it as "decide before writing the executor".

Choosing `postgres` (the official synchronous wrapper over `tokio-postgres`, same authors) makes the
question disappear rather than answering it:

- `EngineReader` is already a synchronous trait — that was the right shape for a read seam and it
  stays right. A session implements it directly, with no runtime and no bridging.
- When RFC 0160 needs chunk parallelism, the answer is a pool of independent sessions on separate
  threads, each owning its own connection, results collected before anything touches the store. That
  is a better shape than sharing one async client, and it keeps the ledger's thread-safety nobody
  else's problem.

`sqlx`'s compile-time query checking buys nothing here, because every query EKOS Migrate issues is
generated at runtime from a dialect table.

---

## What was built

| Component | Role |
|---|---|
| `session.rs` | `PgSource`, `SessionPolicy`, the write probe, LSN and replica-lag readers |
| `catalog.rs` | 23 `ObjectKind`s, `introspect`, `reconcile`, redaction at the entry point |
| `tests/live_catalog.rs` | A fixture schema with **one object of every kind**, plus the redaction and reconciliation tests |

---

## Implementation details worth remembering

### The write probe has to defeat its own session

The first version ran `CREATE TEMP TABLE` inside a transaction and reported "refused" — on a role
that owns the database. Of course it did: the session sets `default_transaction_read_only = on`, so
*every* role looks safe and the probe reports on the setting rather than on the role.

That is worse than not probing, because it manufactures confidence in the wrong thing. A session
setting is one `SET` away from being undone; the guarantee EKOS actually wants is a role without
write privileges. The probe now sets `transaction_read_only = off` for the duration, inside a
transaction that is always rolled back, so a refusal means the *role* cannot write. The sandbox's
owner role correctly reports `RoleCanWrite`, which is a finding.

### The catalog reconciles against a different query shape

`reconcile` counts relations and routines with queries structured differently from the ones that
built the object list. The enumeration could be wrong in an invisible way — a `JOIN` that drops
rows, a filter subtly too narrow — and a short catalog silently shrinks RFC 0158's completeness
denominator, which is the number RFC 0154's whole coverage guarantee rests on. A mismatch is an
error, never a smaller list.

For the same reason there is no `ObjectKind::Other` and no wildcard arm that skips: an unknown
`relkind` (a foreign table, on a source using `postgres_fdw`) raises `UnknownRelkind`. A test asserts
that match has no `continue`.

### `NOT VALID` constraints are carried as such

A `NOT VALID` foreign key is present in the catalog and is *not* a guarantee — the rows that existed
when it was added were never checked. Recording `validated: false` is what lets RFC 0158's
referential-integrity family know it must actually look for orphans rather than trusting the
constraint.

---

## Knowledge Captured

**`client.query()` silently returned an empty string for every non-text column.** The first
`raw_query` decoded each column as `Option<String>` and swallowed the decode error with
`.ok().flatten().unwrap_or_default()`. PostgreSQL's *extended* query protocol negotiates a binary
format per type, so an `int8` handed to a `String` decoder is simply refused — and the swallow
turned that refusal into `""`. `SELECT 1, 'two'` returned `["", "two"]`; every `count(*)` returned
nothing; `reconcile` compared 0 against 0 and passed.

It failed loudly only because a test asserted an exact value. Had the tests compared a table with
itself — which a self-comparison test naturally does — both sides would have mangled identically and
the suite would have been green on a driver that could not read numbers.

The fix is the **simple query protocol** (`simple_query`), which returns every value in its text
form. That is also the form RFC 0155's canonical expressions are written against, so there is now
one rendering rather than two. Two consequences, both documented on the method: NULL comes back as
an empty string, which is safe only because every query this crate issues renders NULL explicitly
server-side; and the simple protocol permits multiple statements per message, which is another
reason RFC 0160's classifier is the control for anything built from text.

The general lesson: `.ok()` on a decode is a silent-corruption switch. And a self-comparison test
cannot detect a symmetric bug — it needs at least one assertion against a value known from outside
the system.

**`postgres::Error`'s `Display` is the string "db error".** The server's message lives on
`Error::source()`. Rendering only the outer error throws away the one part a human needs and the one
part a test can assert on. `describe()` joins both.

**`Once` around a fixture whose DDL starts with `DROP SCHEMA … CASCADE`.** Cargo runs tests in
parallel, so five tests each dropping and recreating the same schema is a lock fight that fails for
reasons unrelated to what any of them tests. The failure is confusing because it points at the
fixture helper, not at the race.

**A fixture with one object of *every* kind is the only way to test a coverage guarantee.** The
catalog test creates an enum, a domain, a range type, a partitioned table and its partition, a
matview, an exclusion constraint, an RLS policy, a procedure, a trigger and a `NOT VALID` foreign
key, then asserts each kind came back non-empty. A kind nobody puts in a fixture is a kind nobody
knows is missing.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/pg-live/` | New crate: `session.rs`, `catalog.rs`, `lib.rs` |
| `ekos/crates/pg-live/tests/live_catalog.rs` | 11 live tests: session safety, write probe, every object kind, reconciliation, redaction, tiers over the real driver |
| `ekos/Cargo.toml` | Workspace member, dependency entry, `postgres = "0.19"` |
| `TODO.md` | Phase 2 partly ticked; the Phase 0 concurrency question closed with its answer |

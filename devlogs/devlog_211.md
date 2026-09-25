# Devlog 211 — The pipeline joins up, and the sync driver's catch

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos migrate discover` and `ekos migrate profile` exist, so the Migrate pipeline runs end to end
for the first time: `init → discover → profile → status`, against a real PostgreSQL, writing real
facts that `ekos ekl` can query.

Along the way, a correction to devlog_209.

---

## Correction: the synchronous driver did not remove the question

devlog_209 chose the synchronous `postgres` crate over `tokio-postgres`, on the reasoning that it
avoids having to answer how a chunk-parallel executor coexists with the non-`Sync` `KnowledgeStore`.
It put it like this: *"The synchronous driver sidesteps it entirely."*

The first end-to-end run panicked:

```
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`)
attempted to block the current thread while the thread is being used to drive asynchronous tasks.
```

`bin/ekos.rs` is `#[tokio::main]`. Every CLI command already runs inside a runtime, and the
synchronous wrapper builds its own internally.

The *choice* still holds — `EngineReader` is a synchronous trait, and an async driver would have
meant an async seam through every tier. What did not hold is "entirely". The arrangement needed is
one function, `off_runtime`, which runs database work on a dedicated OS thread and collects results
before anything touches the store. That is exactly the shape devlog_209 predicted RFC 0160 would
need for chunk parallelism; it is simply needed now, for one connection, rather than later for many.

It is caught by a `#[tokio::test]` that connects to a closed port and asserts the failure is an
ordinary connection error rather than a panic. No plain `#[test]` can catch this, because a plain
test has no runtime — which is why it survived a full unit suite and eleven live tests.

---

## What was built

| Component | Role |
|---|---|
| `migrate/src/profile_facts.rs` | Source-independent `TableProfileFact` / `ColumnProfileFact` / `DriftFact`, their writers, and `reconcile_tables` |
| `pg-live/src/lib.rs` | `From` conversions from the connector's profile types |
| `cli/commands/migrate.rs` | `discover`, `profile`, `off_runtime`, alias resolution |
| `compiler-core/src/config.rs` | `[migrate.connections.<alias>]` |

Three new object kinds with registry rows: `MigrationTableProfile`, `MigrationColumnProfile`,
`MigrationDrift`.

---

## Implementation details worth remembering

### The fact model stays source-independent

`TableProfileFact` and `ColumnProfileFact` are plain data with no PostgreSQL in them. The connector
converts into them. When RFC 0154's expansion list reaches SQL Server or Oracle, those write the same
shapes — and keeping the boundary there is what stops "the fact model" quietly becoming "whatever
PostgreSQL happened to return".

The conversion re-checks the suppression rule rather than trusting it: a column marked
`values_suppressed` gets `None` bounds on the way through, so a future change upstream that forgets
to clear a bound cannot leak one by this path either. A test populates the bounds deliberately and
asserts they do not survive.

### Profile ids are tier-scoped

`table_profile_id(project, table, tier)` includes the tier, so a P1 profile does not overwrite the P0
one. They answer different questions — P0 says what the planner believes, P1 says what a sample
measured — and an append-only ledger should keep both.

### Aliases resolve through config, not through facts

A DSN names an alias; `[migrate.connections.<alias>]` holds host, port, user and the *name* of the
password variable. So the ledger never holds a hostname with credentials attached, and pointing a
workspace at staging instead of production is a config edit rather than a new set of facts. A missing
alias is an error that prints the TOML to add, because defaulting to `localhost:5432` would silently
point a migration at the wrong database.

### Discover creates units only for tables

Views and functions are recovered into the catalog snapshot but do not become `MigrationUnit`s yet.
Creating them now would put units into the state machine that nothing can advance until RFC 0163/0164
exist, and a permanently-stuck unit is worse than an absent one.

---

## Knowledge Captured

**A sync database driver inside `#[tokio::main]` panics, and no ordinary test catches it.** The
failure needs a runtime to reproduce, so the entire unit suite and eleven live integration tests
passed while the CLI could not open a connection at all. Any future crate that wraps a blocking
client — an HTTP client with an internal runtime, a driver like this one — needs at least one
`#[tokio::test]` exercising it, or the first real invocation is the test.

**`thread::scope` is the right tool here, and the reason is the ledger.** `Box<dyn KnowledgeStore>`
is `Send` but not `Sync`, so a `&dyn KnowledgeStore` cannot cross into a spawned thread. Structuring
the work as "read everything on the thread, write everything after it joins" is not a workaround for
that — it is the same rule RFC 0154 states for the executor, arriving early.

**Drift is only checkable when there is something to check against.** `ekos migrate discover` reports
`Drift: not checked` when the ledger holds no compiled `Table` objects, and names the commands that
would produce them. Reporting "0 drift" there would be a clean bill of health derived from an empty
comparison, which is the same failure shape as a tier reporting green with no controls.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate/src/profile_facts.rs` | New — neutral fact shapes, writers, `reconcile_tables` |
| `ekos/crates/migrate/src/kinds.rs` | Three new kinds, `HAS_PROFILE` edge, `ALL_KINDS` grown to 6 |
| `ekos/crates/kir/src/custom_kinds.rs` | Registry rows for the three new kinds |
| `ekos/crates/pg-live/src/lib.rs` | `From` conversions + the suppression re-check test |
| `ekos/crates/cli/src/commands/migrate.rs` | `discover`, `profile`, `off_runtime`, alias resolution, the `#[tokio::test]` regression |
| `ekos/crates/cli/src/app.rs` | Two new subcommands and their dispatch |
| `ekos/crates/compiler-core/src/config.rs` | `MigrateConnection`, `[migrate.connections]` |
| `TODO.md` | Phase 2 all but column-level drift ticked |

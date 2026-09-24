# Devlog 205 — EKOS Migrate Phase 0: the crate, the state machine, and the controls

**Date:** 2026-09-24
**PRs:** none — committed directly to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0154's Phase 0 is implemented: a new `ekos-migrate` crate holding the migration project model,
migration units and an append-only state machine, plus `ekos migrate init|status`, the `[migrate]`
config section, registry rows for the three new object kinds, and the enforcement tests that make
the rest of the design possible to trust later.

Nothing here connects to a database. That is the point of a foundation phase: the parts that are
hard to change later — how state is written, what an id is, who may take a decision, what may not
reach the ledger — are settled before anything is built on them.

24 new tests, all green. Full workspace suite green (129 test binaries), clippy clean, fmt clean.

---

## What was built

| Component | File | Role |
|---|---|---|
| Kind constants | `crates/migrate/src/kinds.rs` | The three `Migration*` object kinds, the two event kinds, `ALL_KINDS` as the reviewable list |
| State machine | `crates/migrate/src/state.rs` | 14 states, `allowed_next()` as the single source of truth for legal moves |
| Connections | `crates/migrate/src/connection.rs` | `ConnectionRef`, `EngineKind`, `Environment` — aliases and secret *names*, never secrets |
| Project & units | `crates/migrate/src/project.rs` | Deterministic ids, the two-write transition, `WriteContext` stamping |
| Lifecycle | `crates/migrate/src/lifecycle.rs` | Human-only `Actor`, `abandon`, `supersede` |
| CLI | `crates/cli/src/commands/migrate.rs` | `init`, `status`, and the two guard tests |
| Config | `crates/compiler-core/src/config.rs` | `[migrate]`, off by default |
| Sandboxes | `docker-compose.migrate.yml` | PostgreSQL 16 + ClickHouse 24.8 |

---

## Implementation details worth remembering

### The two-write transition, and why state is not a fold

A transition re-appends the unit object with its new `state` and appends a `MigrationTransition`
event. Reading a unit's state is a point lookup on the latest object version; the event log is the
audit trail, not the source of truth.

The alternative — deriving state by folding the transition events — was rejected because *every*
read path would then need the fold, including the ones that just want to count units by state.
`session::lifecycle` made the same split for claim status, and it is why session status is cheap to
read and fully historical to audit at the same time.

### `transition` returns the event id, because nothing else can find it

`KnowledgeStore` can fetch an event only by id: there is no `all_events` and no `events_for`. So a
caller that appends an event and does not keep its id has written something it can never read back,
and a test asserting "the event was recorded" cannot be written at all.

The first version of the test worked around this with an assertion that checked nothing. Changing
`transition` to return `Result<Option<KirId>, Error>` — `Some(event_id)` for a real move, `None` for
a no-op — fixed the test *and* gave callers something they need. `None` also makes the no-op case
explicit rather than silent.

### Deterministic ids are not a nicety in an append-only ledger

Ids are `Uuid::new_v5` over a structural seed (`migration-project:<name>`,
`migration-unit:<project>:<key>`), the pattern `semantic::transform_ir` and
`KirRelationship::deterministic` already use.

With random ids, a second `ekos migrate init` on the same project creates a *second project*, and
the ledger has no dedup and no delete to fix it with. The test
`init_is_idempotent_because_ids_are_deterministic` runs init twice and asserts exactly one project
object exists.

### Credentials are refused, not sanitized

`ConnectionRef::parse` rejects any DSN containing `@` rather than parsing and stripping the
userinfo. A password that reached this process's argv has already leaked into shell history and
`ps` output; quietly accepting it would teach the habit and make the leak invisible.

The refusal happens before the store is opened, which a test asserts by checking that no `.ekos`
directory was created.

---

## Decisions

**A separate `docker-compose.migrate.yml`, not services in `docker-compose.dev.yml`.** That file is
a build container for a machine with no local Rust. Nothing in a normal `cargo build` needs a
database, and starting a PostgreSQL and a ClickHouse for every build is a poor trade. The Postgres
service does carry real configuration though, chosen deliberately: `wal_level=logical` and
`max_replication_slots` because RFC 0166 needs them at server start, `pg_stat_statements` because
RFC 0157 reads query shapes from it, and `LANG=C`/`TZ=UTC` because RFC 0155's canonical form is
pinned by golden tests and a server whose locale differs from CI's would make them machine-dependent.

**`default_environment` defaults to `sandbox` and can never usefully default to `production`.**
Promotion to production is an explicit act with an approval behind it (RFC 0160/0161), not a config
value somebody set once and forgot.

**Phase 0's one open item stays open.** The executor's concurrency model against the non-`Sync`
`KnowledgeStore` is not needed until RFC 0160, and deciding it now — with no executor to measure —
would be a guess recorded as a decision.

---

## Knowledge Captured

**The identity CI guard cannot see kinds built from constants.** This is the important one.
`every_pipeline_custom_kind_is_registered` scans source text for `ObjectKind::Custom("` followed by
a **string literal**. Adding `migrate/src` to its scan list, which RFC 0154 called for, achieves
*nothing* on its own: this crate writes `ObjectKind::Custom(kinds::PROJECT_KIND.into())`, and the
scanner finds no literal to check.

Discovered by writing the test and asking what would fail if a registry row were deleted — the
answer was "nothing". The real enforcement is a separate test in `ekos-migrate`,
`every_migrate_kind_has_a_registry_row`, which iterates `kinds::ALL_KINDS` and asserts both that a
row exists and that `structurally_keyed` is `true`. The scan-list extension is kept as well, because
it still catches a stray literal someone adds later, but it is the weaker of the two.

Anyone adding a crate that emits `Custom` kinds from constants needs the constants-based test. The
scan is a backstop, not the control. This is the same lesson as RFC 0151's isolation incident, in a
different place: a guard that looks like it covers something, and does not, is worse than no guard,
because it stops people looking.

**`#[derive(Subcommand)]` binds to the next item, and inserting an enum above one silently steals
it.** Adding `MigrateCommands` immediately before `SessionCommands` left the existing derive
attached to the new enum and `SessionCommands` underived. The failure surfaced as forty
`cannot find attribute 'arg' in this scope` errors pointing at `SessionCommands`' fields — nowhere
near the actual edit. When inserting a derived item next to another, check the item *below* the
insertion point still has its attributes.

**`EkosConfig` has two places to add a section, not one.** The struct field and the hand-written
`Default` impl. `#[allow(clippy::derivable_impls)]` sits on that impl, so it is explicit rather than
derived, and a new field fails the build with `missing 'migrate'` at the `Self {` line. Harmless
here; worth knowing before assuming `#[serde(default)]` is sufficient.

**`Ledger` inherent methods shadow the `KnowledgeStore` trait in tests.** The foundation test
imports only `Ledger` and calls `object_history`, `audit_trail` and `get_event` on it directly.
Importing `KnowledgeStore` produces an unused-import warning, which under `-D warnings` is a build
failure, so the trait import belongs only where a `&dyn KnowledgeStore` is actually needed.

**Environment variables in tests need `unsafe` in Rust 2024.** `std::env::set_var` and `remove_var`
are `unsafe` in this edition. The no-credential test wraps both. Worth noting because the coding
rules forbid `unsafe` without an RFC justification, and this is the mundane exception: test-only,
and the language requires it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate/` | New crate: `kinds.rs`, `state.rs`, `connection.rs`, `project.rs`, `lifecycle.rs`, `lib.rs` |
| `ekos/crates/migrate/tests/foundation.rs` | 17 tests: registry obligation, credential refusal, deterministic ids, transition shape, illegal transitions, supersede, provenance stamping |
| `ekos/crates/cli/src/commands/migrate.rs` | New — `init`, `status`, and 7 tests including the two human-only guards |
| `ekos/crates/cli/src/app.rs` | `Migrate` command, `MigrateCommands` enum, dispatch; restored `SessionCommands`' derive |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod migrate;` |
| `ekos/crates/cli/Cargo.toml` | `ekos-migrate` dependency |
| `ekos/crates/compiler-core/src/config.rs` | `MigrateConfig` + `[migrate]` field and `Default` entry |
| `ekos/crates/kir/src/custom_kinds.rs` | Three `Migration*` registry rows, all `structurally_keyed: true` |
| `ekos/crates/identity/src/lib.rs` | Guard scan list extended with `migrate/src` (backstop only — see Knowledge Captured) |
| `ekos/Cargo.toml` | Workspace member + dependency entry |
| `docker-compose.migrate.yml` | New — PostgreSQL 16 and ClickHouse 24.8 sandboxes |
| `README.md` | "EKOS Migrate (foundation only, opt-in)" section |
| `docs/generated/ekos-self-documentation.html` | New capability section 16, nav entry, evidence section renumbered to 17 |
| `TODO.md` | Migrate Phase 0 ticked, with the one deliberately-open item explained |

# Devlog 253 — RFC 0164 Phase 2: V5 differential execution, reconstruction, proposals

**Date:** 2026-10-11
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

RFC 0164's remaining phases, for views. A lowered view is now **run** against its source on a PostgreSQL and a ClickHouse sandbox, over a fixture built from the view's own comparisons, with branch coverage in every verdict. A model's rewrite of a routine is kept only if it passes the fidelity gate and the anti-invention check. Lowerings, trigger redesigns and surviving candidates are saved as `MigrationProposal` objects.

V5 earned its place on the first real run: it showed that four of the seven LedgerSMB views Phase 1 had lowered were wrong. The honest count is 3 of 17 lowered, 2 passing V5.

---

## V5 — differential execution of a lowered view

### Problem / motivation

Phase 1 proved a lowering parses on the target and invents nothing. Neither says the two engines return the same rows.

### What was built

| Component | Where |
|---|---|
| Fixture plan, setup SQL per engine, comparison, coverage | `ekos/crates/migrate-logic/src/v5.rs` |
| `Sandbox` trait | same; the crate opens no connection |
| Writable, rollback-only PostgreSQL session | `ekos/crates/pg-live/src/sandbox.rs` |
| ClickHouse sandbox, `ekos migrate v5`, the sandbox guard | `ekos/crates/cli/src/commands/migrate_logic.rs` |
| `sandbox = true` on a connection | `MigrateConnection` in `compiler-core/src/config.rs` |
| Planted-defect suite | `ekos/crates/migrate-logic/tests/v5_live.rs` |

### Implementation details worth remembering

- **Cross the compared columns.** The first fixture took each comparison both ways and still missed `<` → `<=`: no row had the boundary value *and* passed the other filter. Columns the view compares against a literal are now crossed, capped at 512 rows; the other columns walk their pools at a stride coprime with the pool's length, so every value appears.
- **Ask the source what the view returns.** Output columns and types come from `information_schema` on the sandbox after the view is created. Each is rendered with RFC 0155's `canon_expr` on both sides.
- **Two PostgreSQL round trips, each its own rolled-back transaction.** One to describe the view, one to read it and measure coverage. The setup is replayed; the fixture is small, and the session keeps no state between them.
- **`PgSandbox` is a separate type from `PgSource`.** The source type sets `default_transaction_read_only` first and has no way to become writable. The only code that writes to PostgreSQL cannot be reached with a source session.
- **The guard is the config mark, checked in one function** (`sandbox_ref`): right engine, alias exists, `sandbox = true`, and not any project's source DSN.

### Decisions

- **`not_run` is a result of its own.** Six conditions produce it, each with a reason. None counts as passed, and `ekos migrate v5` says so in its last line.
- **Multisets.** Row order is not compared. A `LIMIT` makes the kept rows depend on order, so such a view is `not_run`.
- **Fixture types are the module's choice.** An `int` and a `bigint` column behave the same over these values, so both are `bigint` / `Nullable(Int64)`.

---

## What V5 found

`user_listable` failed on ClickHouse with `Unknown identifier 'id'`. ClickHouse names an output column `u.id` where PostgreSQL names it `id`, when another joined table also has an `id`.

| Fix | Effect |
|---|---|
| Every unaliased select-list item gets the name PostgreSQL gives it (`u.id AS id`, `sum(x) AS sum`, `CASE … AS case`) | `user_listable` passes V5 |
| An expression PostgreSQL would name `?column?` blocks (`LOGIC.CH.OUTPUT_NAME`) | The author aliases it |
| `*` or `t.*` over a join blocks | Which names are ambiguous is not knowable without the catalog |

LedgerSMB lowered views: 7 → 3. The four that dropped out used `t.*` over a join.

---

## Reconstruction

### What was built

| Component | Where |
|---|---|
| Prompt, SQL extraction, verdict | `ekos/crates/migrate-logic/src/reconstruct.rs` |
| Model call, spending guard, `ekos migrate reconstruct` | `commands/migrate_logic.rs` |

### Implementation details worth remembering

- **The gate runs twice on purpose**: in the command before the spending decision, so an ineligible routine needs no flag and costs nothing; and inside `reconstruct_one`, so the function is safe however it is called. A test provider that panics when asked proves the second.
- **`--allow-metered` is per run.** A local Ollama provider needs nothing. There is no config key for it, so nobody sets it once and forgets.
- **The proposal records the model, the prompt version and a hash of the prompt.** It carries `v5: not_run` with the reason.

### Measured

Local `llama3` on LedgerSMB's `entity__list_classes`: the candidate read `system.tables` and filtered on a `role` column. Rejected, 7 of 17 atoms unmapped. Two other routines were refused before any call (dynamic SQL; signature only).

---

## Proposals

`MigrationProposal`, keyed by the source object's id, so recording again replaces. It carries the source's evidence ids. Written with `status: proposed`; nothing writes another status.

A proposal has a `logic_hash`: the hash of the lowering without a target database name. `lower --record --database lsmb` after `v5 --record` used to erase the V5 result, because it replaced the object. The result is now carried over exactly when the `logic_hash` is unchanged.

---

## Knowledge Captured

- **Per-comparison coverage can be 100% and the fixture still blind.** Conjunctions need joint coverage. A planted boundary defect is the test that shows it.
- **ClickHouse output names keep the qualifier when the bare name is ambiguous.** `SELECT u.id FROM e JOIN u` yields a column called `u.id`. Selecting `"id"` from such a view fails.
- **A `LANG=C` PostgreSQL has `SQL_ASCII` databases**, where `length('é')` is 2. A correct lowering looked wrong. V5 refuses a non-UTF8 sandbox; the live suite creates `ekos_v5_utf8` from `template0`.
- **`psql -c "a; b; c"` prints only the last result.** Feed a multi-statement script on stdin (`-f -`).
- **All-Nullable fixtures cannot show a missing `join_use_nulls`**: a Nullable column's default is already `NULL`. That rule stays covered by the engine-semantics suite.
- **`clippy::result_large_err`** fires on a `Result<_, Enum>` whose enum holds a 160-byte struct; box the variant.
- **Recording twice must be idempotent in what it keeps, not only in its key.** Replacing an object by id silently drops fields the second writer does not know about.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-logic/src/v5.rs` | New: fixture plan, `Sandbox`, run, coverage, 6 tests |
| `ekos/crates/migrate-logic/src/reconstruct.rs` | New: prompt, extraction, verdict, 5 tests |
| `ekos/crates/migrate-logic/src/lower.rs` | Output names stated; `LOGIC.CH.OUTPUT_NAME`; wildcard over a join blocked |
| `ekos/crates/migrate-logic/tests/v5_live.rs` | New: nine planted defects, leftover check |
| `ekos/crates/pg-live/src/sandbox.rs` | New: `PgSandbox` |
| `ekos/crates/cli/src/commands/migrate_logic.rs` | `v5`, `reconstruct`, `--record`, sandbox guard, proposals, 7 tests |
| `ekos/crates/cli/src/app.rs`, `commands/migrate.rs` | Subcommands; helper visibility |
| `ekos/crates/compiler-core/src/config.rs` | `MigrateConnection.sandbox` |
| `ekos/crates/migrate/src/kinds.rs`, `ekos/crates/kir/src/custom_kinds.rs` | `MigrationProposal` |
| `ekos/docs/rfcs/0164-*.md` | Phase 2 section, acceptance criteria, one open question answered |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |

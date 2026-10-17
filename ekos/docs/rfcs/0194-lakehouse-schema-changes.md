# RFC 0194 — EKOS Lakehouse M3: source schema changes

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-17
**Builds on:** RFC 0190 §5 (the nine change classes), RFC 0192 (bronze), RFC 0193 (silver)
**Code:** `ekos-lakehouse`, crate `changes` (pure) and `lakehouse-ext`

## Goal

A change in the source is detected before the next load, classified by what it can break, and
either applied (safe classes) or proposed for a person to approve, with its impact. Exit
(RFC 0190): every class is exercised on the sandbox and produces its documented action.

## Design

### 1. Detection: the live plan against the deployed one

`lakehouse sync` plans the live source (RFC 0192) and diffs it against `lakehouse.lock.json`
on the pipeline repository's checked-out branch — "the last deployed snapshot". Identity is
PostgreSQL's, not the name: a table is its OID, a column its `attnum`. A rename keeps both, so
a rename is *known*, never guessed from a drop plus an add (lock format 3 records them).

### 2. Classes, actions, policy

| Class | Detected as | Default | While pending |
|---|---|---|---|
| `new_table` | an OID not in the lock | auto | — |
| `new_nullable_column` | a new `attnum`, nullable, no default | auto | — |
| `widening` | int→bigint, real→double, decimal precision up (same scale) | auto | — |
| `new_required_column` | a new `attnum` that is NOT NULL or has a default | propose | bronze continues on the old schema |
| `rename` | same `attnum` (column) or OID (table), new name | propose | bronze paused (its SELECT names the old column) |
| `drop` | a deployed `attnum` / OID gone | propose | bronze paused |
| `narrowing` | any other type change | propose | bronze paused |
| `key_change` | primary key columns differ | propose | bronze continues; silver frozen |
| `routine_change` | the proof of an incremental watermark (RFC 0192 §3) is gone or changed | propose | bronze continues; the table is reconciled every run as always |

`[extensions.lakehouse.policy]` overrides a class's action (`rename = "auto"`). A refused table
(RFC 0192 §2) is a finding, not a change.

### 3. Applying a safe change

The generated files are regenerated from the new plan and committed on the checked-out branch.
A full-mode table needs nothing more (`CREATE OR REPLACE` lands the new shape). An incremental
table gets `bronze/<schema>/<table>/schema.sql` — idempotent DDL (`ADD COLUMN IF NOT EXISTS`,
`ALTER COLUMN … SET DATA TYPE`) the orchestrator runs before each load. Loads name their
columns (`INSERT INTO t (…) SELECT …`), so a table with columns the source no longer has, or
not yet, never takes a positional insert.

### 4. Proposing a change

A branch `ekos/change/<id>` (id: the change's content hash) built in a separate `git worktree`
— the checkout the orchestrator reads is never switched — holding the regenerated files and
`proposals/<id>.md`: the class, the evidence (catalog facts), and the **impact**: every silver
model, column and test that reads the changed bronze column, from the compiled ledger (RFC 0193
§6). With `git-remote` set the branch is pushed and a pull request opened (`gh`). A person
merges it; nothing in EKOS merges a proposal.

### 5. Paused and pending

`sync` writes `.lakehouse/status.json` (not committed): each table's state — `active`,
`pending` (a proposal is open; loads continue), `paused` (a proposal is open; loads skipped) —
with the proposal it waits on. The orchestrator reads it. Merging the proposal makes the
deployed lock match the source again; the next `sync` finds nothing to change and the table is
active. Every decision is a `LakehouseChange` fact.

## Found while building it (2026-10-17)

- **No-op DDL is free and idempotent** on Trino's Iceberg connector (measured): `ADD COLUMN IF
  NOT EXISTS` on an existing column and `SET DATA TYPE` to the current type write no new
  metadata version. So `schema.sql` is derived from the plan alone — no migration history.
  Iceberg accepts int→bigint, real→double and decimal precision up, and refuses narrowing.
- **Proposals would conflict with each other**: each regenerates the shared lock and manifest.
  EKOS owns its proposal branches (`ekos/change/<schema.table>/<id>`): a branch whose base is no
  longer HEAD is rebuilt (its content is generated, so rebuilding is exact), one superseded by a
  new set of changes for its table is deleted, and branches of tables with no open change are
  removed. With a remote, rebuilt branches are force-pushed. Push review commits elsewhere.
- **A rename of a key column must not read as a routine change**: the routine comparison looks
  at the watermark's proof only (the trigger and its function body's SHA-256).
- **Impact through masks**: a masked silver column is computed, so the ledger's lineage does not
  list it; impact also reports a model's same-named column when the model reads the table.
- A table added by a safe change gets its Dagster asset when the code location reloads;
  everything else (SQL, schema, pause) is read from disk at run time.

## Out of scope

V5 differential runs for routine changes (RFC 0190 names them; bronze reads base tables, so the
routine that matters here is a watermark's trigger), partition-spec evolution, CDC.

## Tests

Pure: every class from a pair of plans, identity by `attnum`/OID, policy overrides, the
proposal text. Live: one sandbox database driven through all nine classes by real DDL, each
checked for its action, then approvals merged and loads reconciled green.

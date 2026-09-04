# Devlog 160 — RFC 0135 Part B: ledger entry provenance (`audit_trail`)

**Date:** 2026-09-04
**Branch:** `rfc/0135-part-b-ledger-provenance` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0135-core-provenance-and-determinism-foundations.md` (Part B of 4)

---

## Summary

`LedgerEntry` recorded *when* a write happened (`written_at`) but never *which pipeline run*
produced it — RFC 0004's original design called for a `source_artifact_id` per entry plus an
`audit_trail(id)` reader; it was never built (`TODO.md` Phase 9, confirmed by grepping the live
source).

Part B closes it without a storage-format version bump:

- **`ekos_ledger::provenance::WriteContext { run_id, stage, source_artifact_id }`** is set on a
  `KnowledgeStore` handle (`set_write_context`, default no-op) and stamped onto every subsequent
  write until changed.
- **SQLite** records it in three nullable `entries` columns added on open
  (`ALTER TABLE … ADD COLUMN`, idempotent, no `user_version` bump — additive is enough).
- **FactLedger** appends it to a `<root>/provenance.jsonl` sidecar keyed by transaction id — no
  segment-format change (RFC §6.2's cleaner option).
- **`KnowledgeStore::audit_trail(id)`** returns the write history of one entity with each write's
  provenance. Surfaced as `ekos ledger audit <id> [--json]` and the read-only `ekos_audit` MCP
  tool.
- **`ekos commit`** stamps `(run_id, "commit"|"commit:rollup"|"commit:lineage"|
  "commit:llm-description", ckm-content-hash)`; **`ekos build`** stamps `(run_id, "build",
  observation ArtifactId)` per `File` object.

Verified end to end against this repo: `ekos ledger audit <ekos_ledger::Ledger id>` shows its 4
real versions (all 2026-08-26, all `None` provenance — written before this RFC, correctly).

---

## PR — Part B

| File | Change |
|---|---|
| `ekos/crates/ledger/src/provenance.rs` | **New.** `WriteContext`, `AuditRecord`, `new_run_id()` (`run-<unix>-<8hex>`) |
| `ekos/crates/ledger/src/lib.rs` | `pub mod provenance`; `KnowledgeStore::{set_write_context (default no-op), audit_trail (default empty)}` + macro delegation; `Ledger.write_ctx: RefCell<Option<WriteContext>>`; `Ledger::finish` + `ensure_provenance_columns` (idempotent `ADD COLUMN`); `entries` INSERT + 3 cols; `Ledger::{set_write_context, audit_trail}`; 3 tests (both backends + preexisting-ledger open) |
| `ekos/crates/ledger/src/fact_ledger.rs` | `Inner.{write_ctx, provenance}` + `provenance_path`/`load_provenance`; sidecar append in `append_inner` (best-effort, never fails the write); `FactLedger::{set_write_context, audit_trail}` via `entity_entries` → tx list → `batch_times` + sidecar |
| `ekos/crates/cli/src/commands/commit.rs` | mint `run_id`, `set_write_context` per stage; CKM file hash as `source_artifact_id` |
| `ekos/crates/cli/src/commands/build.rs` | `set_write_context` per `File` artifact with the observation `ArtifactId` |
| `ekos/crates/cli/src/commands/ledger.rs` | `pub fn audit(config, cwd, id, json)`; 1 test |
| `ekos/crates/cli/src/bin/ekos.rs` | `LedgerCommands::Audit { id, json }`; `emits_machine_output` for `--json` |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_audit` tool (schema + handler; uses `ledger` directly); tool-list test updated |

---

## Knowledge Captured

- **`WriteContext` on the handle, not a parameter on `append_*`.** Adding a param to the four
  trait methods means touching the trait, the impl macro, both backends, `PartitionedLedger`, and
  ~60 call sites. A stateful `set_write_context` (default no-op) is invisible to every caller that
  doesn't opt in — `Ledger` needs only a `RefCell` (it's already `!Sync`, bare `Connection`).
- **No `user_version` bump for the SQLite columns.** `ALTER TABLE entries ADD COLUMN <x> TEXT`
  with no default is O(1) metadata-only, old rows read `NULL`, and every existing
  `SELECT`/`INSERT` names its columns explicitly — so it's purely additive. A version bump would
  only matter if a *reader* needed to branch on presence, and none does (`audit_trail` selects
  the columns unconditionally; a pre-0135 db gets them added on the first open after upgrade).
- **FactLedger sidecar over segment-format change.** The fact engine decomposes payloads into
  per-attribute facts; there's no per-*version* metadata slot. A `provenance.jsonl` keyed by `tx`
  (the batch id `append_with_seal` already returns, already mapped to wall time in `batch_times`)
  gives `audit_trail` everything it needs with zero risk to `reconstruct`. Best-effort: a failed
  sidecar write logs nothing and never fails the ledger write.
- **The self-workspace's audit trail is the feature's own advertisement.**
  `ekos_ledger::Ledger` shows 4 versions all stamped 2026-08-26 between 08:29 and 10:37 — the
  ledger rebuilt several times that day (which is also why Part A's timeline is one bucket).
  Provenance would have named each of those runs; going forward it will.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/provenance.rs` | New — `WriteContext` / `AuditRecord` / `new_run_id` |
| `ekos/crates/ledger/src/lib.rs` | trait + macro + `Ledger` impl + schema + tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | `Inner` fields + sidecar + `FactLedger` impl |
| `ekos/crates/cli/src/commands/{commit,build,ledger}.rs` | wiring + `ekos ledger audit` |
| `ekos/crates/cli/src/bin/ekos.rs` | `Audit` subcommand |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_audit` tool |
| `ekos/docs/rfcs/0135-…md` | Part B marked implemented |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | `ekos ledger audit` + `ekos_audit` |

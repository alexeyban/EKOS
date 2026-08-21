# Devlog 9 — Phase 13: Optimizer

**Date:** 2026-07-11
**PRs:** —
**Branch:** main

---

## Summary

Implemented Phase 13 — the Optimizer. Wrote RFC 0011 first per the mandatory workflow, then closed
out all five TODO items: incremental compilation (source fingerprinting so unchanged sources skip
re-scanning), parallel pass execution (DAG-level concurrency in `PassManager`), artifact cache
invalidation (`should_recompute` skipping unchanged passes), knowledge diff (`ekos diff`), and
knowledge branch/merge (`ekos branch`). Along the way, fixed a real pre-existing gap: the ledger's
append was idempotent purely on id, so updating an existing object/relationship's content was
silently a no-op — `diff_ledger` couldn't exist without fixing that first, so the fix is part of
this phase, not a detour. All 218 workspace tests pass, clippy clean (only pre-existing unrelated
warnings), and every feature verified end-to-end against a real built ledger, not just unit tests.

---

## RFC 0011 — Optimizer

### Problem / motivation

Every prior phase re-processes the entire enterprise from scratch on every `ekos build`/
`ekos recover`. Auditing the codebase before writing the RFC surfaced that `Ledger::append_object`/
`append_relationship` were keyed by the object's own stable `KirId`, and `append` no-op'd whenever
that id already existed — regardless of whether the payload changed. Re-running the pipeline
against a modified source silently dropped the update. `diff_ledger` ("a new entry superseded an
older one for the same object", per the TODO) cannot exist without fixing this, so the RFC treats
it as in-scope.

### What was built

| Component | Change |
|---|---|
| `docs/rfcs/0011-optimizer.md` | RFC 0011 (accepted) — all 5 sub-designs below |
| `crates/observation-sdk/src/lib.rs` | `Fingerprint`, `source_fingerprint(ctx)` — mtime+size hash over ignore-filtered tree |
| `crates/cli/src/commands/build.rs` | Fingerprint gate skips both observers per unchanged `observe.paths` entry; `.ekos/fingerprints.json` |
| `crates/ledger/src/lib.rs` | Versioned `entries` (rowid-addressed), `content_signature` (excludes volatile `created_at`), `LedgerEntryId`/`LedgerDiff`/`diff_ledger`, `MergeReport`/`MergeConflict`/`merge_branch`, `vacuum_into` |
| `crates/compiler-core/src/pass.rs` | `PassContext` now `Clone` with `Arc<Mutex<DiagnosticSink>>`; `CompilerPass::version()`/`cache_inputs()`; `PassManager::execution_levels()`/`run_all_parallel()`; cache-skip wired into `run_all()` |
| `crates/compiler-core/src/cache.rs` | New: `should_recompute`, `record_manifest`, `config_hash` — JSON manifests at `.ekos/artifacts/pass-manifests/` |
| `crates/compiler-core/src/scheduler.rs` | `PassOutcome::{ran,skipped}`, `ExecutionReport::passes_skipped()`, `Scheduler::run_parallel` |
| `crates/recovery/src/sql_analyzer.rs`, `git_analyzer.rs` | `cache_inputs()` overrides — SQL content hash, commit/repo artifact ids |
| `crates/compiler-core/src/config.rs` | `ledger_dir`, `branch_ledger_path` |
| `crates/cli/src/commands/diff.rs`, `branch.rs` | New: `ekos diff --from --to`, `ekos branch create/list/merge/delete` |
| `crates/cli/src/bin/ekos.rs` | Wired `Diff`, `Branch{Create,List,Merge,Delete}`, `Recover{parallel}` |
| `crates/cli/src/commands/recover.rs` | `--parallel` flag selecting `run_all_parallel`; prints skip/parallel-mode summary |

### Implementation details worth remembering

**The ledger versioning fix is the load-bearing change.** `entries.id` is no longer `UNIQUE` —
logical identity (`KirObject.id`/`KirRelationship.id`) stays stable across an object's lifetime, but
each distinct *content* under that id gets its own row, addressed by SQLite `rowid`.
`current_objects`/`current_relationships` became pointer tables (`entry_rowid` instead of directly
aliasing `entries.id`), exactly like a Git ref pointing at a commit. This incidentally also fixed
`object_at` for true historical correctness — it previously only worked for never-updated objects,
since it filtered the *current* pointer's timestamp rather than querying all past versions.

**Comparing raw JSON payloads for version-equality doesn't work** — `KirObject`/`KirRelationship`
both stamp a fresh `created_at` on every `::new()` call, so two logically-identical objects from two
separate build runs always serialize differently. Fixed with `content_signature()`: strips
`created_at` before hashing (via `ArtifactId::compute`, reusing the artifact crate's canonicalization
rather than duplicating it — `ekos-ledger` now depends on `ekos-artifact`). Caught this by writing
the versioning fix, testing it, and watching the diff test count 3 "added" entries instead of the
expected 1 — every re-append of an unchanged object was being treated as a new version. The same
`content_signature` helper is reused by `merge_branch`'s conflict detection for the same reason.

**`PassContext` becoming `Clone` (config/diagnostics/artifact_store are all `Arc`, cwd is a cheap
`PathBuf`) is what makes parallel pass execution possible** without fighting Rust's aliasing rules:
each concurrently-spawned pass gets its own owned context clone rather than sharing one `&mut`
reference, while all clones still write into the same `Arc<Mutex<DiagnosticSink>>` and artifact
store. `DiagnosticSink` moving behind a `Mutex` was flagged as deferred to exactly this phase back
in RFC 0001 — about 10 call sites across `recovery`, `semantic`, and two `cli` commands needed a
`.lock().unwrap()` added.

**`execution_levels()` groups the existing dependency DAG via iterated Kahn layering** (take all
zero-remaining-indegree, not-yet-placed passes as one level, repeat) — passes within a level have no
path between them, so `run_all_parallel` spawns each level's passes concurrently via `tokio::spawn`
and awaits the whole level with a plain loop over `JoinHandle`s (no `futures::join_all` needed —
`tokio::spawn` already starts execution immediately on spawn, so sequentially awaiting handles
afterward doesn't reduce concurrency). Verified with a unit test asserting three independent passes'
start times land within 100ms of each other — the exact criterion from the TODO — and with a real
CLI run (`ekos recover --parallel` against two SQL files) where both passes' "running pass
(parallel)" log lines are 17 microseconds apart.

**Artifact cache invalidation manifests are plain JSON files, not entries in the content-addressable
`ArtifactStore`.** `ArtifactStore::write()` is a no-op once an id already exists — correct for
immutable content, wrong for "here is the latest state for pass X," which needs to be overwritten
every run. Manifests live at `.ekos/artifacts/pass-manifests/<sha256-of-pass-name>.json` instead,
written directly via `std::fs`, mirroring the `.ekos/fingerprints.json` approach from the
incremental-build fingerprint gate for the same reason.

**`cache_inputs()` had to be pass-specific, not caller-supplied**, despite the TODO's literal
`should_recompute(pass, inputs: &[ArtifactId], store)` signature — different passes have
structurally different notions of "input" (SQL text vs. a set of commit artifact ids), and
`PassManager` has no generic way to know either. `SqlAnalyzerPass::cache_inputs()` hashes its own
`self.sql` string; `GitAnalyzerPass::cache_inputs()` returns its existing `commit_artifact_ids` (already
`Vec<ArtifactId>`) plus the repo id. Both are genuinely available on the pass already — no new
plumbing needed.

**A real regression caught before it shipped:** the first version of the cache-manifest wiring
called `record_manifest` unconditionally from `run_all`, and several existing tests construct
`PassContext` with `cwd = PathBuf::from(".")` for convenience. That meant every `cargo test` run
started writing a real `.ekos/artifacts/pass-manifests/` directory into the crate's actual source
tree. Caught it by noticing a stray `.ekos/` appear under `crates/compiler-core/` after a test run,
fixed by switching those three test call sites (`pass.rs` ×2, `compiler.rs` ×1) to `tempfile::tempdir()`,
and cleaned up the polluted directory. Worth remembering: any test that runs code touching
`ctx.config.artifact_dir()`/`ekos_dir()` needs a tempdir `cwd`, not `"."`, from now on.

### Decisions

**Ledger versioning uses `rowid`, not a rewritten event-sourcing model.** `ekos.md`'s original
framing describes state as "a fold over events," but a full event-sourcing rewrite is exactly the
"v1.0 backend swap" RFC 0004 already deferred. The rowid fix is the minimal change that makes
`diff_ledger` correct without touching the CKM/Runtime read paths that depend on `all_objects`/
`get_object`'s current-state semantics.

**`ekos branch merge` never fails on conflicts** — conflicts are informational output (same pattern
as Phase 7's identity-resolution merge proposals), not a hard error. A non-zero exit would make
`merge` unusable as a routine check-in step when conflicts are common but not blocking.

**Branch/merge is last-write divergence detection, not true 3-way merge.** No common-ancestor
lookup, no automatic conflict resolution — explicitly out of scope for v0, documented as such in
RFC 0011, matching this project's established pattern (RFC 0009/0010 both narrowed scope the same
way) of shipping something fully specified and testable over something partially done.

**`ekos build` was not migrated onto `Compiler`/`PassManager`.** It still hand-rolls its observer
loop; only the fingerprint gate (§1) was added to it. Parallel pass execution and cache invalidation
apply to whatever already goes through `PassManager` — today, that's `ekos recover`'s SQL/Git
analyzer passes.

---

## Knowledge Captured

- **Ledger idempotency was silently data-losing before this phase.** Re-running `ekos build`
  against a changed file never updated the ledger — `append_object` no-op'd on existing id
  regardless of payload. Anyone building on the Phase 9 ledger assuming "re-append reflects current
  state" was wrong; this is now fixed, but it's worth knowing the ledger had this gap for 4 phases.
- **Two logically-identical `KirObject`/`KirRelationship` values are never `==` across two
  `::new()` calls** because `created_at` is stamped fresh each time and is part of the serialized
  payload. Any future code comparing KIR values for "did this actually change" must strip
  `created_at` first (see `content_signature` in `ekos-ledger`) — raw equality or raw JSON diffing
  will always say "changed."
- **`tokio::spawn` starts execution immediately, before the returned `JoinHandle` is ever awaited.**
  `run_all_parallel` spawns a whole level's passes in a tight loop, then awaits the handles in a
  second loop — concurrency comes from the spawn, not from `futures::join_all`, so the latter
  dependency wasn't needed.
- **Any test constructing `PassContext`/`Compiler` must use a tempdir `cwd`, never `"."`**, now
  that `run_all` touches `ctx.config.artifact_dir()` for cache manifests. Three pre-existing tests
  in `compiler-core` got this wrong; fixed and documented here so it isn't rediscovered by finding a
  stray `.ekos/` in a crate directory again.
- **Benchmark numbers** (50 identical SQL files, debug build, tmpfs): cold `ekos build` ~199ms,
  warm (fingerprint-skipped) ~7ms — 3.5% of cold, comfortably under the TODO's "<10% of first run"
  criterion. Cold `ekos recover` ~28ms, warm (all passes cache-skipped) ~10ms. Absolute numbers are
  noisy at debug-build/tmpfs scale; the fingerprint/cache-skip mechanism itself, not the exact ratio,
  is what matters.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0011-optimizer.md` | New RFC (accepted) |
| `crates/observation-sdk/{Cargo.toml,src/lib.rs}` | `Fingerprint`, `source_fingerprint`; 3 new tests |
| `crates/cli/src/commands/build.rs` | Fingerprint-gated incremental skip |
| `crates/ledger/{Cargo.toml,src/lib.rs}` | Versioned entries, `diff_ledger`, `merge_branch`, `vacuum_into`; 8 new tests |
| `crates/compiler-core/{Cargo.toml,src/lib.rs,src/pass.rs,src/scheduler.rs}` | `Clone` `PassContext`, parallel execution, cache invalidation wiring; new `src/cache.rs` (5 tests) + 3 new pass.rs tests |
| `crates/recovery/src/{sql_analyzer.rs,git_analyzer.rs}` | `cache_inputs()` overrides |
| `crates/semantic/src/lib.rs` | `.diagnostics.lock().unwrap()` call-site update |
| `crates/compiler-core/src/config.rs` | `ledger_dir`, `branch_ledger_path` |
| `crates/cli/src/commands/{diff.rs,branch.rs}` | New CLI commands |
| `crates/cli/src/commands/{mod.rs,recover.rs}` | Registration, `--parallel` flag |
| `crates/cli/src/bin/ekos.rs` | `Diff`, `Branch`, `Recover{parallel}` subcommands |
| `TODO.md` | Ticked all 5 Phase 13 items — Phase 13 fully complete |

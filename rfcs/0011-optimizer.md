# RFC 0011 — Optimizer (Phase 13)

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | EKOS team |
| **Created** | 2026-07-11 |
| **Gating** | Phase 13 |

---

## Motivation

Every prior phase re-processes the entire enterprise from scratch on every `ekos build`/`ekos
recover`. That's fine for a demo fixture, unusable at real enterprise scale. Phase 13 adds the same
capabilities `make`/Bazel/Cargo have for code: skip unchanged inputs, run independent work
concurrently, and know precisely what changed between two points in ledger history. This phase does
not change what is produced, only how quickly and how legibly.

Auditing the current implementation surfaced one real gap that blocks the "knowledge diff" goal
outright: `Ledger::append_object`/`append_relationship` are keyed by the object's own stable
`KirId`, and `Ledger::append` no-ops whenever that id already exists in `entries` — regardless of
whether the payload changed. Re-running the pipeline against a modified source today silently
drops the update; nothing is lost destructively, but nothing new is recorded either. `diff_ledger`
as specified ("a new entry superseded an older one for the same object") cannot exist without
fixing this first, so this RFC treats it as in-scope, not a pre-existing issue to work around.

---

## Design

### 1. Incremental compilation — source fingerprinting

`observation-sdk` gains:

```rust
pub struct Fingerprint(pub String);

/// Cheap pre-scan signature for a source tree: hashes (relative path, size, mtime)
/// for every non-ignored file under ctx.workspace_root, without reading file
/// contents. Two scans of an unchanged tree produce the same fingerprint;
/// any add/remove/modify changes it.
pub fn source_fingerprint(ctx: &ScanContext) -> Fingerprint;
```

This is a single generic fingerprint, not one fingerprint type per connector — `ekos build` uses
it as a per-`observe.paths` entry gate *before* invoking any `Observer::scan`. If the fingerprint
for a given base path is unchanged since the last successful build, both the `file` and `git`
observers are skipped entirely for that path and their previously-recorded artifacts/objects are
left as-is (correct, since content-addressed artifact ids already dedupe on the content side —
this closes the *scan cost* gap, not the *cache-hit* gap, which Phase 2 already solved for writes).

Fingerprints persist at `.ekos/fingerprints.json` (`{ "<base-path>": "<hex fingerprint>" }`) —
plain JSON, not the content-addressable artifact store, since this is workspace-level scan-cache
state, not a compiler artifact with dependents.

**Non-goal for v0:** per-file incremental scanning within a changed tree (i.e. `FileObserver` still
re-reads every file once the tree-level fingerprint changes). Git HEAD-sha and DB schema-version
fingerprints mentioned in the original TODO wording are folded into the one generic mtime/size
fingerprint rather than implemented per-connector-type — `.git/HEAD`'s mtime already changes on
every commit, so the generic fingerprint catches git changes too without a separate code path.
There is no SQL/DB observer yet (Phase 14), so a "schema version hash" fingerprint has nothing to
attach to; deferred until that connector exists.

### 2. Ledger versioning fix + knowledge diff

**Root cause fix:** `entries.id` stops being required unique. An object/relationship keeps one
stable logical id (`KirObject.id` / `KirRelationship.id`) across its lifetime, but each *distinct
payload* written for that id becomes its own row in `entries`, addressed by SQLite `rowid`
(monotonic, unique, free). `current_objects`/`current_relationships` become pointer tables: they
map the logical id to the `rowid` of its latest version, exactly like a Git ref pointing at a
commit.

```sql
CREATE INDEX IF NOT EXISTS idx_entries_id ON entries(id);  -- no longer UNIQUE

CREATE TABLE IF NOT EXISTS current_objects (
    object_id   TEXT PRIMARY KEY,
    entry_rowid INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS current_relationships (
    rel_id      TEXT PRIMARY KEY,
    entry_rowid INTEGER NOT NULL,
    from_id     TEXT NOT NULL,
    to_id       TEXT NOT NULL,
    kind        TEXT NOT NULL
);
```

`append()`'s idempotency check changes from "does this id exist" to "does this exact
(id, payload) pair exist" — same content re-appended is still a no-op (returns the existing
rowid); different content under the same id inserts a new row and returns the new rowid. Callers
(`append_object`, `append_relationship`) always repoint the current-state table at the returned
rowid, so `get_object`/`all_objects`/etc. keep returning the *latest* version with no interface
change. `get_evidence`/`get_relationship`'s direct by-id lookups add `ORDER BY rowid DESC LIMIT 1`
defensively (evidence is not expected to version in practice, but the schema no longer guarantees
uniqueness, so the query must not depend on it).

This also fixes `Ledger::object_at` for free: since old versions are no longer overwritten, keying
directly off `entries WHERE id = ? AND entry_type = 'object' AND written_at <= ? ORDER BY
written_at DESC LIMIT 1` returns the true historical version, not just "the current version, if it
happens to be old enough" (the pre-existing behavior, which only worked for never-updated
objects). `relationships_at` keeps its current-table-based approach — true historical multi-version
relationship queries are a non-goal here, since nothing in Phase 13's validation criteria requires
it, and it doesn't regress (relationships weren't versioned at all before this RFC).

**Diff:**

```rust
pub struct LedgerEntryId(pub i64);   // the version row's rowid

pub struct LedgerDiff {
    pub added: Vec<LedgerEntryId>,   // object/relationship versions written in (from, to]
    pub unchanged: usize,            // currently-tracked objects/relationships not among them
}

pub fn diff_ledger(ledger: &Ledger, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<LedgerDiff, LedgerError>;
```

`added` = rowids of `entries` rows with `entry_type IN ('object','relationship')` and
`from < written_at <= to`. `unchanged` = (current object count + current relationship count) minus
the number of distinct logical ids touched by `added`. No deletions exist in an append-only ledger,
so "changed" is exactly "a new version was recorded for an id that already existed" — `added`
doesn't distinguish "brand new object" from "updated object" because the ledger has no concept of
object birth vs. update beyond "was this id present before `from`", which callers can check
themselves via `object_at(id, from)` if they need that distinction.

CLI: `ekos diff --from <RFC3339> --to <RFC3339>` (both required in v0 — the shorthand
`ekos diff <state-1> <state-2>` in the original TODO's bash sketch is positional sugar for the same
two timestamps; flags are used here for parity with the rest of the CLI's `--`-flag style).

### 3. Parallel pass execution

`CompilerPass::run` already takes `&mut self` and `PassContext` is already threaded through by
`&mut` reference — both must change for real concurrency:

- `PassContext.diagnostics` becomes `Arc<Mutex<DiagnosticSink>>` (RFC 0001 flagged this
  refactor as deferred to exactly this phase). All `ctx.diagnostics.foo(...)` call sites become
  `ctx.diagnostics.lock().unwrap().foo(...)`.
- `PassContext` derives `Clone` (config/artifact_store are already `Arc`, cwd is a cheap
  `PathBuf` clone, diagnostics is now a shared `Arc<Mutex<_>>` clone) — each concurrently
  scheduled pass gets its own **owned** `PassContext` clone rather than sharing one `&mut`
  reference, sidestepping Rust's aliasing rules while all clones still write into the same
  diagnostics sink and artifact store.
- `PassManager::execution_levels()` groups the existing dependency DAG into levels via iterated
  Kahn layering (repeatedly take all currently-zero-indegree, not-yet-run passes as one level).
  Passes within a level have no path between them in the DAG by construction, so nothing in one
  level can read output the others in the same level would produce.
- `PassManager::run_all_parallel()` runs one level at a time: `tokio::spawn` each pass in the
  level with its own `PassContext` clone, `futures::future::join_all` the level, collect
  `PassOutcome`s, then proceed to the next level. Sequential `run_all` is untouched and remains
  the default — `run_all_parallel` is opt-in.
- `ekos recover --parallel` selects `run_all_parallel`. `ekos build` doesn't use
  `PassManager` today (a pre-existing gap, not introduced here) so parallelism only applies to
  the passes that already go through it: `SqlAnalyzerPass` (one per SQL file, no declared
  dependencies) and `GitAnalyzerPass`.

**Non-goal:** migrating `ekos build`'s hand-rolled observer loop onto `Compiler`/`PassManager`.
That's a larger, separately-RFC-able refactor; Phase 13 only needs `build`'s incremental fingerprint
gate (§1) and `recover`'s pass-level parallelism (this section), which don't require it.

### 4. Artifact cache invalidation (`should_recompute`)

`CompilerPass` gains a default method:

```rust
fn version(&self) -> &str { "v1" }  // bump manually when a pass's logic changes
```

Each pass run's *recomputation identity* is `{pass_name, version, config_hash, input_ids}` where
`config_hash` is the SHA-256 of the canonical JSON of the pass-relevant `EkosConfig` subtree (e.g.
`[llm]` for `SqlAnalyzerPass`). After a pass runs, `PassManager` writes this record as a small JSON
manifest to `.ekos/artifacts/pass-manifests/<pass_name>.json`. Before running a pass again:

```rust
pub fn should_recompute(pass: &dyn CompilerPass, config_hash: &str, input_ids: &[ArtifactId], store: &dyn ArtifactStore) -> bool;
```

returns `true` (recompute) if no manifest exists yet, or if any of `version`/`config_hash`/
`input_ids` differs from the stored manifest — covering all three invalidation rules from RFC 0002
(content change is covered by `input_ids` differing, since those ids are already content hashes).
Skipped passes are reported as `PassOutcome::Skipped` in the build summary ("N passes skipped
(cached)").

### 5. Knowledge branch and merge

Branches are alternate ledger files: `.ekos/ledger/<name>.db`, created via SQLite's
`VACUUM INTO` (not a raw file copy — the main ledger runs in WAL mode, so a plain `cp` can miss
data still sitting in the `-wal` file; `VACUUM INTO` always produces a consistent, complete
snapshot regardless of WAL state).

```rust
pub struct MergeConflict { pub object_id: String, pub reason: String }
pub struct MergeReport {
    pub objects_merged: usize,
    pub relationships_merged: usize,
    pub conflicts: Vec<MergeConflict>,
}

pub fn merge_branch(main: &Ledger, branch: &Ledger) -> Result<MergeReport, LedgerError>;
```

For every object/relationship currently tracked in `branch`: if `main` has no version of that
logical id, append it (a clean addition). If `main` already has a version and it's `==` the
branch's version (added `PartialEq` to `KirObject`/`KirRelationship`), it's a no-op — both sides
already agree. If `main` has a *different* version, it's a conflict: recorded in the report,
**not** auto-resolved or overwritten (the ledger's append-only /no-silent-mutation invariant
extends to merges — a human must decide). `ekos branch merge` prints the report; a non-empty
conflict list doesn't fail the command (conflicts are information, not necessarily an error state
the branch feature can't otherwise proceed past — see Alternatives).

**Non-goal:** any real 3-way merge (finding a common ancestor version and diffing both sides
against it). This is last-write divergence detection only — sufficient for the validation
criteria and honest about what it is.

`ekos branch create <name>` / `list` / `merge <name>` / `delete <name>`.

---

## Alternatives Considered

- **Full event-sourcing rewrite of the ledger** (fold-over-events state reconstruction, per
  `ekos.md`'s original framing) — rejected for this phase. The rowid-versioning fix is the minimal
  change that makes `diff_ledger` correct and makes `object_at` incidentally more correct, without
  touching the CKM/Runtime read paths (Phase 8/10) that already depend on `all_objects`/
  `get_object`'s current-state semantics. A full rewrite is exactly the "v1.0 backend swap" RFC
  0004 already deferred; conflating it with the Optimizer phase would block this phase on a much
  bigger, separately-reviewable change.
- **Fail the `ekos branch merge` command on any conflict** — rejected. Conflicts are expected,
  informational output (like identity-resolution's merge-proposal conflicts in Phase 7), not a
  hard error; a non-zero exit would make merge unusable as a routine check-in step when conflicts
  are common but not blocking.
- **Per-connector fingerprint types (git HEAD sha, DB schema hash, fs mtime)** — rejected in favor
  of one generic mtime/size fingerprint, since the only two connectors that exist today (file, git)
  are both filesystem-backed and a DB connector doesn't exist yet to fingerprint. Revisit when
  Phase 14 adds a real DB connector.

---

## Open Questions

*(none — all resolved above; this RFC was written after the exploratory groundwork was already
laid out against the current codebase, not before, so there was no design-time ambiguity left to
carry into implementation.)*

---

## Acceptance Criteria

- [x] `source_fingerprint` implemented in `observation-sdk`; `ekos build` run twice unchanged skips
      both observers and reports "0 connectors re-scanned".
- [x] `PassManager::run_all_parallel` runs DAG-independent passes concurrently; `ekos recover
      --parallel` measurably overlaps SQL-analyzer passes.
- [x] `should_recompute` implemented and wired into `PassManager`; changing a pass's relevant config
      forces recompute even with unchanged inputs.
- [x] `diff_ledger` implemented against the fixed versioned-append ledger; `ekos diff` CLI works.
- [x] `ekos branch create/list/merge/delete` implemented; conflict detection verified.
- [x] Design is consistent with `ekos.md`'s compiler architecture (append-only ledger, read-only
      Runtime untouched, deterministic passes untouched — only their scheduling and the ledger's
      internal versioning changed).

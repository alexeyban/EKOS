# Devlog 1 — Phases 2, 3, 4, 5: Artifact System, Observation SDK, Git Observer, KIR Formalization

**Date:** 2026-07-02  
**PRs:** —  
**Branch:** main

---

## Summary

Bootstrapped the EKOS compiler through Phase 5. Starting from a working walking skeleton (Phase 1.5:
`ekos init / build / clean / doctor / query` with 26 tests), this session added content-addressable
artifact storage, a formal connector SDK, a git observer plugin, and formalized KIR with full
round-trip tests. Total test count grew from 26 → 54, clippy clean across all 16 workspace members.

---

## Phase 2 — Artifact System (`crates/artifact/`)

### Problem / motivation
The skeleton wrote `KirObject`s directly to the ledger; there was no caching, no way to know
whether an observation changed, and no stable reference to "what was seen." The artifact system
gives every compiler input and output a stable, content-derived identity.

### What was built

| Component | Description |
|---|---|
| `ArtifactId` | SHA-256 of canonical (key-sorted) JSON of *content fields only* |
| `canonicalize()` | Recursively sorts object keys before hashing |
| `ObservationArtifact` | Raw connector output; ID stable across time |
| `KnowledgeArtifact` | Compiled KIR graph + input artifact IDs |
| `EvidenceArtifact` | Storage wrapper for `KirEvidence` |
| `DiagnosticArtifact` | Build diagnostics (diffable across runs) |
| `IndexArtifact` | Per-build manifest: logical name → `ArtifactId` |
| `ArtifactStore` trait | Backend-agnostic read/write/exists API |
| `FileSystemArtifactStore` | Git object-store layout: `<root>/<2-hex>/<64-hex>.json` |
| `PassContext.artifact_store` | `Arc<dyn ArtifactStore>` threaded into every compiler pass |

### Implementation details worth remembering
- Content/metadata split: each artifact type has a nested `*Content` struct. The `ArtifactId`
  is `compute_content_id(&content)` — the volatile outer wrapper (`created_at`, `produced_by`)
  is excluded. This is the key invariant for build idempotency.
- `#[serde(flatten)]` on `content` merges the content fields into the top-level JSON, while
  `meta` remains a nested object. This separates stable content from volatile metadata cleanly.
- `FileSystemArtifactStore::write` returns `bool`: `true` = newly written, `false` = cache hit.
  Same semantics as `Ledger::append_object`.

### Decisions
- **Separate `*Content` struct per artifact type** over a generic `payload: Value` field — gives
  type-safe access to artifact fields without downcasting.
- **Store JSON blobs** (not typed Rust structs) — the store is type-erased at the boundary;
  callers serialize before writing and deserialize after reading. Simpler trait, easier to evolve.

---

## Phase 3 — Observation SDK (`crates/observation-sdk/`, `plugins/file/`)

### Problem / motivation
The skeleton's `ekos build` had file walking hard-coded inline. Connectors need a formal contract
so new sources (git, postgres, confluence) can be added without touching the compiler core.

### What was built
- `Observer` trait: `fn name() -> &str` + `async fn scan(ctx) -> ObservationPackage`
- `ScanContext`: `workspace_root`, `ignore_patterns`, `ConnectorConfig`, `is_ignored(rel_path)`
- `ObservationPackage`: `Vec<ObservationArtifact>` + `PackageMeta`
- `FileObserver`: walks with `walkdir`, computes SHA-256 of file content, emits one artifact per file
- `ekos build` now routes through `Observer::scan` → artifact store → KIR → ledger

### Implementation details worth remembering
- `filter_entry` in walkdir prunes entire directory subtrees (`.git`, `target`) efficiently.
  `ScanContext::is_ignored` is a secondary check for relative-path components.
- Rust 2024 `if let` chains (`if cond && let Some(x) = ...`) are valid in edition 2024 and
  clippy wants them when two nested `if`/`if let` can be merged.
- `ObserverError` re-exported as `ObserveError` alias so old stub code using `ObserverError`
  compiles unchanged.

---

## Phase 4 — Git Observer (`plugins/git/`)

### Problem / motivation
Files alone give structural knowledge. Git history gives authorship, coupling, and temporal patterns.
The git observer is Phase 4's first semantic data source.

### What was built
- `GitObserver` shells out to `git` CLI (no `git2` dep — keeps the build simple)
- Emits one `ObservationArtifact(target="repo")` with head branch, remotes, contributors
- Emits one `ObservationArtifact(target=SHA)` per commit: author, date, message, files, +/-
- `ekos build` now runs `[FileObserver, GitObserver]`; git artifacts are stored but not yet
  converted to KIR (that's Phase 6 `GitAnalyzer`)
- `IndexArtifact` written after each build + `.ekos/snapshots/<timestamp>.json` for history

### Implementation details worth remembering
- `git log --format=` with `\x1f` (ASCII unit separator) as field delimiter avoids issues with
  commas/tabs in commit messages.
- `is_git_repo()` uses `git rev-parse --git-dir` — returns false cleanly for non-git directories
  so the observer is safe to run everywhere.
- `parse_stat_summary` parses the last line of `git show --stat`: "N files changed, M insertions(+),
  K deletions(-)". Only the summary line matters; individual file lines are ignored.

---

## Phase 5 — KIR Formalization (`crates/kir/`)

### Problem / motivation
KIR types existed since Phase 1.5 but `KirRelationship`, `KirEvent`, and full `KirGraph`
serialization were untested. RFC 0003 required confirmation that the graph round-trips cleanly
and that the kir crate has no forbidden dependencies.

### What was built
- 4 new tests: `kir_graph_full_round_trip`, `kir_relationship_serializes_from_to`,
  `kir_event_round_trip`, `knowledge_artifact_embeds_kir_graph`
- `sample_graph()` fixture: customers → orders FK, one `Created` event, backed by evidence
- Confirmed: kir crate imports only `chrono`, `serde`, `serde_json`, `uuid`, `thiserror`

### Decisions
- **No KirGraph-level hash / ID** — a `KirGraph` is always embedded inside a `KnowledgeArtifact`;
  the artifact's `ArtifactId` (SHA-256 of the KnowledgeContent) serves as the graph's stable ID.

---

## RFCs Written
- **RFC 0003** (`docs/rfcs/0003-kir.md`) — four primitives, `KirId` (UUIDv4/v5), `KirGraph`
  as `KnowledgeArtifact` payload, no semantic enrichment in kir crate
- **RFC 0006** (`docs/rfcs/0006-observation-sdk.md`) — `Observer` trait, `ScanContext`,
  `ObservationPackage`, static plugin linking for v0.x, snapshot output contract

---

## Knowledge Captured

- **Serde flatten + nested metadata**: `#[serde(flatten)]` on the content field and a plain
  `meta: ArtifactMeta` field at the same level gives the cleanest JSON without key clashes,
  as long as none of the flattened fields share names with the outer struct's fields.

- **ArtifactStore write returns bool**: Mirroring `Ledger::append_object`'s idempotency signal
  makes calling code uniform. Always check the return value for cache-miss telemetry.

- **Git observer cost**: `git diff-tree` + `git show --stat` per commit is O(N) shell invocations.
  For repos with thousands of commits this will be slow. In Phase 4+ we should batch with
  `git log --stat` in one call and parse the combined output instead.

- **Rust 2024 if-let chains**: `if outer_cond && let Some(x) = expr { ... }` is stable in
  edition 2024. clippy's `collapsible_if` lint enforces this. Expect it whenever nesting
  `if bool_expr { if let Some(...) = ... { ... } }`.

- **Snapshot vs IndexArtifact**: Both are written on each build. `IndexArtifact` lives in the
  artifact store (content-addressed, cacheable). The snapshot JSON in `.ekos/snapshots/` is
  human-readable build history (not content-addressed, kept as-is).

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/artifact/src/lib.rs` | Full artifact type system: ArtifactId, 5 artifact types, ArtifactMeta |
| `crates/artifact/src/store.rs` | ArtifactStore trait + FileSystemArtifactStore |
| `crates/artifact/Cargo.toml` | Added tempfile dev-dep |
| `crates/observation-sdk/src/lib.rs` | Observer trait, ScanContext, ObservationPackage |
| `crates/observation-sdk/Cargo.toml` | Added ekos-artifact, tokio, chrono |
| `plugins/file/src/lib.rs` | Full FileObserver with SHA-256 content hashing + 5 tests |
| `plugins/file/Cargo.toml` | Added ekos-artifact, sha2, hex, tokio, tracing |
| `plugins/git/src/lib.rs` | GitObserver: shells out to git, 5 tests |
| `plugins/git/Cargo.toml` | New crate |
| `crates/cli/src/commands/build.rs` | Uses Observer trait; runs file+git observers; writes snapshot |
| `crates/cli/Cargo.toml` | Added ekos-artifact, ekos-observation-sdk, plugin-file, plugin-git |
| `crates/compiler-core/src/pass.rs` | PassContext gains artifact_store: Arc<dyn ArtifactStore> |
| `crates/compiler-core/Cargo.toml` | Added ekos-artifact |
| `crates/kir/src/lib.rs` | 4 new Phase 5 tests for KirGraph, KirRelationship, KirEvent |
| `docs/rfcs/0003-kir.md` | RFC 0003 — KIR spec |
| `docs/rfcs/0006-observation-sdk.md` | RFC 0006 — Observation SDK spec |
| `ekos/Cargo.toml` | Added plugins/git to workspace |

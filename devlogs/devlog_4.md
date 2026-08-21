# Devlog 4 — Phase 8 + 9: Semantic Compiler + Knowledge Ledger wiring

**Date:** 2026-07-03  
**PRs:** —  
**Branch:** main

---

## Summary

Implemented Phase 8 (Semantic Compiler: KIR → CKM) and wired Phase 9 (Knowledge Ledger).
The full pipeline is now runnable: `ekos build` → `ekos recover` → `ekos compile` → `ekos commit`.
The ledger was already partially implemented in the skeleton; this phase completes the CLI.
93 tests total (9 new in semantic, +4 new small helpers), clippy clean.

---

## Phase 8 — Semantic Compiler (`crates/semantic/`)

### What was built

| Component | Description |
|---|---|
| `EvidenceRecord` | Flattened provenance: id, source path, fragment, confidence |
| `CkmObject` | Canonical object: embedded evidence sorted by confidence desc; primary_description |
| `CkmRelationship` | Canonical relationship: embedded evidence, dedup'd by (from, to, kind) |
| `CkModel` | version=1, compiled_at, objects, relationships, evidence_index; `validate()` |
| `merge_graphs(dst, src)` | Append all KirGraph nodes from src into dst |
| `apply_merges(graph, proposals)` | Remap non-canonical IDs; remove merged objects; dedup rels |
| `dedup_relationships(rels)` | Dedup by (from, to, kind), merging evidence lists |
| `build_ckm(graph)` | KirGraph → CkModel; embeds evidence into objects + relationships |
| `SemanticCompilerPass` | CompilerPass: loads KAs → identity resolution → CKM → model.json |

### Algorithm

1. Load all `KnowledgeArtifact`s from the artifact store (filter by `artifact_type == "knowledge"`)
2. Merge their `KirGraph`s into one combined graph
3. Run `DefaultResolver` (Phase 7) — emit warnings for any conflicts
4. `apply_merges()` — remap IDs, remove duplicates, deduplicate relationships
5. `build_ckm()` — denormalize evidence into each object/relationship
6. `CkModel::validate()` — check no dangling relationship from/to IDs
7. Write `model.json` to `.ekos/ckm/`

### Key implementation detail: `ArtifactStore::list()` promoted to trait

`list()` was previously only on `FileSystemArtifactStore`. The semantic compiler needs it through
`Arc<dyn ArtifactStore>`. The fix was to add `list()` to the `ArtifactStore` trait and move the
implementation there. The old concrete method on `FileSystemArtifactStore` was deleted.

This is correct: any backend that implements `ArtifactStore` must be listable, or passes that
scan the store cannot operate against it in tests (where `InMemoryArtifactStore` could be used).

---

## Phase 9 — Knowledge Ledger CLI (`crates/cli/`)

### What was wired (ledger crate was already implemented)

| Command | What it does |
|---|---|
| `ekos compile` | Runs `SemanticCompilerPass`, prints object/rel counts, path |
| `ekos commit` | Reads `model.json`, writes CkmObjects + evidence to ledger |
| `ekos ledger status` | Prints total entries and object count |

The `ekos query object <id>` and `ekos query find <query>` commands already used the ledger (from
the skeleton). No changes needed to `query.rs`.

### Commit flow

`ekos commit` does a one-way projection: `CkmObject → KirObject` + `EvidenceRecord → KirEvidence`.
The ledger's `append_object()` is idempotent — re-running commit after a second `ekos compile`
with no source changes writes zero new entries (all skipped as already present).

---

## Diagnostics additions

Added `has_warnings()` and `warning_count()` to `DiagnosticSink` (compiler-core).

---

## Knowledge Captured

- **Dyn-trait method calls don't need the trait import**: `ctx.artifact_store.list()` where
  `artifact_store: Arc<dyn ArtifactStore>` compiles without `use ekos_artifact::ArtifactStore;`.
  Rust resolves virtual dispatch through the vtable without the caller needing the trait in scope.
  The import IS needed when calling trait methods on a concrete type or generic `T: ArtifactStore`.

- **`CkModel::validate()` is non-fatal**: The semantic pass emits `SEM002` warnings for validation
  failures but does NOT return `Err`. This lets downstream consumers (ledger commit) run even if
  the CKM has minor structural issues, e.g. a relationship whose target was pruned during identity
  resolution. Phase 10+ should decide if validation failures become hard errors.

- **Evidence index key is `KirId.to_string()`**: The `evidence_index: HashMap<String, EvidenceRecord>`
  is keyed by UUID string, not by `KirId`. This is because `HashMap` keys must implement `Hash +
  Eq`, and `KirId` wraps `Uuid` which does implement those, but JSON serialization always produces
  string keys. Using `String` keys makes the JSON `evidence_index` object roundtrip cleanly.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/semantic/src/lib.rs` | Full implementation replacing stub (9 tests) |
| `crates/semantic/Cargo.toml` | Added deps: serde, chrono, ekos-artifact, ekos-identity, etc. |
| `crates/artifact/src/store.rs` | `list()` promoted from concrete to `ArtifactStore` trait |
| `crates/compiler-core/src/diagnostics.rs` | Added `has_warnings()`, `warning_count()` |
| `crates/identity/src/lib.rs` | Added `canonical_id: KirId` to `MergeProposal` |
| `crates/cli/src/commands/compile.rs` | New: `ekos compile` |
| `crates/cli/src/commands/commit.rs` | New: `ekos commit` |
| `crates/cli/src/commands/ledger.rs` | New: `ekos ledger status` |
| `crates/cli/src/commands/mod.rs` | Added compile, commit, ledger modules |
| `crates/cli/src/bin/ekos.rs` | Compile, Commit, Ledger subcommands |
| `crates/cli/Cargo.toml` | ekos-semantic dep |
| `ekos/Cargo.toml` | ekos-semantic workspace member |

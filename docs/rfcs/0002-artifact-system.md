# RFC 0002 — Artifact System and Content-Addressing

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | alexeyban |
| **Created** | 2026-07-02 |
| **Gating** | Phase 2 |

---

## Motivation

Artifacts are the unit of exchange between compiler passes. Content-addressing gives caching,
reproducibility, and dependency tracking for free: if the content hash of an artifact matches a
previously computed result, the pass that produced it can be skipped.

---

## Design

### Content hash

**Algorithm:** SHA-256, hex-encoded (64 chars).

**Input:** The canonical JSON serialization of the artifact's *content fields*, **excluding all
volatile metadata** (`created_at`, `written_at`, wall-clock timestamps). Including timestamps would
mean identical content hashes differently at different times, defeating the cache.

```
artifact_id = SHA-256(canonical_json(content_fields))
```

"Canonical" means: keys sorted alphabetically, no insignificant whitespace, consistent float
representation.

### Artifact types

Five types, all implementing the `Artifact` trait:

```rust
pub trait Artifact {
    fn id(&self) -> &ContentHash;
    fn artifact_type(&self) -> ArtifactType;
    fn dependencies(&self) -> &[ContentHash];
    fn schema_version(&self) -> u32;
}
```

| Type | Purpose |
|------|---------|
| `ObservationArtifact` | Raw facts from one connector scan |
| `KnowledgeArtifact` | Compiled KIR output from one pass |
| `EvidenceArtifact` | Storage wrapper for a `KirEvidence` node |
| `DiagnosticArtifact` | Collected diagnostics for a build run |
| `IndexArtifact` | Named manifest mapping logical names → artifact ids |

### On-disk layout

```
.ekos/artifacts/<first-2-hex>/<full-64-hex-id>.json
```

Mirrors the Git object store layout for efficient filesystem distribution. On write: compute hash,
check existence, skip if present (cache hit). On read: verify hash of content matches filename
before returning.

### Cache invalidation

An artifact is recomputed if any of:
1. Any transitive input artifact hash changed
2. The producing pass has a different version string
3. The pass configuration section in `ekos.toml` changed (hash of the relevant TOML subtree)

### Serialization

JSON v1 (`"schema_version": 1`). Binary formats (FlatBuffers, MessagePack) are a Phase 13
optimization — the current design must not assume binary.

---

## Alternatives Considered

**Including timestamps in the hash:** Rejected — makes cache always miss for identical content.

**Blake3 instead of SHA-256:** Faster, but SHA-256 has better ecosystem tooling. Can be swapped
in Phase 13 if benchmarks justify it.

---

## Open Questions

All resolved.

---

## Acceptance Criteria

- [x] Content-hash algorithm decided (SHA-256, exclude volatile fields)
- [x] All five artifact types named and described
- [x] On-disk path formula defined
- [x] Cache invalidation rules defined
- [x] Serialization format decided (JSON v1)

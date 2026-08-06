# RFC 0006 — Observation SDK and Connector Contract

**Status:** Accepted  
**Author:** alexeyban  
**Created:** 2026-07-02  
**Gating:** Phase 3 (retrospective — implementation preceded RFC)

---

## Motivation

EKOS connectors (file, git, SQL, Confluence, …) are pluggable. The compiler core must be able to
invoke any connector without knowing its implementation details. The SDK defines the boundary.

---

## Design

### `Observer` trait

```rust
#[async_trait]
pub trait Observer: Send + Sync {
    fn name(&self) -> &str;
    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError>;
}
```

Constraints:
- `scan` must not modify the workspace.
- `scan` must be idempotent: same workspace state → same artifact IDs.
- `scan` may be called concurrently with other observers (different targets).

### `ScanContext`

Passed into every `scan` call by the compiler:

```rust
pub struct ScanContext {
    pub workspace_root: PathBuf,
    pub config: ConnectorConfig,       // from ekos.toml [connectors.<name>]
    pub ignore_patterns: Vec<String>,  // from ekos.toml [observe].ignore-patterns
}
```

`ScanContext` does **not** hold a mutable reference to global state. Connectors that need to write
artifacts do so through the return value (`ObservationPackage`), not through side channels.

### `ObservationPackage`

```rust
pub struct ObservationPackage {
    pub observer: String,
    pub target: String,
    pub artifacts: Vec<ObservationArtifact>,
    pub meta: PackageMeta,
}
```

Each `ObservationArtifact` carries a content-addressed ID (Phase 2). The compiler writes artifacts
to the artifact store and indexes them in the build `IndexArtifact`.

### Connector plugin crates

Each connector lives in `plugins/<name>/`. The CLI (or future daemon) imports the plugins it needs.
Connectors are **not** loaded dynamically at runtime in v0.x — static linking only.

### Snapshot output

After observation, the compiler writes an `IndexArtifact` to the artifact store and a snapshot
JSON to `.ekos/snapshots/<iso-timestamp>.json`. This gives build history and allows incremental
rebuilds.

---

## Alternatives Considered

- **Dynamic plugin loading (`.so` / WebAssembly)** — deferred to v1.0; adds significant complexity.
- **Passing `Arc<dyn ArtifactStore>` through `ScanContext`** — rejected for v0.x; connectors write
  through the package return value, keeping them pure and testable.

---

## Acceptance Criteria

- [ ] `Observer::scan` is `async` and `Send + Sync`.
- [ ] `FileObserver` passes all unit tests including same-content → same-artifact-ID.
- [ ] `GitObserver` produces one artifact per commit + one repo metadata artifact.
- [ ] Snapshot JSON written to `.ekos/snapshots/<timestamp>.json` after every `ekos build`.
- [ ] No connector crate imports `ekos-compiler-core`.

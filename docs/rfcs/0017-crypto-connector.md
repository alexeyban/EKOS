# RFC 0017 — Crypto Connector (DeFi Sentinel Ingestion)

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | EKOS team |
| **Created** | 2026-07-20 |
| **Gating** | Phase 5 of the DeFi Sentinel Platform project (external consumer) |

---

## Motivation

The DeFi Sentinel Platform (`~/PycharmProjects/DeFiSentinelPlatform`) is a Python pipeline that
detects Solana/Pump.fun scam tokens and wants to reuse EKOS's Entity Compiler, Semantic Knowledge
Ledger, and Evidence Store rather than build its own graph store. EKOS is a Rust workspace with no
network API and a read-only MCP surface (RFC 0013) — there is no way for an external Python process
to write to the ledger directly. The two systems need a defined boundary.

DeFi Sentinel already produces, on every 15-minute batch, a file-based export (Parquet)
of entities (Wallet, Token, Developer, TelegramChannel, TwitterAccount, Website, Domain, Exchange,
LiquidityPool), relationships (DEPLOYED, PROMOTED, TRANSFERRED, LINKED_TO, MENTIONED, REUSED,
ASSOCIATED_WITH), and evidence records, documented in that repo's
`docs/ekos-export-contract.md` (schema_version 1). This RFC defines the EKOS-side connector that
ingests those files through the normal `ekos build → recover → compile → commit` pipeline.

---

## Design

### Shape: Observer + recovery pass, not the `build.rs` file-object shortcut

Two existing patterns in this codebase could apply:

1. The **inline shortcut** `build.rs` uses for `FileObserver`: observe, then hand-construct
   `KirObject`s and write straight to the ledger inside `build.rs` itself, skipping `recover`.
2. The **general pattern** every other connector uses: `Observer::scan` emits
   `ObservationArtifact`s; a `recovery::CompilerPass` reads them back from the artifact store during
   `recover` and produces a `KirGraph` wrapped in a `KnowledgeArtifact`; `compile` and `commit` then
   promote it into the ledger unchanged, with zero code on their end.

This RFC picks **(2)**, because the crypto export already carries three distinct KIR primitives
(objects, relationships, evidence with cross-references) — the same shape `SqlAnalyzerPass` and
`GitAnalyzerPass` produce — not the single-object-kind case `FileObserver` special-cases. Using the
general pipeline means `compile`/`commit` require **zero changes**: `ckm_rel_to_kir` /
`ckm_object_to_kir` / `evidence_record_to_kir` in `commit.rs` already handle arbitrary
`ObjectKind`/`RelationshipKind` values, including `Custom(String)`.

### Crate layout

```
ekos/plugins/crypto/          # ekos-plugin-crypto
  src/lib.rs                  # CryptoObserver, CryptoExportReader trait, ParquetExportReader
ekos/crates/recovery/src/crypto_analyzer.rs   # CryptoAnalyzerPass
```

### `ekos-plugin-crypto` (Observer)

Follows the constructor-injection pattern from RFC 0012 (`SalesforceObserver` / `SalesforceClient`)
rather than routing through `ScanContext.config` — that TOML `[connectors.<name>]` → `ConnectorConfig`
wiring is declared in `observation-sdk`'s doc comments but **not actually implemented** anywhere in
`compiler-core`/`build.rs` today (checked: no crate reads `config.connectors` from `ekos.toml`, and no
existing observer calls `ScanContext.with_config`). Building that generic plumbing is out of scope
here; inventing crypto-specific config parsing instead would contradict "credential/config assembly is
the caller's job, not the observer's." So:

```rust
#[async_trait]
pub trait CryptoExportReader: Send + Sync {
    /// Read the lexicographically-latest `batch_id=<ts>/` directory under `export_root`.
    /// Returns `Ok(None)` if `export_root` has no batches yet.
    async fn read_latest_batch(&self, export_root: &Path) -> Result<Option<ExportBatch>, CryptoReaderError>;
}

pub struct ExportBatch {
    pub batch_id: String,
    pub entities: Vec<EntityRecord>,
    pub relationships: Vec<RelationshipRecord>,
    pub evidence: Vec<EvidenceRecord>,
}
// EntityRecord / RelationshipRecord / EvidenceRecord field-for-field match the Parquet
// columns in docs/ekos-export-contract.md (DeFiSentinelPlatform repo).

pub struct ParquetExportReader; // real reader, uses the `parquet` crate's row API
pub struct MockCryptoExportReader { pub batch: Option<ExportBatch> } // fixed batch for tests

pub struct CryptoObserver {
    reader: Arc<dyn CryptoExportReader>,
    export_root: PathBuf,
}
impl CryptoObserver {
    pub fn new(reader: Arc<dyn CryptoExportReader>, export_root: impl Into<PathBuf>) -> Self { ... }
}

#[async_trait]
impl Observer for CryptoObserver {
    fn name(&self) -> &str { "crypto" }
    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        // read_latest_batch(&self.export_root); if None, return an empty package (no error —
        // "no export yet" is a normal state, not a failure).
        // One ObservationArtifact per batch: target = batch_id, data = the whole batch
        // serialized as JSON (entities/relationships/evidence arrays + manifest counts).
        // Content-addressed by construction (ObservationArtifact::new hashes `data`), so
        // re-scanning an unchanged batch produces the same artifact id — the Observer
        // contract's "identical remote state → identical artifact IDs" holds without extra work.
    }
}
```

**Dependency**: `parquet` crate (pure Rust, via `arrow-rs`; no native/system libraries, no `bindgen`)
added to `plugins/crypto/Cargo.toml` only — unlike the SAP/Oracle native-dependency problem in RFC
0012, this cannot break `cargo build --workspace` for anyone without an SDK installed, since it has no
system dependency. Only the row-level `parquet::record` API is used (`SerializedFileReader` +
`RowAccessor`); the `arrow` feature/crate itself is not needed.

**Registration**: `build.rs` adds `CryptoObserver` to the observer list only when
`EKOS_CRYPTO_EXPORT_DIR` is set (mirrors `recover.rs`'s `build_llm_provider`, which selects
Anthropic-vs-mock off `ANTHROPIC_API_KEY` at construction time) — soft-skip, not hard-fail, since most
`ekos build` runs in this repo self-observe and have no crypto export to read. If unset, the connector
is simply absent from the observer list for that build.

### `CryptoAnalyzerPass` (recovery)

Modeled directly on `GitAnalyzerPass` (reads artifact IDs from `ctx.artifact_store` during `run()`,
not at construction):

```rust
pub struct CryptoAnalyzerPass {
    pass_id: String,
    batch_artifact_ids: Vec<ArtifactId>,   // crypto ObservationArtifacts found by recover.rs
}

impl CompilerPass for CryptoAnalyzerPass {
    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();
        let mut entity_ids: HashMap<String, KirId> = HashMap::new(); // export entity_id -> KirId

        for artifact_id in &self.batch_artifact_ids {
            let json = ctx.artifact_store.read(artifact_id)?; // batch data written by the Observer
            // 1. Evidence first: one KirEvidence per evidence row, id = deterministic
            //    Uuid::new_v5(NAMESPACE, evidence_id) so re-ingestion is idempotent
            //    (KnowledgeStore::append_evidence dedupes by content hash regardless, but a
            //    stable KirId lets relationships reference it without a lookup table surviving
            //    across passes).
            // 2. Objects: one KirObject per entity row, kind = ObjectKind::Custom(row.kind),
            //    properties = parsed `attrs` JSON, id = Uuid::new_v5(NAMESPACE, entity_id).
            // 3. Relationships: one KirRelationship per relationship row, kind =
            //    RelationshipKind::Custom(row.kind), from/to resolved via entity_ids map,
            //    evidence = the referenced evidence KirIds. A relationship whose src/dst entity
            //    is missing from this batch is skipped with a diagnostic warning, not a hard
            //    error (exports are batch-scoped; an entity can in principle land in a later
            //    batch — same tolerance GitAnalyzerPass has for missing repo artifacts).
        }

        // Write KnowledgeArtifact, same as SqlAnalyzerPass/GitAnalyzerPass.
    }
}
```

`Uuid::new_v5` over the export's own stable `entity_id`/`evidence_id` strings (not a random KirId)
is what makes ingestion idempotent across batches — the same wallet address appearing in twelve
consecutive 15-minute exports must resolve to the *same* `KirObject`, not twelve near-duplicates
merged later by identity resolution. This mirrors `build.rs`'s `Uuid::new_v5(&Uuid::NAMESPACE_URL,
rel_str.as_bytes())` for file paths.

### `recover.rs` wiring

Add a `collect_crypto_artifact_ids` helper alongside the existing `collect_git_artifact_ids`
(same shape: scan the artifact store for `connector_name == "crypto"`, group by `target` = batch_id),
and register one `CryptoAnalyzerPass` per batch found. No LLM involved — unlike `SqlAnalyzerPass`,
crypto data is already typed and named by the producer; there is nothing for an LLM to enrich.

### Testing strategy

- `ekos-plugin-crypto`: unit tests against `MockCryptoExportReader` (no filesystem/Parquet
  dependency) — package structure, one-artifact-per-batch, "no batches yet" → empty package,
  same-content-same-artifact-id.
- A **real-fixture** integration test reads an actual Parquet batch produced by DeFi Sentinel's
  Python exporter, copied into `plugins/crypto/tests/fixtures/`:
  - `sample_batch/` — small, hand-seeded via the real Python `export_batch()` function (not
    invented JSON) so the Parquet encoding is genuine; covers all three relationship kinds
    (DEPLOYED, ASSOCIATED_WITH, LINKED_TO) and both a high-risk and a clean token.
  - `live_batch/` — output of one real `sentinel run pipeline-cycle` against live DexScreener/
    GeckoTerminal APIs (93 real pump.fun-launched entities), proving `ParquetExportReader` parses
    genuine producer output, not just a fixture shaped to fit the reader.
- `CryptoAnalyzerPass`: unit tests build a `KirGraph` from an in-memory `ObservationArtifact`
  (no need to go through the real reader) and assert `ObjectKind::Custom("Wallet")` /
  `RelationshipKind::Custom("DEPLOYED")` come out correctly, evidence is attached, and a
  relationship referencing a missing entity is skipped with a diagnostic rather than failing the
  pass.
- End-to-end: `ekos build && ekos recover && ekos compile && ekos commit` against a temp workspace
  with `EKOS_CRYPTO_EXPORT_DIR` pointing at `sample_batch/`, then `ekos_ekl "FIND Object WHERE
  kind = 'Token'"` / `ekos_neighborhood` confirm the data is queryable with evidence attached.

---

## Alternatives Considered

- **HTTP/gRPC ingestion API instead of file export** — rejected. It would require EKOS to run a
  long-lived write-capable service, contradicting "the Runtime is read-only" and "AI systems consume
  knowledge through the Runtime only" (this isn't an AI system, but the same read-only-surface
  principle applies to any external caller); RFC 0013 already deliberately ships **no** write tools
  over MCP. A file-based Observer keeps the ledger's only write path as `build → recover → compile →
  commit`, unchanged.
- **`build.rs` inline shortcut (like `FileObserver`)** — rejected; see Design above. Would also mean
  the crypto payload's relationships/evidence never flow through identity resolution or the semantic
  compiler, since `build.rs`'s shortcut only ever constructs bare `KirObject`s.
- **Wire `ConnectorConfig`/`ekos.toml` `[connectors.crypto]` parsing now** — rejected for this pass;
  it doesn't exist for *any* connector yet (checked: `ScanContext.with_config` is never called from
  `build.rs`), and building that generic plumbing is a separate, larger RFC. `EKOS_CRYPTO_EXPORT_DIR`
  is a narrower, honest stand-in that doesn't block on it.
  _Tracked as backlog (generic, not crypto-specific): see `TODO.md` → "Promoted from RFC
  Non-Goals" → "MCP / connector infrastructure"._
- **Add the `arrow` crate for full Arrow-batch reading** — rejected; the `parquet` crate's
  `record`/`RowAccessor` API reads the same files without pulling in Arrow's compute/kernel surface,
  which this connector has no use for.

---

## Open Questions

None — every design choice above is fully specified. Two things are explicitly **out of scope**,
named rather than silently absent (see below), not left open:

---

## Acceptance Criteria

- [x] Design is consistent with the Observation SDK contract (`Observer::scan` never mutates the
      workspace; identical batch content → identical artifact id, since `ObservationArtifact::new`
      content-addresses from `data`).
- [x] `compile`/`commit` require no changes — verified by reading `commit.rs`'s
      `ckm_object_to_kir`/`ckm_rel_to_kir`, which already handle `Custom` kinds generically.
- [x] Every relationship in the export is evidence-backed by construction (the Python-side contract
      requires non-empty `evidence_ids`); `CryptoAnalyzerPass` carries that through to `KirRelationship.evidence`
      rather than dropping it.
- [x] Real producer-output fixtures included (`sample_batch`, `live_batch`), not only hand-invented
      JSON — same "near-real data" convention as RFC 0012's Salesforce fixtures.
- [x] What's deliberately deferred is named, not silently absent:
      - **Incremental multi-batch consumption**: each `scan()` reads only the *latest* batch
        directory; historical batches already ingested are naturally no-ops on `commit` (content
        dedup), but a batch that arrives and is superseded before a scan runs is silently missed.
        Fine for the current 15-minute cadence with no gaps expected; revisit if batches need to be
        queued/retained explicitly.
      - **`ekos.toml`-based connector configuration**: this connector reads
        `EKOS_CRYPTO_EXPORT_DIR` directly rather than the (currently unimplemented, workspace-wide)
        `[connectors.<name>]` TOML surface.
      - **Promoting `Wallet`/`Token`/`DEPLOYED`/etc. to first-class `ObjectKind`/`RelationshipKind`
        enum variants**: this RFC ships them as `Custom(String)` only, per the low-risk fallback the
        `kir` crate's own doc comment describes; a follow-up RFC can promote the ones that prove
        durable once more than one producer emits them.

# RFC 0055 — `world.sources` Document Ingestion

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-13

---

## Motivation

Tenth RFC in the continuation: RFC 0047 (claims/temporal validity), RFC 0048 (World Model), RFC
0049 (Agent Model), RFC 0050 (Decision/Action/Simulation Engine), RFC 0051 (Scenario Definition),
RFC 0052 (Conflict Resolution), RFC 0053 (Virtual Social Environment), RFC 0054 (Event Store +
Replay), and now RFC 0055 — the user's explicit choice, closing the one remaining named fork from
RFC 0051: `world.sources` document ingestion, wiring EKOS's real Observer → recovery pipeline into
scenario loading so a scenario's starting world can be seeded from real documents instead of only
`RFC 0051`'s minimal, scenario-authored `events:` block.

Checked before designing, same discipline as the prior nine RFCs — and this is the first RFC in
the continuation to reach outside the graph/simulation layer into the compiler proper:

- **`LocalDocAnalyzerPass` (`ekos-recovery`) is a pure structural pass — no LLM in the loop**,
  confirmed by direct read of its own module doc ("Pure structural mapping — no LLM in the loop,
  same shape as `ConfluenceAnalyzerPass`/`GitHubAnalyzerPass`"). It converts local-document
  observation artifacts (already produced by the real, tested `ekos-plugin-localdocs` connector —
  PDF/DOCX/text/Markdown/HTML/email, per `CLAUDE.md`'s own crate map) into `Custom("Document")`/
  `Custom("Section")`/`Table` KIR objects, each carrying real extracted text in `properties
  ["excerpt"]`. `DocumentSemanticsAnalyzerPass` (LLM-based Concept extraction over that same
  content) is a *separate*, opt-in pass — deliberately not wired here, matching this continuation's
  "deterministic first" discipline (RFC 0050's own Design Principle §4.4) and the compiler's own
  existing `[document-semantics] enabled` opt-in posture.
- **`KnowledgeStore` still has no bulk "every event" query, but `ArtifactStore` does have
  `list()`** — confirmed by reading both traits before assuming either shape. The real pipeline's
  own "gather every `KnowledgeArtifact` a pass produced" pattern (used verbatim by
  `SemanticCompilerPass` in `ekos-semantic`) is directly reusable, not reinvented.
- **`LocalDocsObserver::scan` computes each artifact's path via `abs_path.strip_prefix(root)`** —
  confirmed by reading the connector's own scan loop before assuming a single-file `ScanContext`
  would work. Pointing `ScanContext` directly at one file (root == the file itself) strips to an
  *empty* relative path — a real, silent-corruption risk caught before writing any ingestion code,
  not after a test produced an empty document name. `world.sources`'s explicit allowlist is instead
  honored by scanning the scenario's own directory once, then filtering the resulting artifacts
  down to exactly the requested source paths — recovering exact allowlist semantics without a
  single-file scan mode the connector doesn't have.
- **RFC 0043's redaction baseline is a hard, non-disable-able invariant at every raw-content entry
  point** (`CLAUDE.md`'s own words) — this is a second, independent entry point (`build.rs`'s own
  is the first), so it gets `build.rs`'s exact same choke-point treatment: excluded-path filtering,
  then `redact_json` over every artifact's content, before anything reaches an artifact store.

## Scope

1. **`ScenarioDefinition` gains `world.sources: Vec<String>`** (source document §15's own schema,
   finally implemented) — paths resolved relative to the scenario file's own directory, matching
   `agents:`'s existing convention.
2. **`ingest_sources`** (`crates/simulation/src/ingest.rs`) — for a scenario's listed sources: scan
   the scenario directory once via the real `LocalDocsObserver`, filter to exactly the requested
   paths, apply RFC 0043 redaction, run the real `LocalDocAnalyzerPass` against an ephemeral,
   scenario-load-scoped artifact store (a fresh `tempfile::tempdir()`, torn down when ingestion
   finishes — this pipeline runs fresh on every scenario load, the same posture RFC 0051 already
   gives agents/relationships), and append the resulting `Document`/`Section`/`Table` objects (plus
   their evidence) directly to the scenario's own `&dyn KnowledgeStore` — no CKM, no identity
   resolution, no `commit.rs` (this continuation has never routed simulation data through the CKM
   layer; ingested documents get the same direct-to-ledger treatment every other simulation entity
   already does).
3. **Ingested objects join the scenario's own name registry** — `load_scenario`'s existing two-pass
   "assign names, then resolve references" structure (RFC 0051) gains a third source of named
   entities alongside agents and scenario-authored events: each ingested `Document`/`Section`/
   `Table`'s own `name` (the source path, or `"<path>: section N"`/`"<path>: table N"` for a chunk
   within it — exactly what `LocalDocAnalyzerPass` already names them). An agent's `knowledge:`/
   `relationships:` can reference `world.sources: [reports/report_01.md]` by that same path string,
   or a specific section within it, with zero new resolution logic.

## Non-goals

- **No `DocumentSemanticsAnalyzerPass` (LLM-based Concept extraction).** Real, deferred — a
  scenario's ingested documents carry their real extracted text (`properties["excerpt"]`), which an
  agent can already be `Knows`-linked to and a scenario author can already read; turning that prose
  into structured `Concept` objects and relationships is a genuinely separate, opt-in, LLM-backed
  enhancement layered the same way it already is in the main compiler pipeline, not bundled here.
- **No SQL/Git/GitHub/Confluence/crypto observers, no dialect registry, no fingerprint-based
  incremental caching, no `PassManager` DAG scheduling.** `world.sources` in the source document's
  own example is explicitly document paths, not arbitrary connector configs — this RFC wires
  exactly the one connector (`localdocs`) and exactly the one pass (`LocalDocAnalyzerPass`) that
  claim, not the full `ekos build`/`ekos recover` machinery built for whole-workspace, multi-source,
  incrementally-cached enterprise compilation.
- **No workspace `ekos.toml` `[security]` extension patterns applied to scenario ingestion.**
  Uses `ekos_common::redaction::RedactionConfig::default()` — the built-in baseline alone, which
  already fully satisfies the hard "never observed or stored" invariant. Applying a workspace's own
  *extra* patterns would mean threading `&EkosConfig` through `load_scenario`'s signature, a larger
  change than this RFC's scope justifies; real, deferred work if a scenario author's own org-
  specific secret patterns ever need to apply here too.
- **No incremental/cached re-ingestion.** Every scenario load re-runs the full scan-and-analyze
  pipeline for its listed sources, the same "runs fresh every time" posture RFC 0051 already
  established for agents/relationships/scenario-events — proportionate for the small, throwaway
  scenario ledgers this engine targets, not optimized for large source sets or repeated reloads.

_The `DocumentSemanticsAnalyzerPass`, `[security]` extension patterns, and incremental/cached
re-ingestion are all tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" →
"World Engine". (No SQL/Git/GitHub/Confluence observers etc. is a permanent scope boundary for
this RFC, not deferred work — the main compiler pipeline already covers that ground.)_

## Design

### `ScenarioDefinition.world` (`crates/simulation/src/scenario.rs`)

```rust
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorldDefinition {
    #[serde(default)]
    pub sources: Vec<String>,
}
```

Added as `#[serde(default)] pub world: WorldDefinition` on `ScenarioDefinition` — an omitted
`world:` block behaves exactly as every pre-RFC-0055 scenario already does (empty sources, no
ingestion attempted).

### `ingest_sources` (`crates/simulation/src/ingest.rs`)

```rust
pub(crate) fn ingest_sources(
    store: &dyn KnowledgeStore,
    scenario_dir: &Path,
    sources: &[String],
) -> Result<Vec<KirObject>, IngestError> {
    if sources.is_empty() { return Ok(Vec::new()); }
    // Bridges into async territory only for this opt-in path — see
    // Alternatives Considered for why load_scenario/load_scenario_from_path
    // stay synchronous rather than propagating async through their entire
    // existing call graph (40+ pre-existing call sites, CLI included).
    // Two call shapes are both real (found live, not assumed — see
    // Acceptance Criteria): a plain #[test] fn with no Tokio runtime
    // active, and ekos simulate's own #[tokio::main] entry point, already
    // driving the current thread. block_on-ing a fresh runtime from inside
    // an already-running one panics, so the bridge branches on which case
    // actually applies.
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(ingest_sources_async(store, scenario_dir, sources))
        }),
        Err(_) => {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            runtime.block_on(ingest_sources_async(store, scenario_dir, sources))
        }
    }
}
```

`ingest_sources_async`: one `LocalDocsObserver::with_defaults(Arc::new(TesseractOcr)).scan(&ScanContext::new(scenario_dir))`
call (the same real connector `ekos build` already uses; `TesseractOcr` soft-skips, not hard-fails,
when the `tesseract` binary isn't on `PATH` — confirmed from its own doc comment, so this never
blocks ingestion of non-scanned-image documents); filter `package.artifacts` down to exactly the
requested source paths (erroring `SourceNotFound` on any listed source the scan didn't produce —
a scenario author's typo or missing file is a real, surfaced error, not a silent no-op); RFC 0043's
exact exclusion+redaction sequence; write to a fresh `FileSystemArtifactStore` rooted at a
`tempfile::tempdir()`; run one `LocalDocAnalyzerPass` over the written artifact ids; gather the
resulting `KnowledgeArtifact` (`artifact_store.list()` + deserialize + `artifact_type ==
ArtifactType::Knowledge`, the exact pattern `ekos-semantic`'s own `SemanticCompilerPass` already
uses); append every evidence/object/relationship in it directly to `store`; return the objects for
name registration.

### `load_scenario`/`load_scenario_from_path` (`crates/simulation/src/scenario.rs`)

`load_scenario` gains a fourth parameter, `world_objects: &[KirObject]` — already-ingested,
already-appended objects (so the *pure* core stays testable without real files: a unit test can
hand it a small hand-built `Vec<KirObject>` instead of needing documents on disk). Pass 1 (name
registration) gains a third loop registering each `world_objects` entry by its own `name`,
alongside agents and scenario-events, in the same duplicate-name-checked registry. `world_objects`
are *not* re-appended to the store here — `ingest_sources` already did that; this loop only
registers names.

`load_scenario_from_path` calls `ingest_sources(store, base_dir, &scenario.world.sources)` (using
the same `base_dir` agent files already resolve against) before calling `load_scenario`, threading
the result through as the new fourth argument.

## Alternatives Considered

- **Reimplementing document parsing directly in `ekos-simulation`** (skip the Observer/recovery
  pipeline, hand-roll Markdown/text extraction) — rejected outright; `ekos-plugin-localdocs`
  already handles PDF/DOCX/text/Markdown/HTML/email, tables, OCR, and chunking, tested and real.
  Re-implementing a subset would be a real quality regression and directly contradicts the
  "reuse over reinvent" discipline this whole continuation has followed — RFC 0051 itself named
  this exact pipeline as the thing to eventually wire, not replace.
- **Making `load_scenario`/`load_scenario_from_path` `async fn`** to avoid a runtime-bridge inside
  `ingest_sources` — rejected; would ripple through every existing call site (40+ test functions
  across `scenario.rs`/`scenario_fixture.rs`, plus `ekos simulate`'s CLI command and its dispatch in
  `bin/ekos.rs`) for a capability only exercised when `world.sources` is non-empty. A contained,
  documented `tokio::runtime::Builder::new_current_thread()` bridge inside the one opt-in code path
  keeps every existing signature and test unchanged.
- **Pointing `ScanContext` at each individual source file** — rejected once traced through
  `LocalDocsObserver::scan`'s own `strip_prefix(root)` logic: a single-file root strips to an empty
  relative path, corrupting the resulting document's name silently. One scenario-directory-rooted
  scan, filtered post-hoc to the requested paths, avoids this entirely.
- **Applying `DocumentSemanticsAnalyzerPass` too, for richer Concept-level knowledge** — rejected
  for this RFC; a genuinely separate, LLM-backed, opt-in enhancement, not required to prove
  `world.sources` ingestion works, and the pipeline already treats it as optional upstream of this
  RFC. Real, deferred work.
- **Threading a real `&EkosConfig` through for workspace-specific redaction extensions** — rejected;
  the built-in baseline alone already satisfies the hard invariant; extending it is real, deferred
  work gated on someone actually needing org-specific patterns inside a scenario's ingested content.

## Testing

- `simulation` unit tests (`ingest.rs`): a single-source scenario directory (one real `.md` file on
  disk in a tempdir) ingests into a `Document` object with the expected `properties["excerpt"]`;
  a listed source that doesn't exist produces `IngestError::SourceNotFound`, not a silent skip; a
  sibling `.md` file in the same directory *not* listed in `sources` is never ingested (proving the
  post-scan filter genuinely restricts to the allowlist, not "everything under scenario_dir");
  redaction is exercised (a source file containing a recognizable secret pattern has it stripped
  before any object/evidence reaches the store — proving RFC 0043's choke point applies here too,
  not just in `build.rs`).
- `simulation` integration test (`scenario_fixture.rs`, extended): a scenario file with a real
  `world.sources` entry plus an agent whose `knowledge:` references that same source path by name —
  loads end-to-end, and the agent's own `agent_observation` includes the ingested `Document`
  object, proving the full "listed in world.sources, named the same, resolvable from an agent's
  knowledge:" loop.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `ScenarioDefinition.world.sources` implemented; an omitted `world:` block is fully
      backward-compatible with every pre-RFC-0055 scenario file.
- [x] `ingest_sources` reuses the real `ekos-plugin-localdocs` connector and `LocalDocAnalyzerPass`
      unmodified — no document-parsing logic reimplemented in `ekos-simulation`.
- [x] RFC 0043 redaction is applied at this entry point, verified by
      `redaction_strips_a_recognizable_secret_before_it_reaches_the_store`, not just by
      construction.
- [x] `world.sources`'s allowlist is exact — verified by
      `only_listed_sources_are_ingested_not_every_document_in_the_directory`.
- [x] An agent's `knowledge:`/`relationships:` can reference an ingested document by the same path
      string listed under `world.sources`, verified end-to-end by
      `world_sources_document_is_ingested_and_referenceable_by_an_agents_knowledge` and live against
      the real `ekos simulate` CLI command.
- [x] No LLM call anywhere in this change (`LocalDocAnalyzerPass` only) — confirmed out of scope,
      not partially started.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.

A real bug was found and fixed via live CLI testing, not caught by the test suite alone: the
original `ingest_sources` unconditionally built a fresh Tokio runtime and called `block_on` on it.
Every unit/integration test runs outside any Tokio runtime, so this worked in every automated test
— but `ekos simulate`'s own `main` is `#[tokio::main]` (multi-thread by default), and calling
`Runtime::block_on` from *inside* an already-running runtime panics ("Cannot start a runtime from
within a runtime"), confirmed live before the fix. Corrected to branch on
`tokio::runtime::Handle::try_current()`: inside an existing (multi-thread) runtime, bridge via
`tokio::task::block_in_place` + the current handle; otherwise (a plain `#[test]` function), spin up
a throwaway current-thread runtime as originally planned. This is exactly the kind of gap this
session has now hit twice — RFC 0054's `relationships_at` bug was invisible to every test until the
right *shape* of usage exercised it; here, no automated test happened to run inside a Tokio runtime
at all, so a real, would-have-shipped bug only surfaced by actually running the CLI command by hand.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0055-world-sources-document-ingestion.md` | This RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/Cargo.toml` | New dependencies: `ekos-observation-sdk`, `ekos-plugin-localdocs`, `ekos-recovery`, `ekos-artifact`, `ekos-compiler-core`, `ekos-common`, `tokio`; `tempfile` promoted from dev- to a regular dependency |
| `ekos/crates/simulation/src/ingest.rs` | New: `ingest_sources` (with the `Handle::try_current` runtime-nesting fix), `IngestError`; 4 unit tests |
| `ekos/crates/simulation/src/scenario.rs` | `WorldDefinition`; `load_scenario` gains `world_objects` parameter; `load_scenario_from_path` wires ingestion in |
| `ekos/crates/simulation/src/lib.rs` | `pub mod ingest;` + re-exports |
| `ekos/crates/simulation/tests/scenario_fixture.rs` | Extended: a real `world.sources` + agent-`knowledge:` end-to-end test |

# Devlog 55 — RFC 0055: world.sources document ingestion, and a runtime-nesting bug only the real CLI could find

**Date:** 2026-08-13
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Tenth RFC in the continuation: RFC 0047 through RFC 0054, and now RFC 0055 — the user's explicit
choice, closing the one remaining named fork from RFC 0051: `world.sources` document ingestion.
This is the first RFC in the continuation to reach outside the graph/simulation layer into the real
compiler pipeline, wiring the actual `ekos-plugin-localdocs` connector and `LocalDocAnalyzerPass`
(deterministic, no LLM) into scenario loading, so a scenario's starting world can be seeded from
real documents instead of only hand-authored YAML. The design stayed proportionate throughout —
exactly one connector, exactly one pass, no CKM, no identity resolution, no LLM — but the
implementation still surfaced a real bug the entire automated test suite was structurally blind to:
a naive sync-to-async bridge that only broke inside `ekos simulate`'s own `#[tokio::main]` entry
point, something no `#[test]` function could ever exercise.

---

## RFC 0055 — `world.sources` Document Ingestion

### Problem / motivation

Checked before designing, same discipline as the prior nine RFCs:

- `LocalDocAnalyzerPass` is confirmed pure structural (no LLM) by its own module doc — exactly the
  "deterministic first" shape this continuation has held to since RFC 0050's Design Principle §4.4.
  `DocumentSemanticsAnalyzerPass` (LLM-based Concept extraction) is a separate, opt-in pass upstream
  of this RFC — deliberately not wired here.
- `LocalDocsObserver::scan` computes each artifact's path via `abs_path.strip_prefix(root)` —
  confirmed by reading the connector's own scan loop *before* writing any ingestion code. Pointing
  `ScanContext` at a single source file directly (the natural first instinct, matching how
  `world.sources` lists individual files) would strip to an empty relative path, silently
  corrupting every ingested document's name. Caught before it shipped, not after a test failed
  unexplainably — the same "check the real implementation before assuming an API shape" discipline
  RFC 0053 applied to `KnowledgeStore`'s missing bulk event query.
- RFC 0043's redaction baseline is a hard, non-disable-able invariant at *every* raw-content entry
  point — this is a second, independent one, so it gets `build.rs`'s own exact choke-point treatment
  (exclude, then redact), not a lighter-weight approximation.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0055-world-sources-document-ingestion.md` |
| `ScenarioDefinition.world.sources` | `crates/simulation/src/scenario.rs` |
| `ingest_sources` (real Observer + `LocalDocAnalyzerPass`, RFC 0043 redaction) | `crates/simulation/src/ingest.rs` |
| `load_scenario`'s `world_objects` parameter | `crates/simulation/src/scenario.rs` |

**Deliberately not built**, per the RFC's own Non-goals: `DocumentSemanticsAnalyzerPass` (LLM
Concept extraction — real, deferred, opt-in the same way it already is upstream); SQL/Git/GitHub/
Confluence/crypto observers, dialect registries, fingerprint-based incremental caching, or
`PassManager` DAG scheduling (`world.sources` in the source document's own example is document
paths, not arbitrary connector configs — this RFC wires exactly the one connector and one pass that
claim covers); workspace `ekos.toml` `[security]` redaction extensions (built-in baseline only);
incremental/cached re-ingestion (every scenario load re-runs the pipeline fresh, the same posture
RFC 0051 already gives agents/relationships).

### Why the allowlist needed a scan-then-filter design, not a per-file scan

`world.sources: [reports/report_01.md]` reads like an explicit allowlist a caller would naturally
implement by scanning each listed file directly. That instinct breaks against `LocalDocsObserver`'s
actual implementation: `rel_path = abs_path.strip_prefix(root)`, and if `root` is set to the file
itself, `strip_prefix` returns an empty string. The fix scans the scenario's own directory *once*
(recovering a real, non-empty relative path for everything found), then filters the resulting
artifacts down to exactly the requested paths — and separately verifies every *requested* path was
actually found, turning a scenario author's typo or missing file into a real `SourceNotFound`
error rather than a silent partial ingestion.

### The bug only the real CLI could find

Every one of `ingest_sources`'s own tests passed on the first try — four unit tests plus an
end-to-end integration test proving a scenario's `world.sources` document is ingested and
resolvable from an agent's `knowledge:` list. Running the same scenario through the actual `ekos
simulate` command immediately panicked: *"Cannot start a runtime from within a runtime."*
`ingest_sources` unconditionally built a fresh Tokio runtime and called `block_on` — correct from a
plain `#[test]` function (no Tokio runtime active), but `ekos simulate`'s own `main` is
`#[tokio::main]` (multi-thread by default), already driving the current thread when `ingest_sources`
runs inside it. No automated test in this entire crate runs *inside* a Tokio runtime, so nothing in
`cargo test` could ever have caught this — it took literally running the CLI command by hand.
Fixed by branching on `tokio::runtime::Handle::try_current()`: inside an existing runtime, bridge
via `tokio::task::block_in_place` and the current handle; otherwise, build a throwaway current-
thread runtime as originally planned. Verified live afterward: the same scenario now ingests its
document correctly and prints a real `local-docs-analyzer complete... objects=2 edges=1` log line.

This is the second RFC running where a real bug was invisible to the entire test suite until the
right *execution context* exercised it — RFC 0054's `relationships_at` needed a relationship
updated more than once at a historical query point; this one needed a call from inside an active
async runtime. Neither gap was reachable by adding more unit tests in the same shape as the ones
already there; both needed a different kind of check (a cross-backend regression test for the
first, an actual CLI invocation for the second).

### Decisions (alternatives considered, why this choice)

- **Reimplementing document parsing directly in `ekos-simulation`** — rejected outright;
  `ekos-plugin-localdocs` already handles PDF/DOCX/text/Markdown/HTML/email, tables, OCR, and
  chunking, tested and real. RFC 0051 itself named this exact pipeline as the thing to eventually
  wire, not replace.
- **Making `load_scenario`/`load_scenario_from_path` `async fn`** — rejected; would ripple through
  40+ existing call sites for a capability only exercised when `world.sources` is non-empty. The
  runtime-bridge (once correctly handling both call contexts) keeps every existing signature and
  test unchanged.
- **Pointing `ScanContext` at each individual source file** — rejected once traced through
  `LocalDocsObserver::scan`'s own path-computation logic; a single-file root silently corrupts the
  resulting document's name.
- **Applying workspace `[security]` redaction extensions** — rejected for this RFC; the built-in
  baseline alone already satisfies the hard invariant; extending it would mean threading `&EkosConfig`
  through `load_scenario`'s signature, a larger change than justified here.

---

## Knowledge Captured

- **A sync-to-async bridge needs to handle both "no runtime" and "already inside a runtime" —
  testing only the first is not enough, because nothing in a plain `#[test]` suite ever exercises
  the second.** `tokio::runtime::Handle::try_current()` is the standard, correct way to detect
  which case actually applies; `tokio::task::block_in_place` is the correct bridge once already
  inside a multi-thread runtime (the default for `#[tokio::main]` with no explicit flavor).
- **A connector's own path-computation logic (`strip_prefix(root)`) is exactly the kind of detail
  worth reading before designing a caller around an assumed API shape.** The "scan once, filter to
  the allowlist" design was cheaper and more correct than the "scan each file individually" instinct
  it replaced, and was only found by checking, not assumed.
- **Real end-to-end CLI verification remains load-bearing even after a full green test suite** —
  this is now the second RFC in a row (RFC 0054's ledger fix, this RFC's runtime fix) where a
  passing `cargo test --workspace` did not mean the feature actually worked end to end, and only
  running the real command surfaced the gap.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0055-world-sources-document-ingestion.md` | New RFC, all Acceptance Criteria checked |
| `ekos/crates/simulation/Cargo.toml` | New: `ekos-observation-sdk`, `ekos-plugin-localdocs`, `ekos-recovery`, `ekos-artifact`, `ekos-compiler-core`, `ekos-common`, `tokio`; `tempfile` promoted to a regular dependency |
| `ekos/crates/simulation/src/ingest.rs` | New: `ingest_sources` (with the `Handle::try_current` runtime-nesting fix), `IngestError`; 4 unit tests |
| `ekos/crates/simulation/src/scenario.rs` | `WorldDefinition`; `load_scenario` gains `world_objects`; `load_scenario_from_path` wires ingestion in |
| `ekos/crates/simulation/src/lib.rs` | `pub mod ingest;` + re-exports |
| `ekos/crates/simulation/tests/scenario_fixture.rs` | New end-to-end `world.sources` + agent-`knowledge:` test |

## Still open (tracked, not silently dropped)

- **No unscoped fork remains from the original RFC 0047-0055 continuation's own named list.** Every
  fork flagged along the way (Phase 9, Phase 11, `world.sources`) has now been either built or
  explicitly deferred with a stated reason. Phase 14+ (Metrics, Turning Point Detection, Report
  Generation, and beyond) haven't been scoped at all — a fresh decision point, not a carried-over one.
- **No `DocumentSemanticsAnalyzerPass` wiring** — real, deferred, LLM-backed, opt-in.
- **No workspace `[security]` redaction extensions applied to scenario ingestion** — built-in
  baseline only; real, deferred if ever needed.

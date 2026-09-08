# RFC 0140 — Source-grounded retrieval: precise spans, and reading the source on demand

**Status:** Proposed
**Author:** EKOS team
**Created:** 2026-09-08
**Relationship to RFC 0139:** RFC 0139 raised answer correctness 36.2% → 42.5% by fixing the ruler,
the query, and the prompt. It also established where the remaining ceiling is: three of six
identifier-lookup scenarios expect retrieval to surface an object whose name the question never
uses. No lexical index can do that. This RFC is about giving retrieval and reasoning access to what
the compiler already recorded — the actual source text behind an entity — rather than only its name
and a file path.
**Relationship to RFC 0043:** the hard constraint. Secrets/PII are never observed or stored, and
redaction is a *prevention* control at every raw-content entry point. §4 exists because the obvious
implementation of "read the source at query time" would open a new entry point that bypasses it.

---

## Motivation

Every claim the reasoner sees today is a name and, at best, a file path. Measured on the RFC 0138
suite (1,289 evidence claims across a full run):

| | |
|---|---|
| claims carrying **any** source location | 340 / 1,289 (**26.4%**) |
| claims carrying a precise `path:line` | **0** |

So when the model is told *"`parse_ddl_structural` — search match"*, it is being asked to answer
questions about code it has never been shown. It knows a symbol exists and which file it lives in.
It cannot see what it does.

The gap is not that the compiler failed to capture this. Most of it is already there, and unused:

| capability | status | consumed by |
|---|---|---|
| `SourceLocation { path, line, column }` | exists on every `KirEvidence` | — |
| … actually populated with a line | **2 of 35 call sites** (`dbt_analyzer`, `llm_description`) | — |
| `source_span { start_line, end_line }` on symbols | **exists** (RFC 0088) — `rust_analyzer`, `elixir_analyzer`, `python_analyzer` | `docs-gen`, `commit` only |
| `CkmObject::source_artifact_ids` | **exists** (RFC 0135 Part B) | provenance/audit only |
| `ArtifactStore::read(id)` — content by id | **exists**, content-addressed | the compiler |

**The search and answer path reads none of them.** `grep -rn source_span crates/runtime crates/ledger`
returns nothing. An entity's link to its original text is recorded at compile time and then dropped
on the floor at query time.

---

## Design

### 1. Give a symbol a link to its source text at all

The initial framing of this section was wrong, and the code said so. `rust_analyzer.rs:117` states
it plainly: *"this crate emits no evidence/`SourceLocation` at all"*. So a Rust symbol carries
`source_span {start_line: 200, end_line: 322}` and **no file path** — the path exists only as an
id-hash ingredient inside `parse_rust_file`. Verified live on `parse_ddl_structural`: five facts, a
real span, no location on any of them.

A line range with no file cannot be opened, cited, or re-read. This is not imprecision; the link is
absent. And it affects the largest analyzer family in this repo — 2,604 Rust symbols in a full
`recover`.

Fix: the analyzer emits one `KirEvidence` per span-carrying symbol, with
`SourceLocation::at(path, start_line)` and a fragment sliced from the source it already holds. The
fragment is capped (40 lines) while `source_span` keeps the true range, so a consumer wanting the
whole body can still go and read it.

The fragment is safe to persist precisely because `data.source` reached the analyzer through the
observation layer and has already passed RFC 0043 redaction — the same reasoning that makes §3
insist query-time reads come from the artifact store rather than the live filesystem.

**Status: implemented for `rust_analyzer`, `python_analyzer` and `elixir_analyzer`.**

**A change to this code does not take effect until the pass version is bumped.** Learned by losing
a full 42-minute `recover`/`resolve`/`compile`/`commit` to it on 2026-09-08. `PassManager::run_all`
skips a pass when `manifest.version == pass.version()` (`compiler-core/src/cache.rs`'s
`should_recompute`), and `CompilerPass::version` has a **trait default of `"v1"`** that none of
these three analyzers overrode. `cache_inputs` fingerprints the *artifacts being read*, never the
logic reading them — so with an unchanged corpus, the analyzers were permanently cached and no code
change could ever invalidate them.

The failure is silent and looks like success: every stage exited 0, and `recover` reported

```
Passes run: 0
Passes skipped (cached): 9
Rust symbols recovered: 0 total, 0 Calls edges
```

while faithfully rebuilding the ledger from pre-change KIR. Note RFC 0135 Part A fixed exactly this
hazard for the `build`/observation stage (`PIPELINE_LOGIC_VERSION`); the `recover` pass cache had
the same hazard unaddressed.

Fixed by giving all three analyzers an explicit `version()` of `"v2"`, plus a guard test
(`source_evidence.rs::analyzers_emitting_source_evidence_declare_a_non_default_pass_version`)
asserting they have moved off the default — verified to fail when a version is reverted, not merely
to pass. The guard cannot check that a bump happened for the *right* reason; that stays review's
job.

#### The CKM discards evidence line numbers (found 2026-09-08, not yet fixed)

`semantic/src/lib.rs:377` flattens `KirEvidence` into `EvidenceRecord` with:

```rust
source: ev.location.path.clone(),
```

`SourceLocation` carries `path`, `line` and `column`; **only `path` survives compilation.** So §1
attaches a line at `recover` time and `compile` throws it away.

For a span-carrying symbol this is masked — §2 re-derives `path:start-end` from the object's own
`source_span`, so the rendered claim still gets a line. It is **not** masked for the two analyzers
that set a real evidence line with no corresponding span, `dbt_analyzer` (`line_at(content, …)` for
a Jinja `ref()`/`source()` macro) and `llm_description`: for those the line is destroyed
permanently and no downstream consumer can recover it.

Fix (deferred to the next batched rebuild, per this RFC's own Verification note): add
`line: Option<u32>` to `EvidenceRecord` as an **additive** `#[serde(default)]` field rather than
reformatting `source` into `"path:line"` — `source` is an established string contract, and
`source_artifact_ids` set the precedent for evolving this struct additively.

### 2. Surface the span through the answer path

`reason::entity_item` reads `source_span` when the evidence location has no line of its own, and
renders `path:start-end`. Nothing new is captured; a property the compiler already writes stops
being invisible to the reasoner.

**Implemented, and inert until §1 lands in a rebuilt ledger** — it upgrades an existing evidence
location, and span-carrying symbols currently have no evidence to upgrade. Measured after the
change with the existing ledger: claims with a line number stayed at 0, which is the correct
outcome for a fix whose input does not exist yet. It takes effect on the next
`recover`/`compile`/`commit`.

### 3. On-demand source text in the evidence set

For the top *k* retrieved entities (k small — this is the expensive tier), attach the actual source
text of their span as an `EvidenceItem`, so the model reasons over code rather than over names.

**Where the text comes from is the whole design.** It must be the **artifact store**
(`ArtifactStore::read`, via `source_artifact_ids`), never the live filesystem:

- artifacts are content-addressed and immutable, so a citation stays reproducible;
- artifact content has **already passed RFC 0043 redaction** at observation time.

Reading the file from disk at query time would be a new raw-content entry point that bypasses
redaction entirely — an answer could surface a secret that the ledger correctly refused to store.
That is a violation of RFC 0043's prevention model, not a performance trade-off, and §5 lists it as
a non-goal for that reason.

### 4. LLM-assisted rerank (opt-in, measured, off by default)

With real source text available, a second-stage rerank becomes possible: ask the model which of the
top *n* candidates actually answers the question, then reorder before assembling the evidence set.

This directly targets RFC 0139's identified ceiling — *"the content-addressable, checksummed unit of
raw observed data"* cannot be matched lexically to `ObservationArtifact`, but is trivial to match
from the type's own doc comment and definition.

Constraints, each of which has bitten this project before:

- **Determinism.** RFC 0126's CI gate assumes reproducible ranking. An LLM rerank is not
  reproducible, so it must be excluded from that gate's path and gated behind config
  (`[retrieval] rerank = "llm"`, default off), exactly as `[embeddings]` is.
- **Latency and cost.** P95 is already 36 s/scenario locally. A rerank adds a call per query. It
  should be bounded (top *n* only) and measured on the RFC 0138 suite before being recommended.
- **Measure both directions.** RFC 0139 established the pattern the hard way: three separate
  changes reached a perfect adversarial score while collapsing legitimate answer correctness.
  A rerank must be evaluated on fabrication *and* on answer correctness, not the metric it targets.

---

## Non-goals

- **Reading source from the live filesystem at query time.** §3 explains why: it bypasses RFC 0043
  redaction. Source text comes from the artifact store or not at all.
- **Making the LLM rerank the default.** It is non-deterministic and costs a call per query; it
  ships off, and the eval suite decides whether it earns being on.
- **Re-capturing spans that analyzers do not already compute.** Analyzers without a span
  (SQL, git, confluence, localdocs) keep file-level locations in this RFC. Extending span capture to
  them is separate work with its own per-analyzer cost.
- **Changing the ledger format.** Everything here reads data the compiler already writes.

---

## Verification

- `cargo test --workspace` including RFC 0126's retrieval gate, which §4 must not enter.
- Re-measure the RFC 0138 suite per phase, comparing under a **fixed ruler version** (RFC 0139 §2):
  §1/§2 should move citation precision and answer correctness without touching fabrication; §4 must
  be judged on answer correctness *and* fabrication together.
- Direct check on the three vector-dependent scenarios (`arch-017`, `lin-009`, `arch-007`) that
  RFC 0139 identified as unreachable lexically — they are this RFC's clearest success signal.

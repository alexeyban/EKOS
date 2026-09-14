# RFC 0140 — Source-grounded retrieval: precise spans, and reading the source on demand

**Status:** Accepted — §1/§2 shipped devlog_173 (2026-09-08); §3/§4 shipped 2026-09-14
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

#### The pipeline discarded evidence line numbers in two places (found and fixed 2026-09-08)

`semantic/src/lib.rs:377` flattened `KirEvidence` into `EvidenceRecord` with:

```rust
source: ev.location.path.clone(),
```

`SourceLocation` carries `path`, `line` and `column`; **only `path` survived compilation.** So §1
attached a line at `recover` time and `compile` threw it away.

It was a **two-hop** loss, and either hop alone would have kept the line just as lost while looking
correct in isolation — `commit.rs:533` then rebuilt the location with an unconditional
`SourceLocation::file(ev.source)`, which cannot re-narrow a file-level location no matter what
`compile` had done. This is why the regression test asserts on the full
`recover → compile → commit` round trip rather than on either conversion.

For a span-carrying symbol this is masked — §2 re-derives `path:start-end` from the object's own
`source_span`, so the rendered claim still gets a line. It is **not** masked for the two analyzers
that set a real evidence line with no corresponding span, `dbt_analyzer` (`line_at(content, …)` for
a Jinja `ref()`/`source()` macro) and `llm_description`: for those the line is destroyed
permanently and no downstream consumer can recover it.

**Fixed:** `EvidenceRecord` gained `line: Option<u32>` as an **additive** `#[serde(default)]` field
rather than reformatting `source` into `"path:line"` — `source` is an established string contract,
and `source_artifact_ids` set the precedent for evolving this struct additively. `compile` now
carries `ev.location.line` across, and `commit` restores `SourceLocation::at(path, line)` when one
is present, `::file` otherwise (a missing line must never be invented — the second test covers
that). `SemanticCompilerPass::version` is bumped to `"v2"` accordingly, since a CKM compiled by
`v1` carries no line on any evidence record and must not be reused from cache.

**Not yet re-measured.** The code is correct and tested, but the line only reaches the ledger after
a `compile` + `commit`, which this change has not yet had. Per this RFC's Verification note it
should ride the next batched rebuild alongside RFC 0141's work rather than pay a third rebuild.

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

**Shipped 2026-09-14.** `source_artifact_ids` turned out to reach the ledger only through
`WriteContext`/the audit trail (`commit.rs`'s `per_source_ctx`), not as a directly queryable
object property — so `Runtime` gained a thin `audit_trail(id)` passthrough
(`KnowledgeStore::audit_trail`, RFC 0135 Part B), and the most recent record's
`source_artifact_id` is what `ArtifactStore::read` is keyed on. `reason.rs::attach_source_text`
takes the first `top_k` *distinct* entities already named in an assembled `EvidenceSet`
(deduplicated, so the same entity referenced by two items is only read once), and for each one
with a `source_span` (RFC 0088 — Rust/Python/Elixir symbols only, per this RFC's own Non-goals),
appends one additional `EvidenceItem` carrying the real, uncapped (well, capped at a generous 400
lines — `MAX_SOURCE_TEXT_LINES` — rather than `source_evidence`'s 40-line compile-time cap) span
text. An entity with no span, no recorded artifact, or an unreadable/wrong-shaped artifact is
silently skipped — best-effort, never a hard error.

Wired into `AiRuntime` as an opt-in builder (`AiRuntime::with_artifact_store`), not a required
constructor argument — every pre-existing `AiRuntime::new` call site (17+ of them, mostly tests)
is unaffected, since `artifact_store` defaults to `None` and the enrichment is then simply
inert. `ask.rs`/`eval.rs` both wire it in by best-effort (`if let Ok(store) = ...`), so a
workspace with no artifact store yet still answers exactly as before this RFC. `gather_evidence`'s
own "offline, no LLM" doc-stated contract is preserved — reading from the artifact store is a
local disk read, not a network/LLM call, so §3 does not turn `gather_evidence` into something
`agent_runner.rs`'s trajectory logic or `ekos ask --explain` can no longer call synchronously.

**Measured 2026-09-14 (devlog_182)**: real `recover`/`resolve --force`/`compile`/`commit` +
`ekos eval run --agent ollama` (`evals/reports/20260914T154459Z-ekos-full.json`). Answer
correctness/groundedness landed bit-identical to the pre-fix baseline (expected — `temperature: 0`
means an unchanged prompt produces an unchanged completion locally); the one metric that moved,
`recall_at_10` (52.9% → 47.1%), traces to a single scenario (`code-002`) whose recall flipped from
a false 1.0 to an honest 0.0 — RFC 0139 Phase 2's fix correcting a metric that had been grading
the wrong query. §3 itself (this section) is unit-tested (5 new tests in `reason.rs`) but its
real-world effect on the suite is entangled with everything else changed this session; no scenario
was isolated as "won because of §3 specifically."

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

**Shipped 2026-09-14.** `[retrieval] rerank = "llm"` (a string, not a bool — same shape as
`[embeddings] provider`, leaving room for a future non-LLM reranker with no breaking config
change) gates a new `AiRuntime::rerank_evidence`, called from `reason_with_history` — never from
`gather_evidence`, `retrieve()`, or anywhere RFC 0126's CI gate or `agent_runner.rs`'s offline
trajectory capture can reach it, so the determinism constraint above is structural, not just a
config default. The call shows the model a numbered list of the top `rerank_candidates` (default
10) evidence claims and asks for `{"relevant_indices": [...]}` in relevance order — reordering
only, never dropping an item, matching this section's own "reorder... assembling the evidence
set" framing rather than a filter. Parses the response with the same last-block-first,
brace-depth-aware scanner `extract_citations` already uses (`balanced_json_spans`), for the same
reason: a rerank response wrapped in prose or a fenced block must not be mistaken for a parse
failure. **Best-effort by construction**: an LLM error or an unparseable/all-out-of-range response
leaves the evidence set completely untouched — a failed rerank degrades to exactly this RFC's own
`rerank_llm: false` default, never surfaces as an error the caller must handle.

**Still genuinely not measured.** A real `[llm]` provider and a full `ekos eval run` are both now
available and were used for §1-§3 (devlog_182, 2026-09-14), but `[retrieval] rerank` was left
unset for that run — §4 is off by default and nothing in this pass turned it on, so it was never
exercised against the suite. Implemented and unit-tested (10 new tests in `ai.rs`):
response parsing (valid, prose-wrapped, out-of-range/duplicate indices, unparseable), reorder
semantics (named items move to front in order, unmentioned items keep relative position, empty
order is a no-op), and the two integration paths (off by default never calls the model; on,
reorders using the model's real response; on, a bad response leaves the set unchanged).

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

**Done as of 2026-09-14**: `cargo test --workspace` (including RFC 0126's retrieval gate — §4
confirmed structurally unable to enter it, not merely tested against it once) / `clippy -D
warnings` / `fmt --check` all clean, with 15 new unit tests across §3/§4 covering the specific
failure modes each section's own text calls out. A real `recover`/`resolve --force`/`compile`/
`commit` + `ekos eval run --agent ollama` ran the same day (devlog_182) — the suite result is
entangled across everything shipped this session (see RFC 0141's own Verification note for the
one scenario, `code-002`, whose retrieval was directly traced), and per-phase attribution under a
fixed ruler version was not done separately. **Still not done**: §4 was never turned on for that
run (`[retrieval] rerank` unset), so it remains genuinely unmeasured; the direct
`arch-017`/`lin-009`/`arch-007` check needs `[embeddings]` enabled, also not configured in the
workspace that was measured.

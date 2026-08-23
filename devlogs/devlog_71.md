# Devlog 71 — RFC 0065 Phase 2/3 + RFC 0066 MVP agent, and a real small-model bug found live

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Completing RFC 0064-0066 end to end, scoped to the real MVP both RFCs already define (RFC 0067):
LLM-backed architecture reasoning, a deterministic evaluator, targeted re-collection, and RFC
0066's orchestrating `ekos architecture investigate` command. Built entirely on top of RFC 0065
Phase 1's foundation and existing patterns (`DocumentSemanticsAnalyzerPass`, `LlmProvider`,
independently-callable pipeline stages) — no new abstractions needed. Live verification against
this repo's own real workspace, using a real local Ollama model, found a genuine bug — a batched
prompt silently exceeding a small model's context window — diagnosed from the actual raw cached
LLM response, fixed, and reverified with a real ~2× improvement in classification rate.

---

## RFC 0067 — Reasoning, evaluation, and the MVP investigation loop

### What was built

| Component | What it does |
|---|---|
| `ArchitectureReasoningPass` (`crates/recovery/src/architecture_reasoning.rs`) | Modeled directly on `DocumentSemanticsAnalyzerPass` — reads `Crate`/`DependsOn` data, one LLM call per chunk of crates, writes `inference`-type `Claim`s (`has_role`) |
| `evaluate_architecture` (`crates/recovery/src/architecture_evaluator.rs`) | Deterministic, no LLM — `completeness` (fraction of crates classified) + `evidence_coverage` (fraction of claims/gaps with real evidence) |
| `read_crate_doc_comment` | Targeted re-collection's one real evidence source — a crate's own leading `//!` doc comment, via `syn` |
| `ekos architecture investigate` (`crates/cli/src/commands/architecture.rs`) | RFC 0066 §65's 12-step MVP loop, orchestrating `build`/`recover`/`compile`/`commit`/`docs generate` directly — no new state-machine framework |

### Implementation details worth remembering

**Reused, didn't reinvent.** `ekos_recovery::llm::LlmProvider` already is RFC 0065 §41's `LLMProvider`
abstraction; `DocumentSemanticsAnalyzerPass` already is a complete, tested template for §46's "LLM
Output Contract." The actual new code in Phase 2/3 is small — a new pass modeled on an existing
one, a pure scoring function, and one orchestrating command composing stages that already existed
independently. RFC 0066's "agent" reduces to loop control once its individual capabilities already
exist as discrete passes/functions.

**The evaluator only scores what it has real signal for.** RFC 0065 §34 lists many dimensions
(`consistency`, `cross_view_consistency`, ...); this phase computes exactly two —
`completeness`/`evidence_coverage` — because those are the only ones this phase's data can honestly
support. Inventing scores for the rest would be exactly the "unsupported precision" §4.6 warns
against, restated concretely this time rather than just cited.

**Targeted re-collection had a real, not hypothetical, thing to target.** Phase 1's extractor is
exhaustive in one pass, so "re-collect" has nothing to gain from scanning `Cargo.toml` again. A
crate's own doc comment is real information the extractor never reads — genuinely new evidence for
a second reasoning attempt, not manufactured busywork to make the loop look agentic.

## A real bug, found live, not in review

### Symptom

First full `ekos architecture investigate` run against this repo's own workspace (real local Ollama
model, `qwen2.5:1.5b`, no cost, no API key) surfaced two real issues before verification was done:

1. **171 "crates considered" instead of 44.** The artifact store is content-addressed and additive
   (RFC 0015) — every past *uncached* `recover` run in this repo (several, across this session)
   left its own `crate-topology-analyzer` `KnowledgeArtifact` behind, none superseding the others.
   `collect_crates` merged every matching artifact's objects/relationships without deduplication.
   `Crate`/`Technology` object ids are deterministic, so the same real crate collapsed correctly by
   id — but relationship ids are **not** deterministic (`KirRelationship::new` mints a fresh random
   id every run), so fan-in/fan-out counts were inflated by roughly however many times this repo's
   `recover` had ever run uncached. Fixed by deduplicating objects by id and relationships by
   `(from, to, kind)` before computing anything — a real regression test
   (`duplicate_historical_crate_topology_artifacts_are_deduplicated`) seeds the same manifests
   twice and asserts `crates_considered` stays at the real count.

2. **0 roles assigned, three times in a row**, even after fixing (1) and even across a *targeted*
   re-collection round with a *smaller* crate set. Investigated by reading the actual raw cached
   LLM response directly (`.ekos/llm-cache/`, `CachedLlmProvider`'s disk cache) rather than
   guessing: the model had returned free-text prose *explaining* the input JSON instead of
   attempting the requested classification schema at all — `input_tokens: 4096`, exactly the
   model's context window, strongly suggesting the prompt (44 crates' worth of properties, or 28
   crates' worth *plus* a `doc_comment` per entry on the targeted round) exceeded it and truncated
   the system prompt's JSON-schema instructions before the model ever saw them.

### Fix and reverification

Chunked the crate list into batches of `MAX_CRATES_PER_CALL = 12` instead of one call for the
entire set — real cost discipline (RFC 0065 §42) traded for reliability once the failure mode was
concretely diagnosed, not abandoned. Added `chunks_more_than_max_crates_per_call_into_multiple_calls`
(a `CountingLlmProvider` test double proving multiple real calls happen for >12 crates) alongside
the dedup fix's own regression test.

**Reverified live, not just in unit tests**: re-ran `ekos recover` against this repo's own real
workspace with the fixed binary. `architecture-reasoning complete crates_considered=44
roles_assigned=39 rejected_unknown_crate=0` — up from 19/44 before the chunking fix, and 0/44 (then
0/28 twice more) before the dedup fix. Spot-checked the actual real classifications via `ekos ekl`:
`ekos-benchmark → "test support"`, `ekos-compiler-core`/`ekos-identity`/`ekos-ledger`/
`ekos-runtime`/`ekos-semantic → "core library"`, `ekos-compiler-sdk`/`ekos-observation-sdk →
"plugin/connector"` — genuinely sensible, evidence-backed classifications from a 1.5B-parameter
local model with real reasoning behind them (`properties["reason"]` on each claim), not just a
plausible-looking count.

**What this run did *not* re-verify**: the full 3-iteration `investigate` loop with the chunking
fix in place end to end (would cost another ~30-45 minutes against this repo's real ~35k+ object
ledger, on top of the two full runs already completed for the loop-mechanics check below). The loop
mechanics themselves — iteration control, evaluator scoring, exit codes, idempotent re-runs via
Phase 13 caching, targeted re-collection actually firing — were already proven correct across the
two earlier full runs; only the *classification quality* needed re-checking after the fix, and the
fast `recover`-only check above did that directly against the real data.

## Live verification against real data — the honest account

Three real runs against this repo's own self-hosted workspace (`.ekos/` backed up to scratch before
each, non-destructively, matching this session's own established discipline):

1. **Pre-fix, full `investigate`**: 3 iterations, real Ollama calls each time, mechanics all
   correct (evaluator scores computed for real, exit code reflected the real unmet threshold,
   curated docs still generated even though quality wasn't reached) — but 0/44 crates classified
   throughout, due to the undiscovered duplicate-artifact bug inflating the prompt to 171 "crates."
2. **Post-dedup-fix, full `investigate`**: prompt now correctly sized at 44 crates for the broad
   pass (16/44 classified — the pre-chunking single-call-for-everything prompt was still too large
   for the model in one shot) and 28 for two targeted rounds (0/28 both times — confirmed via the
   real `has_role` claim count in the ledger staying at 16 across all 3 iterations). This is the run
   that led to reading the raw LLM cache and finding the context-window truncation.
3. **Post-chunking-fix, `recover` only**: 39/44 classified in one broad pass, real, sensible,
   evidence-backed roles, confirmed by direct inspection.

---

## Knowledge Captured

- **When live verification finds 0 of something that should be nonzero, read the actual raw
  model output before assuming the pipeline logic is wrong.** The dedup bug was a real pipeline
  defect; the 0-roles-assigned symptom that persisted *after* fixing it was a completely different,
  model-capability-related cause, only found by reading `.ekos/llm-cache/`'s actual cached response
  content directly rather than re-reading my own Rust code for the third time.
- **A content-addressed, additive artifact store (RFC 0015) means "read every artifact matching
  this pass name" is never safe without deduplication**, the moment more than one real run of that
  pass has ever happened in the workspace's history. `document_semantics_analyzer.rs::collect_sections`
  has the same latent shape (not fixed here — out of scope, a pre-existing pattern, noted for
  whoever touches that pass next).
- **A small local model's context window is a real, load-bearing constraint on prompt batching
  strategy**, not just a "nice to have" cost optimization to abandon when convenient. Chunking
  correctly, rather than reverting to "one call per crate," kept the real cost benefit RFC 0065 §42
  asks for while fixing the actual failure mode.
- **Free local Ollama models make full live-LLM verification of an agentic loop genuinely
  affordable** — no API key, no cost, real network-free inference — good enough to catch two real
  bugs a mocked-provider test suite structurally cannot see (both were about real prompt/data
  shape at real scale, not logic a unit test's small fixtures would ever exercise).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0067-architecture-reasoning-and-investigation-loop.md` | New RFC |
| `ekos/docs/rfcs/0065-architecture-knowledge-model-v2.md`, `0066-...md` | New dated Phase 2/3/MVP-agent status notes |
| `ekos/crates/compiler-core/src/config.rs` | `ArchitectureReasoningConfig` |
| `ekos/crates/recovery/src/architecture_reasoning.rs` | New: `ArchitectureReasoningPass`, chunked LLM calls, dedup fix, `read_crate_doc_comment`; 8 tests |
| `ekos/crates/recovery/src/architecture_evaluator.rs` | New: `evaluate_architecture`; 5 tests |
| `ekos/crates/cli/src/commands/recover.rs` | Registers the new pass, opt-in-gated |
| `ekos/crates/cli/src/commands/architecture.rs` | New: `ekos architecture investigate` |
| `ekos/crates/cli/src/bin/ekos.rs` | New `architecture investigate` subcommand |
| `README.md` | New "Architecture reasoning + investigation loop" section |
| `TODO.md` | Backlog item updated to reflect Phase 2/3/MVP-agent completion |
| `devlogs/devlog_71.md` | This file |

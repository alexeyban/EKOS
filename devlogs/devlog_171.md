# Devlog 171 — RFC 0139: making the eval explain itself, then fixing what it revealed

**Date:** 2026-09-07
**PRs:** local commits `fd7adb1`, `691bd8f`, `57a291e`, `0c63dc9`, `a1369f5`, `3e74863`, `db7b256`
**Branch:** main (local, not pushed at time of writing)

---

## Summary

The RFC 0138 harness reported `48/101` with every gate failing, and the honest conclusion published
in devlog_170 was "`llama3:latest`'s own answer quality is the primary gap". That conclusion turned
out to be **half wrong**, and the only reason we know is that this session first made the harness
capture what the model was actually shown.

The transcripts showed lexical search returning **nothing at all for 80 of 89 scenarios**, and **26
completely different questions being answered from byte-identical evidence**. The answer usually was
not in the context to find. Relaxing the search query lifted answer correctness **37.6% → 48.1%**
and dissolved the flooding. It also pushed fabrications **10 → 15**, a regression that two attempted
mitigations failed to fix — the second one "succeeded" by refusing a third of legitimate questions,
and was reverted. That trade is recorded, not smoothed.

---

## Phase 0 — make failures attributable (`fd7adb1`)

### Problem / motivation

`ScenarioReport` persisted only scores. The answer text and the evidence shown to the model were
computed in `evaluators/mod.rs` and thrown away, so diagnosing one failure meant re-running
`ekos ask` by hand — which is how devlog_170's contamination bug was found, and which does not scale
to 53 failures. Worse, a score cannot distinguish three different defects that all produce `0.0`:
the ruler being too literal, the model answering wrongly, and the fact never reaching the model.

### What was built

| Component | Role |
|---|---|
| `--save-answers` | Persists answer text, the evidence the model was shown, cited/retrieved ids, and pipeline diagnostics |
| `evaluators::attribute` | Deterministic `retrieval` vs `generation` vs `passed` bucket per scenario |
| `Transcript` on `EvalOutcome` | Carries the above without letting grading depend on it |
| `fabrication_rate` / `invalid_citation_rate` / `uncited_answers` | The denominators the headline metrics hide |
| `ruler_version` on `Report` | So a grading change can never be mistaken for a system change |

The evidence text comes from `AiRuntime::gather_evidence` — the same offline `plan` + `execute` pair
`reason` runs internally, so it reproduces what the answer was generated from without an LLM call.

### Decisions

**Attribution had to be free of new LLM calls.** RFC 0138's non-goal (no LLM judge) applies here
too: an attribution step that itself needed a model would inherit the non-determinism the harness
exists to avoid. Plain substring containment over already-captured text, matching
`answer::matched_count`'s own semantics so attribution and grading never disagree about "present".

---

## §3.1 — the all-terms-AND search query (`691bd8f`)

### Problem / motivation

Phase 0's first full run made the cause visible. `search.rs::query_scored` pushed **every** query
term as `Occur::Must`, so *"what crate implements the SQL DDL recovery analyzer?"* required every
content word to co-occur in one document. With search empty, `reason`'s
`Compose[Search, Graph{Neighborhood}]` plan fell back to the 1-hop neighbourhood of whatever entity
resolved — and since **"ekos" appears in nearly every question in an EKOS workspace**, 26 different
questions received the same evidence.

### Implementation details worth remembering

**tantivy 0.22 has no `minimum_number_should_match`.** Verified against the vendored source at
`~/.cargo/registry/src/*/tantivy-0.22.1/`; real min-should-match landed after this pin. The obvious
coverage-ratio fix was unavailable without a major upgrade.

The fallback design turned out to be *stronger* than the one originally wanted: **append-only
backfill**. Run the strict query; if it underfills, run a pure-OR pass and append non-duplicate hits
strictly below every strict hit, rescaled to preserve the strictly-decreasing invariant. Because
strict hits keep their exact ranks, recall@k/MRR/nDCG over the BM25 list are **monotonically
non-decreasing** — a proof, not a hope, which is what allowed shipping under RFC 0126's CI gate.

### Results

RFC 0126 gate, before → after: recall@10 **0.84 → 0.98**, MRR 0.73 → 0.85, nDCG@10 0.74 → 0.87,
intent accuracy unchanged. Nothing regressed; no re-baselining needed.

On the RFC 0138 suite: answer correctness **37.6% → 48.1%**, completeness 36.8% → 46.4%, scenarios
with zero search claims **80/89 → 25/90**, largest identical-evidence cluster **26 → 4**.

---

## §4.2 — robust citation parsing (`57a291e`)

`extract_citations` split on the **last `{`** in the response. A citation block followed by prose, a
pretty-printed block whose last `{` opened a nested object, or a fenced block with trailing text all
failed and fell to `AI001`. Since `AI001` returns empty `evidence_refs`, and empty refs make
groundedness `None`, those scenarios *silently vanished from the metric* rather than scoring 0 — 24
of 36 zero-scorers were in that state.

Now scans every balanced `{…}` span (string-literal aware), tries them last-first, takes the first
that parses and yields a known id, and strips the block from the visible answer. `AI001` fell
**16 → 6**.

`AI001` and `AI002` stay distinct on purpose: "we could not read what it emitted" is a different
defect from "it cited nothing", and collapsing them would hide which one a fix addressed.

---

## §3.6 — the fabrication regression, and two failed fixes (`0c63dc9`, `3e74863`)

§3.1 did exactly what this RFC's own warning box predicted it might: relaxation makes evidence sets
**non-empty for questions about things that do not exist**. Asked *"what port does the EKOS message
broker listen on?"*, retrieval now returns real EKOS crates that merely share a word. Fabrications
rose **10 → 15**; groundedness 78.3% → 72.7%.

**Attempt 1** threaded a relaxation flag (`SignalSource::Bm25Relaxed` → `EvidenceItem::weak`) and
refused deterministically when every claim was weak. Fabrications stayed at 15 and the guard fired
**twice in the whole suite**. Cause, found by inspecting the transcripts: an adversarial question's
evidence holds 20 weak search claims *plus* graph-neighbourhood claims that were not weak, so "all
weak" was never true.

**Attempt 2** also marked the planner-added neighbourhood weak. Adversarial went to **18/18 passing,
0 fabrications** — and `code` answer correctness collapsed **72.7% → 18.2%** with 8 legitimate
questions refused, plus 7 more in `architecture`. Reverted.

### Decision

Shipped `supporting`/`weak` for **rendering only**; the refusal guard covers empty evidence alone.
Driving fabrication to zero by declining to answer a third of real questions is the worse failure.
The regression stays open and recorded.

---

## Knowledge Captured

- **A pass rate cannot tell you which layer is broken; a transcript can.** Every hour spent on
  Phase 0 paid for itself within one run. The intuitive fix (gate the neighbourhood) would have
  treated a symptom — search returning nothing was the cause, and the flooding dissolved on its own
  once the query was fixed.
- **Test the causal chain before fixing the first thing you see.** "Retrieval starves the model" was
  disproved by token counts (median 670 for failures vs 428 for successes; 11 of 22 *correct*
  answers came from <300 tokens) — and then *re-*proved in a sharper form by attribution. Both the
  naive hypothesis and its naive rejection were wrong; only per-scenario evidence settled it.
- **Check the other direction before believing a metric fix.** Attempt 2's adversarial result was
  perfect (18/18, zero fabrications). Accepting it would have shipped a system that refuses a third
  of legitimate questions. A guard that improves one metric by disabling the feature is not a fix.
- **tantivy 0.22 predates `minimum_number_should_match`.** Check the vendored source before
  designing around a tantivy API. The append-only workaround has a stronger safety property
  (provable monotonicity over the BM25 list) than the API originally wanted.
- **Annotate after RRF fusion, never fuse a sub-list.** Fusing relaxed hits as their own list
  restarts their ranks at 1 and gives the best relaxed hit the same contribution as the best strict
  one, destroying exactly the ordering the relaxation's safety argument depends on.
- **A refusal the grader cannot recognise scores identically to a fabrication.** The refusal text
  must contain phrases from `groundedness::DEFAULT_REFUSAL_PHRASES`; otherwise a correctly-behaving
  system reads as a hallucinating one. The model was being graded against a rubric it had never
  been shown.
- **`ekos eval run` exits non-zero when the *quality gate* fails.** A shell loop using
  `&& echo OK || echo FAILED` therefore reports every run as failed while writing perfectly good
  reports. Check for the output file, not the exit code.
- **Per-category sweeps beat one full-suite run on a memory-constrained box.** Full runs were killed
  three times by unrelated desktop memory pressure; a resumable per-category script that skips
  completed categories survived.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0139-answer-quality.md` | New — the RFC, with measured results and both §3.6 failures |
| `ekos/crates/evals/src/{report,history}.rs` | Transcript fields, `ruler_version`, honest denominators |
| `ekos/crates/evals/src/evaluators/mod.rs` | `Transcript`, `Attribution`, `attribute()` |
| `ekos/crates/evals/src/runners/{mod,agent_runner}.rs` | Capture evidence text + diagnostics |
| `ekos/crates/cli/src/{bin/ekos.rs,commands/eval.rs}` | `--save-answers`, model in `agent_label` |
| `ekos/crates/ledger/src/search.rs` | Append-only relaxation + `query_scored_marked` |
| `ekos/crates/ledger/src/{retrieval,fact_ledger}.rs` | `SignalSource::Bm25Relaxed`, post-fusion annotation |
| `ekos/crates/runtime/src/ai.rs` | Balanced-span citation parsing, empty-evidence refusal |
| `ekos/crates/runtime/src/reason.rs` | `EvidenceItem::weak`, `PlanNode::Graph { supporting }` |
| `docs/presentations/eval-comparison-report.html` | Corrected the recall@10 over-reading |
| `TODO.md` | RFC 0139 phase tracking |

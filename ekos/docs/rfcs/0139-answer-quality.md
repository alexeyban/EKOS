# RFC 0139 — Answer quality: attribution, a fair ruler, and the three defect layers

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-06
**Relationship to RFC 0138:** RFC 0138 built the harness that measures answer quality and, on its
first full 101-scenario run, reported `48/101` passing with every gate failed. This RFC is the
response: it does *not* propose a new harness, it fixes the three independent layers the failure
data actually implicates — how answers are graded, how evidence is retrieved, and how answers are
generated — and adds the missing ability to attribute any given failure to one of them.
**Relationship to RFC 0126:** RFC 0126's `retrieval_eval` baseline is a real CI gate
(`ekos/crates/runtime/tests/retrieval_eval.rs`, run in `cargo test --workspace`). Every retrieval
change in §3 must keep it green or re-baseline it with justification recorded here.

---

## Motivation

The first full `ekos-full` run (101 scenarios, `ollama llama3:latest`,
`evals/reports/20260906T182532Z-ekos-full-clean.json`) came back `Status: FAIL` against every gate:

| Metric | Measured | Gate |
|---|---|---|
| Answer correctness | 37.6% | ≥85% |
| Evidence groundedness | 78.3% | ≥90% |
| Completeness | 36.8% | ≥80% |
| Recall@10 | 65.0% | ≥80% |
| Hallucination rate | 9.9% | ≤5% |

The tempting response is to tune the prompt. RFC 0138's own shipping note and the TODO entry that
preceded this RFC both warned against exactly that: keyword-matching strictness, a genuinely wrong
answer, and an evidence set that never contained the fact all produce the *same* low number, and
only reading the real answers tells them apart.

So the failures were investigated before anything was tuned. Two results reshaped the work:

**1. The obvious hypothesis is wrong.** "Retrieval starves the model" does not survive the data.
Median input tokens are **670** for zero-scoring scenarios versus **428** for perfect-scoring ones,
and **11 of the 22 fully-correct answers** were produced from under 300 tokens of evidence.
Evidence volume does not separate success from failure. Any plan that had jumped straight to
"retrieve more" would have been optimising the wrong variable.

**2. The ruler is measurably unfair, in specific and fixable ways.** Scores are effectively binary —
36 scenarios score exactly `0.0`, 22 score exactly `1.0`, only 2 land in between — because almost
every scenario carries a single `expected_facts` keyword matched by case-insensitive substring
containment. A correct answer saying "redacted" fails a `"redaction"` key; "CKM" fails
`"Canonical Knowledge Model"`; "append only" fails `"append-only"`.

**3. Two of the five headline metrics are measured over a minority of the suite.** **69 of 101
scenarios cited nothing at all**, and a scenario that cites nothing returns `None` for groundedness
— so the reported 78.3% is a mean over **46 scenarios**, decomposing exactly as
`(11 correct refusals + 25 valid-citation answers) / 46 = 0.7826`. It says nothing whatsoever about
the other 55. Meanwhile `hallucination_rate` divides by all 101 for a phenomenon only definable on
the 21 `should_refuse` scenarios (19 in `adversarial.yaml`, 2 in `security.yaml`): the honest number
is **10 of 21 fabricating, 47.6%**, not 9.9%. One genuinely good result is buried by the same
arithmetic — **zero invalid citation ids in the entire run**: nothing the model cited was fabricated.

Neither result is visible from the saved report, because the harness **discards the answer text**.
`ScenarioReport` (`crates/evals/src/report.rs:36-60`) persists only scores; `run.answer` is computed
at `evaluators/mod.rs:69` and thrown away. Diagnosing a single failure currently means re-running
`ekos ask` by hand — which is how the RFC 0138 follow-up session found the `evals/`/`test-runs/`
ledger-contamination bug (devlog_170), and which does not scale to 53 failures.

---

## Design

### 1. Phase 0 — failure attribution (prerequisite for everything else)

Persist, behind a `--save-answers` flag on `ekos eval run` so default reports stay small:
the answer text, the rendered evidence block, and the pipeline diagnostic codes
(`AI001`/`AI002`/`RSN001`…).

Then compute a deterministic **attribution bucket** per scenario from data already in hand — for
each `expected_facts` entry, does it appear in (a) the evidence shown to the model, (b) the answer?

| Bucket | Condition | Meaning |
|---|---|---|
| `Retrieval` | fact not in evidence | the fact never reached the model |
| `Generation` | fact in evidence, not in answer | the model had it and didn't say it |
| `Ruler` | fact in neither, but the scenario is semantically answered | phrasing, not substance |

`Ruler` is the one bucket that cannot be fully decided mechanically; it is reported as
"unattributed" and reviewed by hand. The point is not perfect classification — it is that the
histogram, not intuition, decides which of §2/§3/§4 gets invested in first.

**`ekos eval regrade <report.json> [--ruler v1|current]`** is the second half of this phase and the
mechanism the §2 audit trail depends on: it deserializes a saved report, rebuilds `ScenarioRun`s
from the stored transcripts, re-runs `evaluate`, and emits a fresh `Report` — **offline, with zero
LLM calls**. `Report` also gains `ruler_version`, `prompt_version` and `model` alongside the existing
`agent`, and `RULER_VERSION` is bumped by any change to matching semantics, denominators, composite
weights, the alias table, or the datasets.

The immediate deliverable is a full re-run under the *unchanged* ruler, saved as the reference
transcript set (`R0`). Its headline numbers must reproduce 48/101 within cache noise; if they do not,
that discrepancy is investigated before anything else is touched.

**Touches:** `evals/src/evaluators/mod.rs` (`EvalOutcome`), `evals/src/report.rs`
(`ScenarioReport`; new fields take `#[serde(default)]`, the convention this file already documents
on `cache_hit` for reading older reports), `evals/src/runners/agent_runner.rs`,
`cli/src/commands/eval.rs`.

### 2. A fair ruler (deterministic — still no LLM judge)

RFC 0138's Non-goals anticipated this moment: *"A future `judge` evaluator scored by a second LLM
call is a real, separately-scoped follow-on if keyword matching proves too coarse."* Keyword
matching has proven too coarse — but the fix stays deterministic, because reproducibility is the
property that makes this harness worth having. **An LLM judge remains out of scope.**

1. **Alias / `any_of` + normalisation** (`evals/src/schema.rs:53-55`,
   `evaluators/answer.rs:8-36`): a fact may be a list of accepted alternates; matching normalises
   case and treats hyphen/underscore/space as equivalent (`append-only` ≡ `append only`,
   `dbt-gen` ≡ `dbt_gen`) and applies the same `en_stem` stemming the search index already uses
   (`redaction` ≡ `redacted`). Datasets updated where the key is provably unfair (`arch-001` ⊕
   `CKM`, `arch-019` ⊕ `KIR`, `arch-013` ⊕ the spelled-out protocol name, and the
   `never`/`both`/`extend`/`tombstone` discourse keys).
2. **Stop double-counting a single miss** (`evaluators/completeness.rs:12-24` with
   `mod.rs:82-97`): `completeness` reuses `answer::matched_count`, and both feed the same unweighted
   composite, so one missed keyword costs *two* of N slots. A typical reason scenario therefore
   scores `(0 + 0 + 1.0)/3 = 0.33` against the default `pass_threshold` of 0.7 and fails on a single
   substring miss. For the 55 scenarios with facts but no `expected_evidence_contains`, completeness
   is a near-deterministic function of `answer_score` and carries **zero independent information**.
   Replace the unweighted mean with explicit weights — `answer 0.45 | groundedness 0.30 | recall@10
   0.15 | trajectory 0.10`, renormalised over whichever apply — and drop completeness from the
   composite while keeping it as a reported, gated metric.

   Fix the adjacent free pass in the same place (`mod.rs:92-95`): an empty applicable set currently
   yields `composite = 1.0`, i.e. a scenario with nothing gradable *passes*. Make it `None` /
   `passed = false` with a `not_gradable` flag surfaced in the report. `schema.rs:305-311`'s dataset
   test should already make this unreachable — which is exactly why it must be loud if it ever fires.
3. **Stop silently shrinking the groundedness denominator** (`ai.rs:527-531` →
   `groundedness.rs:62-66`): when the model emits no valid `cited_evidence` block, `evidence_refs`
   is empty, groundedness returns `None`, and the scenario **disappears from the metric** rather
   than scoring 0. For a `should_refuse` scenario `None` is correct; for a scenario that produced a
   confident prose answer with no citation, "not applicable" is simply wrong — it is the *definition*
   of ungrounded, and exactly what `AI001`/`AI002` were invented to surface. Change: a non-refusal
   scenario with `answer.is_some() && cited == 0` scores `Some(0.0)`; an errored run stays `None`.

   > **This will move published groundedness from 78.3% to roughly 35.6% on identical answers.** It
   > is the single largest number in this RFC and it is **entirely a ruler correction, not a
   > regression** — the old figure was a mean over 46 of 101 scenarios. Publish the v1/v2 pair on the
   > same `R0` transcripts so it reads as "we were measuring less than half the suite". Recovering
   > this number is what §4 exists to do.

4. **Split `hallucination_rate` into two honest metrics** (`report.rs:165-170`). It currently divides
   fabrications by all 101 scenarios for a phenomenon only definable on the **21 `should_refuse`**
   scenarios (19 in `adversarial.yaml`, 2 in `security.yaml`). The meaningful number is **10 of 21 =
   47.6%**, not 9.9%. Report instead:
   - `fabrication_rate` = fabrications / `should_refuse` count. At N=21 a single failure is 4.8%, so
     a 0.05 gate is a coin-toss, not a quality bar: start the gate at ≤0.20 with a documented ratchet
     (0.20 → 0.10 → 0.05) as the refusal suite grows past ~40 scenarios.
   - `invalid_citation_rate` = scenarios citing ≥1 unresolvable id / scenarios that cited anything.
     Gate ≤0.02. This is currently **0/32 — a genuinely good result the combined metric hides.**
5. **Remove degenerate keys**: `adversarial.yaml:117-121` (`adv-014`) lists
   `refusal_phrases: ["append-only", "does not", "never", "not"]` — bare `"not"` matches virtually
   any English sentence, making that check unfailable. Near-free-pass `expected_facts`
   (`"ledger"`, `"runtime"`, `"ekos"`) get the same review.
6. **Raise metric power**: only 10 of 101 scenarios carry `expected_objects`, so recall@10 is a mean
   over 10 values. The 75%→65% "regression" reported in devlog_170 is **exactly one scenario
   flipping** — noise, not signal. Broaden `expected_objects` (and `expected_evidence_contains`,
   currently 5) rather than chasing the phantom.

**Audit trail (required).** Changing grading makes new numbers non-comparable to the published
48/101. Because §1 saves answers, the *same saved answers* can be re-scored under both the old and
new rulers with **no new LLM calls**; both columns get published.

### 3. Retrieval

Guarded throughout by RFC 0126's CI baseline (`runtime/src/retrieval_eval.rs:311-316` — recall@10
0.84, MRR 0.73, nDCG 0.74, intent 0.83, tol 0.02).

#### 3.0 Neighbourhood flooding — the single highest-leverage defect

Found by §1's transcript capture on its first full run, and not visible from any score:
**26 completely different questions received byte-identical evidence.** Not similar — identical, to
the byte:

```
ekos-semantic — related to ekos
ekos-distributed — related to ekos
ekos-common — related to ekos
walkdir — related to ekos
…
```

The questions sharing that context include *"What are the main stages of the EKOS compiler
pipeline?"*, *"What Rust edition does the workspace target?"*, *"What port does the EKOS built-in
message broker listen on?"* (adversarial), and *"Which crates does the cli crate depend on?"* All 26
were classified `QueryType::Lexical`; all resolved the token **"ekos"** — which appears in nearly
every question in the suite — to the `ekos` object; and the planner's
`Compose[Search(20), Graph{Neighborhood, hops: 1}]` branch (`reason.rs:151-160`) then filled the
evidence set with that object's neighbours. The 60-item cap (`reason.rs:307`) is a prefix truncate,
so those neighbours crowd out whatever the `Search` step actually found.

Blast radius, measured over R0: **35 of 89 scenarios (39%) received evidence that is >80% generic
`related to ekos` neighbourhood**. They score **32.1% answer correctness against 41.9% for the
rest** — and 9 of them are adversarial, 4 of which fabricated. Handing a model a list of real EKOS
crates as the "evidence" for *"what port does the message broker listen on"* is precisely the
plausible-looking fuel that turns a refusal into a fabrication.

This also explains the Motivation's paradox — why evidence *volume* failed to separate right answers
from wrong ones. Volume was never the variable: 26 scenarios received the same ~1,500 characters of
the same largely irrelevant context.

**Fix, in order of directness:**
1. Do not let a resolved entity that matches the *corpus-wide* name (`ekos` in an EKOS workspace)
   drive a Neighborhood expansion — a mention appearing in nearly every question carries no
   discriminating signal. Gate the `Graph{Neighborhood}` branch on the entity being specific
   (e.g. document-frequency-aware, or excluded when the mention is the workspace/root object).
2. Order the evidence budget so `Search` results and the seed entity's own facts survive truncation
   ahead of graph neighbours (§3.5).
3. Treat "every claim is a generic neighbourhood edge" as *no usable evidence* for the refusal
   short-circuit (§4.4).

**No prompt change can repair this** — the answer is not in the context to be found. That is why
this is sequenced ahead of every generation-side fix.

**Correction found while fixing it (2026-09-07): this is a symptom, not the disease.** Before
building the entity gate proposed above, the causal chain was tested — and **80 of 89 scenarios
received *zero* search results**. The `Search` step returns nothing for ~90% of questions, so the
neighbourhood is not out-competing search; it is all that remains when search finds nothing. Where
search did return hits, answer correctness was **57.1% against 35.1%**. §3.1 is therefore the real
fix, and the entity gate is deferred until the measured picture after relaxation says whether it is
still needed. Recorded because the tempting fix here — gating the neighbourhood — would have
treated the symptom and left the cause in place.

**Measured after §3.1 landed** (5 of 7 categories, 71 scenarios, `ollama llama3:latest`, §3.1 only):
the largest byte-identical evidence cluster fell from **14 to 4**, and scenarios receiving zero
search claims fell from **54/62 to 24/62**. Flooding was substantially dissolved by fixing the
query, exactly as the corrected diagnosis predicted — no entity gate required so far.

1. **Stop requiring every query term — via append-only backfill.** `ledger/src/search.rs:316-334`
   pushes each term as `Occur::Must` — a pure conjunctive AND, so *"What crate implements the SQL DDL
   recovery analyzer?"* requires every content word to co-occur in one document.

   **A constraint that eliminates the obvious fix:** this workspace pins `tantivy = "0.22"`
   (`ekos/Cargo.toml:127`), and **tantivy 0.22.1 has no `minimum_number_should_match` /
   `with_minimum_required_clauses`** — verified against the vendored source; real min-should-match
   landed after 0.22. A coverage-ratio query is therefore unavailable without a tantivy major
   upgrade, which is its own index-format risk surface and is out of scope here.

   **Chosen design — progressive relaxation with append-only backfill**, in `query_scored`:
   1. run today's strict query unchanged → **strict hits**;
   2. if `strict.len() < limit` **and** `terms.len() > 1`, run the same per-term clauses as a pure
      OR, drop ids already present, and **append** the remainder in BM25 order;
   3. rescale appended scores into the band strictly below the weakest strict score, preserving
      `RankedResults`' documented strictly-decreasing-score invariant (`retrieval.rs:107-109`).

   **Why this and not the alternatives:** it is *provably* non-regressive for the BM25 candidate
   list — every strict hit keeps its exact rank and new hits appear only below all of them, so
   recall@k, MRR and nDCG@k over that list are monotonically non-decreasing. That is a far stronger
   guarantee than "we tested it and it looked fine", which matters because RFC 0126's gate is real.
   Splitting terms by IDF (Must for high-signal, Should for the rest) was rejected: it re-ranks
   queries that already work today, which is precisely the change most likely to trip that gate.
   Populating `RetrievalRequest.keywords` was rejected as a no-op — the backend has no boolean
   parser, so `"a OR b"` becomes required `a` + required `or` + required `b`.

   **Residual risk, stated precisely:** the monotonicity guarantee covers the BM25 list, not the
   RRF-fused list (`fact_ledger.rs:917-995`). A backfilled doc entering at rank 40 contributes
   `1/(60+40)` and could displace a vector-arm-only doc. Verification protocol: capture
   `cargo test -p ekos-runtime retrieval_eval::tests::print_current -- --ignored --nocapture` before
   and after, diff per query, then run the full gate. If a metric drops, narrow the trigger (e.g.
   backfill only when `strict.is_empty()`) rather than widening the tolerance. Re-baseline only
   upward-or-equal metrics, documented in the style `retrieval_eval.rs:294-310` already sets.

   **Measured result (implemented 2026-09-07).** The residual risk did not materialise — every RFC
   0126 metric improved and none regressed, so no re-baselining was required:

   | RFC 0126 metric | Baseline | After relaxation |
   |---|---|---|
   | recall@10 | 0.84 | **0.98** |
   | MRR | 0.73 | **0.85** |
   | nDCG@10 | 0.74 | **0.87** |
   | intent accuracy | 0.83 | 0.83 (unchanged) |

   Full workspace green (114 suites), plus four new tests pinning the behaviour that matters: a
   multi-term natural-language question now retrieves rather than returning nothing; a document
   matching every term still outranks every relaxed hit; a single-term query is untouched; and a
   strict hit is never duplicated as a relaxed one.

   **End-to-end effect on the RFC 0138 suite** (5 of 7 categories, 71 scenarios, §3.1 alone, same
   agent and same ruler as R0 — so this is a system change measured against an unchanged ruler):

   | | R0 | after §3.1 |
   |---|---|---|
   | passed | 35/71 | **39/71** |
   | answer correctness | 40.4% | **48.7%** |
   | completeness | 40.4% | **47.7%** |
   | scenarios with zero search claims | 54/62 | **24/62** |
   | largest identical-evidence cluster | 14 | **4** |

   Attribution moved the way a genuine retrieval fix should: `retrieval` 29 → 23, `passed` 20 → 24,
   and `generation` 2 → 4. More generation failures is *progress* — those are scenarios where the
   fact now reaches the model and the answer still omits it, i.e. the bottleneck has moved into
   §4's territory. 24 scenarios still see no search hits, so §3.1 is an improvement, not a
   completed job.

   **Documented divergence, not a bug:** the SQLite/FTS5 backend (`ledger/src/lib.rs:1023-1046`) is a
   separate query path and does not get relaxation. New workspaces use the fact engine (RFC 0016), so
   the eval measures the path that matters.

2. **Fix the identifier tokenisation mismatch.** The mismatch is **CamelCase specifically**, not
   underscores: `search.rs:283-291` splits queries on `_`/`::`/`-`/`.`, and `SimpleTokenizer` splits
   *content* the same way, so a content occurrence of `sql_analyzer` does split — but
   `SqlAnalyzerPass` is alphanumeric throughout and indexes as the single stem `sqlanalyzerpass`.
   Neither `sql` nor `analyzer` can ever match it. That is exactly why `code-006` scores recall 0.

   Fix at **both** ends through one shared function so they cannot diverge: a custom index-time
   tokenizer (`ekos_ident_v1`) splitting on non-alphanumerics, case transitions and letter/digit
   boundaries — emitting the un-split whole token at the same position so `sqlanalyzerpass` stays
   directly matchable — plus query-time expansion of each term through the same function.

   **Migration is free:** changing the registered tokenizer *name* changes the on-disk schema, so
   `Index::open_or_create` returns `SchemaError`, which `search.rs:159-181` already handles by
   rebuilding from the derived source on a writable open (and returns a clear "open writable once to
   self-heal" error read-only). **Trap to avoid:** doing this as write-time text expansion in
   `upsert` instead is easier but produces *no* schema change, so stale indexes would silently serve
   old tokens forever.

   **This is the riskiest change in the RFC** — emitting sub-tokens alters document lengths and term
   frequencies, so BM25 scores shift for *every* query, with no monotonicity argument available.
   Sequence it last, verify with the same protocol, and budget for a justified re-baseline. Fallback
   if the gate cannot be recovered: query-time-only expansion (try the concatenation as a
   `PhrasePrefixQuery`), weaker but with zero index change.
3. **Fix the broken OR ladder.** `ai.rs:439-453` builds `terms.join(" OR ")` on the assumption of
   FTS5 semantics; against tantivy this adds a *required* term `or`, making the "relaxed" fallback
   **stricter** than the query it was meant to loosen. The doc comment above it is stale.
4. **Grade recall on the query the system actually issues.** `runners/agent_runner.rs:67` and
   `retrieval_runner.rs:14` measure recall@10 with `RetrievalRequest::lexical(raw question)`,
   bypassing `extract_search_terms` — so every stopword becomes a required AND term and recall is
   graded against a stricter query than the answer path ever used.
5. **Rank before truncating.** `reason.rs:249-258,307` caps the evidence set at a hardcoded 60 via a
   plain prefix `Vec::truncate` — no ranking, no diversity — so graph neighbours consume the slots
   and the seed entity's own facts are dropped last.
6. **Vector arm (config-gated, last).** `query_embedding` is set only in `retrieval_eval.rs`,
   `cli/commands/query.rs` and `cli/commands/mcp.rs` — **never** by `AiRuntime::ask`/`reason`, and
   `[embeddings]` is absent from this repo's `ekos.toml`. So `Conceptual` queries run BM25-only,
   which RFC 0126's own test concedes "largely collapses without vectors". Real upside, bigger
   change, sequenced after 1-5.

### 4. Generation

1. **Make `REASON_SYSTEM_PROMPT` config-overridable** (`ai.rs:32-34`). Today it is hardcoded, while
   `[ai] system-prompt` overrides only `DEFAULT_SYSTEM_PROMPT` — used by `ask`, which the suite
   never exercises (91 `reason`, 10 `retrieval`, **0 `ask`** scenarios). The tunable prompt is the
   one nobody tests; the tested prompt is untunable.
2. **Parse citations tolerantly** (`ai.rs:499-532`). It splits on the **last `{`** in the response,
   so prose containing `{`, pretty-printed JSON, or a fenced block with trailing text all fall into
   `AI001` — and the raw JSON then leaks into the visible answer, observed live on `arch-001`. Given
   24 of 36 zero-scorers hit `AI001`, this is the single highest-yield generation fix. Strip the
   block from the answer text once parsed.
3. **Explicit refusal contract** for the 10-of-21 fabrications. Today the prompt says "say so
   explicitly" while the grader looks for one of 22 specific phrases (`groundedness.rs:16-39`) — the
   model is graded on a rubric it was never shown. The prompt must name the canonical refusal
   wording literally, and forbid the observed failure mode ("do not guess, do not describe what such
   a thing would probably do").
4. **Short-circuit the empty evidence set.** `ai.rs:233-260` calls the LLM unconditionally. When the
   `EvidenceSet` is empty, return the canonical refusal deterministically, with zero tokens and no
   LLM call — converting a probabilistic fabrication into a guaranteed refusal.

   > **Measured outcome (2026-09-07): the coupling below was real, and the mitigation failed
   > twice.** §3.1 raised answer correctness 37.6% → 48.1% and simultaneously pushed fabrications
   > **10 → 15** of 101, groundedness 78.3% → 72.7%.
   >
   > *Attempt 1* marked only relaxed **search** hits weak and refused when every claim was weak.
   > Fabrications stayed at 15; the guard fired twice in the whole suite. Cause: an adversarial
   > question's evidence set also contains graph-neighbourhood claims, which were not weak, so
   > "all weak" was never true.
   >
   > *Attempt 2* additionally marked the planner-added neighbourhood weak. That **did** drive
   > adversarial to 18/18 passing with 0 fabrications — and collapsed `code` answer correctness
   > from **72.7% to 18.2%**, refusing 8 legitimate questions, with 7 more refused in
   > `architecture`. Eliminating fabrication by declining to answer a third of real questions is
   > the worse failure, so it was reverted.
   >
   > *Attempt 3 (§3.7, term-coverage scoring)* replaced the binary flag with a graded one —
   > `matched_terms`/`total_terms` per hit, weak below a 0.5 coverage floor. Same outcome as
   > attempt 2: adversarial 18/18 with 0 fabrications, `code` correctness **72.7% → 18.2%** again,
   > `architecture` 8 → 4 passed. Reverted. Coverage *alone* (neighbourhood not weak) measures at
   > parity with the baseline — code 9→7 passed, adversarial unchanged at 13 fabrications.
   >
   > **The threshold was never the problem.** Long natural-language questions legitimately produce
   > sub-threshold coverage, so `is_all_weak` cannot separate "nothing answers this" from "the match
   > was loose but right" *while a spurious neighbourhood is attached at all*. Three attempts have
   > now failed on the same mechanism.
   >
   > **The untried lever is §3.0's entity gate.** Stop a corpus-wide hub name — "ekos", present in
   > nearly every question in an EKOS workspace — from driving neighbourhood expansion. Then an
   > adversarial question retrieves only low-coverage search hits and is genuinely all-weak, while a
   > question naming a specific entity keeps its neighbourhood. This is the §3.0 fix originally
   > deferred as "treating a symptom": correct for answer *correctness*, wrong for fabrication,
   > where the spurious neighbourhood is the fuel.

   > **Why the `is_all_weak` mechanism cannot work as specified:** relaxation means *legitimate*
   > questions also retrieve
   > partial-overlap hits, so "every claim is weak" does not separate "nothing answers this" from
   > "the match was loose but correct". The needed signal is **how much of the query a hit
   > matched** — term-coverage scoring — which the binary relaxed/strict flag cannot express.
   > `supporting` and `weak` ship for rendering only; the empty-evidence refusal stands. The
   > fabrication regression is **open**, recorded rather than papered over.

   > **⚠ This is coupled to §3.1, and the coupling runs the wrong way.** Relaxed retrieval makes the
   > evidence set *non-empty* for adversarial questions (`FooBarNonexistentAnalyzer` → `analyzer`
   > matches dozens of real objects), which both disarms this short-circuit and hands the model
   > plausible-looking fuel. The mitigation is therefore **required, not optional**: mark relaxed
   > hits (a `Bm25Relaxed` signal source, propagated through `rrf_fuse`), render them in
   > `render_evidence` with an explicit weak-match marker, and treat "every hit is relaxed" as empty
   > for the short-circuit decision. §3.1 and this item must be measured together on the adversarial
   > category — otherwise recall goes up and so does fabrication.

5. **Name identifiers verbatim** — instruct the model to quote exact crate/module/symbol names from
   the evidence instead of paraphrasing, targeting the identifier-lookup scenarios directly.
6. **Optional single repair retry** on `AI001` before accepting an uncited answer — bounded to one
   short completion, config-gated, token cost kept visible. Drop it if §4.2 alone drives `AI001` to
   near zero. A general answer-quality retry loop is explicitly *not* proposed: non-deterministic,
   expensive at 36 s/scenario, and it optimises the metric rather than the system.

Prompt edits invalidate the LLM disk cache automatically (the key includes the system prompt —
`recovery/src/cache.rs:3,24`), so no manual cache clearing is needed.

### 5. Provider choice and documentation

The answering provider stays **user-selectable via `[llm]`** (already true), per-run overridable via
`ekos eval run --agent`. No new mechanism is needed. What is missing is one safety fix and guidance:

**The one code change — stop the mock provider from producing a publishable report.**
`recover.rs:1096-1148`'s `build_llm_provider` silently degrades to a `MockLlmProvider` returning a
fixed JSON stub when the API-key env var is unset, logging only a `tracing::warn!`. For `ekos
recover` that is reasonable degradation. For `ekos eval run --agent claude` it produces a
fully-formed, timestamped, saved, *publishable* report in which **every answer is a stub** — a live
footgun aimed directly at the cloud reference baseline this section exists to establish. Add a
`Result`-returning strict variant with `eval.rs` as its only caller, and hard-fail naming the missing
env var. Also fix `agent_label` (`eval.rs:27-36`), which returns a bare `"claude"`/`"openai"` with no
model, so the report records what actually answered.

- Document the `[ai]` section, which is **absent** from this repo's `ekos.toml`, so every run to
  date has used compiled defaults (`max_matches` 3, `neighborhood_depth` 1, `max_tokens` 1024).
- State plainly that **running a local model well needs a powerful server**, and that a **cloud
  model is the recommended choice for reference baselines** and quality-sensitive use.
- Establish the reference baseline with a cloud model (`ekos eval run --agent claude`, needs
  `ANTHROPIC_API_KEY`) once §1-§4 land — this is the A/B that finally separates "local-model
  ceiling" from "pipeline defect".

**Touches:** `README.md`, `evals/README.md`, `ekos.toml` comments,
`docs/generated/ekos-self-documentation.html`.

---

## Non-goals

- **No LLM-judge evaluator.** Deferred by RFC 0138 and reaffirmed here; determinism is the property
  that makes this harness trustworthy.
- **No chasing the recall@10 "10pp regression".** It is one scenario of ten (§2.6); the fix is
  metric power, not a hunt.
- **No prompt tuning ahead of §1.** The token-count analysis in Motivation is the standing example
  of why intuition misleads here.
- **No casual re-baselining of RFC 0126.** It is a real CI gate; any movement is justified in this
  document, not edited for convenience.
- **No tantivy upgrade beyond 0.22.** The min-should-match API that would simplify §3.1 arrived
  after this pin; upgrading is an index-format risk surface of its own, and §3.1's backfill design
  exists precisely so the upgrade is not needed.
- **No enabling of the vector arm / `[embeddings]` on the reason path.** Tempting, since RFC 0126's
  own test concedes "Conceptual largely collapses without vectors" (0.45 lexical-only vs 0.938) —
  but that 0.938 came from a **mock** embedder over a small synthetic estate, which is evidence the
  fusion plumbing works, *not* evidence a real embedder helps on this repo's real ledger. It needs an
  embedding pass over the whole ledger, a per-query embed on the latency-critical path, and flipping
  a default that is currently `false`. It deserves its own RFC and its own measurement.
- **No `mode: ask` scenarios.** The suite tests `reason` (91) and `retrieval` (10) and never `ask`;
  the right fix is making the *reason* prompt configurable (§4.1), not diluting the suite with a
  legacy pre-0123 path the product no longer leads with.
- **No CI wiring for `ekos eval`.** RFC 0138's non-goal stands: real LLM calls, 36 s P95,
  non-deterministic.
- **No change to the five headline gate values.** Only the *definitions* of `hallucination_rate`
  (split in two) and the composite weighting change; the bars stay where RFC 0138 put them.
- **No fix to the SQLite/FTS5 query path** to match the new tantivy relaxation semantics (§3.1).

---

## Measured results (2026-09-07)

All 101 scenarios, `ollama llama3:latest`, **unchanged ruler** throughout — so every delta below is
a system change, not a grading artifact. R0 reproduces the published baseline exactly.

| | R0 baseline | R1 (§3.1) | R2 (+§4.2, §3.6 attempt 1) |
|---|---|---|---|
| passed | 48/101 | 49/101 | 48/101 |
| answer correctness | 37.6% | **48.1%** | 46.0% |
| completeness | 36.8% | **46.4%** | 46.0% |
| groundedness | 78.3% | 72.7% | 70.6% |
| fabrications | 10 | **15** | **15** |
| `AI001` (unreadable citation block) | 16 | 7 | **6** |
| scenarios with zero search claims | 80/89 | **25/90** | 25/90 |
| largest identical-evidence cluster | 26 | **4** | 4 |

### The ruler, measured against itself

`ekos eval regrade` re-scores **R0's saved answers** under each ruler version — identical answers,
no LLM calls, so every delta here is grading and nothing else:

| ruler | passed | answer correctness | groundedness | completeness |
|---|---|---|---|---|
| v1 (RFC 0138 as shipped) | 48/101 | 37.6% | 78.3% | 36.8% |
| v2 (§2.1 normalisation + `any_of`) | 48/101 | 37.1% | 78.3% | 35.4% |
| v3 (§2.2 composite + §2.3 groundedness) | **31/101** | 37.1% | **39.6%** | 35.4% |

**The published 48/101 was inflated.** The honest baseline for the same answers is **31/101**. The
system did not get worse between these rows — only the ruler's willingness to score what it had
been quietly excluding.

Two findings worth keeping:

- **v2 went *down*, and that inverted this RFC's premise.** §2.1 was written to fix false negatives
  — and it did fix two (`sec-001` "redacted" vs `"redaction"`, `lin-005` "tombstones" vs
  `"tombstone"`). But it also removed **three false positives** the old substring matcher had been
  awarding: `arch-006` matched `"runtime"` inside "Ai**Runtime**" on an answer that says
  "AiRuntime.kind = RustSymbol"; `code-008` matched `"kir"` inside "**Kir**Object" on an answer
  naming the wrong crate; `arch-002` matched `"compile"` inside "ekos-**compile**r-core" on an
  answer that is nonsense. The old ruler was not merely too strict — it was **also too loose, in the
  direction that flatters the system**, and part of the published 37.6% was credit for wrong
  answers. All three are now regression tests.
- **v3's groundedness drop is the predicted correction, not a regression.** §2.3 forecast ~35.6%;
  it measured 39.6%. 59 of 101 scenarios answered while citing nothing, and those were being
  excluded from the metric rather than scored.

**What this says, plainly.** §3.1 is a clear win on the primary metric (+10.5pp answer correctness)
and it removed the structural pathology — evidence sets stopped being interchangeable. §4.2 cut
unreadable citation blocks 16 → 6. Neither §3.6 attempt reduced fabrication without an unacceptable
cost, so the fabrication regression stands open at 15/101 against a 10/101 baseline.

The net position is a real trade, not a clean victory: **the system answers meaningfully better and
refuses meaningfully worse.** Both halves are on the record because a report that showed only the
first half would be the kind of number this whole RFC exists to distrust.

## Verification

- `cd ekos && cargo test --workspace` (includes the RFC 0126 retrieval gate),
  `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`.
- `ekos eval run --dataset ekos-full --agent ollama --save-answers` after each phase, with old-ruler
  and new-ruler columns published side by side (§2 audit trail).
- Per-phase targets: §1 buys visibility only (no metric movement expected); §2 a corrected baseline
  whose remaining failures are real; §3 recall@10 ≥80% on the broadened set with RFC 0126 green;
  §4 groundedness ≥90% and adversarial fabrications <2/18; §5 a published cloud-model reference
  baseline.
- `docs/presentations/eval-comparison-report.html` updated with the corrected recall@10
  interpretation.

# Devlog 179 — RFC 0138/0139 Phase 2-3: mostly already done, two real gaps closed

**Date:** 2026-09-14
**PRs:** (local, not yet pushed) `fix(ledger,runtime,evals): RFC 0139 Phase 2 — bareword and/or connector noise, grade recall on the pipeline's real query`
**Branch:** main (local)

---

## Summary

Asked to pick up "RFC 0138/0139 Phase 2–3" from `TODO.md`'s own backlog description, the first
finding was that almost all of it had already shipped — under RFC 0139's own `§3.1`/`§3.2`/`§3.6`/
`§3.7`/`§4.1`/`§4.2`/`§4.3` numbering, in earlier sessions (devlog_170-172) — and the "Phase 2"/
"Phase 3" checklist bullet had simply never been marked done or reconciled against the work that
superseded it. `git log` on `search.rs` and `ai.rs` confirmed progressive relaxation, CamelCase
subword expansion, the balanced-brace citation parser, the configurable REASON prompt, and the
worded refusal contract are all real, committed, and tested.

Two items named in that same bullet were genuinely still open, both real and both fixed this
session: a bareword `"OR"` in a fallback search query was being treated as a literal, required
content term on the tantivy backend (it's boolean syntax only on the older SQLite/FTS5 backend the
code was originally written against); and the eval harness's `recall@10` metric was graded against
the raw question sentence, while the actual REASON pipeline searches with a keyword-only string —
two different queries that can rank differently, so the recorded "what did retrieval find" list
didn't always match what the model was actually shown.

`cargo test --workspace` / `clippy -D warnings` / `fmt --check` all clean.

---

## PR — RFC 0139 Phase 2: connector-word noise + honest recall grading

### Problem / motivation

`TODO.md`'s Phase 2 bullet named four things: (1) every query term forced `Occur::Must`, (2) a
CamelCase tokenization mismatch, (3) "the broken `terms.join(\" OR \")` ladder (it adds a
*required* `or` term)", and (4) "grading recall on the query the pipeline actually issues rather
than the raw question." (1) and (2) turned out to be already fixed (§3.1/§3.2). (3) and (4) were
real, live bugs, both confirmed against the actual code before touching anything.

### What was built

| Item | Where | What |
|---|---|---|
| Connector-word noise | `crates/ledger/src/search.rs` | Bareword `and`/`or` dropped from the term list entirely, not turned into a query term |
| Honest recall query | `crates/runtime/src/reason.rs`, `lib.rs`, `crates/evals/src/runners/agent_runner.rs` | `agent_runner` now searches with the same `search_query(understand(question))` string `plan()` uses internally |

### Implementation details worth remembering

**The `terms.join(" OR ")` ladder was correct — for a backend that no longer exists in this
question's likely path.** `ai.rs::search_for_question`'s AND→OR→raw fallback (RFC 0061) was
written against the SQLite/FTS5 `Ledger::find_objects`, where an unescaped `"a OR b"` bareword
MATCH string really is boolean OR syntax native to FTS5. But `RetrievalRequest` (RFC 0119) is a
polymorphic seam over *either* backend, and the same string reaches `search.rs`'s tantivy-backed
`query_scored_marked` on a v3 (fact-segment engine) workspace — the default for new workspaces
since 2026-08-21. That tokenizer just splits on non-alphanumeric characters
(`query.split(|c| !(c.is_alphanumeric() || c == '*'))`) with no concept of a boolean keyword, so
`"widget OR gadget"` became three literal terms: `widget`, `or`, `gadget`. All three were
`Occur::Must` on the strict pass. A document containing both real words but never the literal word
"or" therefore failed the strict pass on a phantom third requirement and surfaced only through
relaxation — downgraded to a `"possible search match (partial term overlap)"` claim in
`reason.rs`, i.e. treated by the model as weaker evidence than it should have been.

Fixed at the tokenizer, not the call site: `and`/`or` are now dropped as connector noise before
becoming query terms at all — the same two words this codebase already treats as English
stopwords everywhere else (`ai.rs::QUESTION_STOPWORDS`). This brings both backends into agreement
on what the same query string means, rather than teaching `ai.rs` to special-case a backend it
can't actually see. Verified as a real regression, not a hypothetical: temporarily reverted the
one-line filter, confirmed the new test fails (`total_terms` reported 3, not 2 — the coverage
fraction the RFC 0139 §3.7 machinery exposes downstream), then restored the fix and confirmed it
passes.

**The recall-grading gap was a genuine query mismatch, not a rounding error.** `agent_runner.rs`
(runs `mode: reason`/`ask` scenarios) called `ai.reason(&scenario.question)` — which internally
calls `reason::plan_question` → `plan(&understand(question, runtime))`, and `plan()`'s own
`search_query()` helper returns `u.keywords.join(" ")` (falling back to the raw question only if
keyword extraction found nothing) — then, entirely separately, captured `retrieved_ids` for
`recall_at_10` by calling `runtime.retrieve(&RetrievalRequest::lexical(&scenario.question))` with
the **raw, unprocessed question**. For a scenario like `code-002` (*"What function builds the
LlmProvider used by ekos ask, choosing between Anthropic, OpenAI, and Ollama?"*), those are two
meaningfully different BM25 queries — one is a full sentence with `"the"`/`"used"`/`"choosing"`/
`","`/`"?"`, the other is exactly the keyword set `understand()` extracted. Confirmed this isn't
theoretical: `retrieval_runner.rs` (pure `mode: retrieval` scenarios, no `AiRuntime` at all) was
checked too and found *not* to have the same bug — every `mode: retrieval` scenario in
`evals/datasets/*.yaml` is already written as a bare keyword phrase (`"sql_analyzer pass"`,
`"redaction"`, `"artifact store"`), matching MCP's own `ekos_search` guidance, so there's no
sentence-vs-keywords gap to close there.

Fixed by exposing `reason::search_query` (previously a private `fn`) as `pub`, re-exported from
`ekos_runtime`'s crate root, and having `agent_runner.rs` call `understand(&scenario.question,
runtime)` + `search_query(&u)` itself before the retrieval capture — the exact same two-step
`plan()` already performs internally, so the recorded ranked list is now provably the one the
model actually saw whenever the plan routes through a `Search` node. Falls back to the raw
question only if `understand` itself errors, keeping RFC 0139 §2.6's "always capture *something*"
reasoning intact rather than leaving `retrieved_ids` empty.

### Decisions (alternatives considered, why this choice)

- **Fix the tokenizer, not `ai.rs`'s query-building.** Could have special-cased backend awareness
  in `ai.rs` (skip the OR rung on a tantivy-backed workspace), but `RetrievalRequest`'s whole
  design point (RFC 0119) is that a caller doesn't need to know which backend it's talking to.
  Teaching the tantivy tokenizer to treat `and`/`or` the same way FTS5 already does — as
  boolean-adjacent noise, not indexed vocabulary — keeps that abstraction honest instead of leaking
  a backend distinction into a caller that was written specifically not to have one.
- **Reuse `plan()`'s own `search_query()` rather than re-deriving keyword extraction in
  `evals`.** A hand-rolled second implementation of "strip stopwords/punctuation from a question"
  in the eval crate would inevitably drift from the real one in `reason.rs` — exactly the kind of
  duplicate-source-of-truth bug this whole investigation was chasing in the first place. Exposing
  the existing private function was a one-line visibility change plus a re-export; no logic
  duplicated.
- **Left `retrieval_runner.rs` untouched** after confirming its scenarios don't have this problem,
  rather than applying the same `understand`/`search_query` call there defensively. Its own module
  doc comment states it exists specifically to test the bare `Runtime::retrieve` primitive with no
  `AiRuntime` involved — running scenario questions through `understand()` there would test a
  different thing than what the module says it tests.

---

## Knowledge Captured

- **A backlog checklist item can go stale by being superseded, not just abandoned.** `TODO.md`'s
  "Phase 2/3" bullet accurately described real, then-open bugs when it was written, but the actual
  fixes landed under a different numbering scheme (`§3.x`/`§4.x`) in a later session, and nobody
  went back to check the original bullet off. `git log -- <file>` against the exact file/line the
  bullet named was the fastest way to find out how much was already done before writing any new
  code — three of five sub-items turned out to need nothing.
- **A query string that means something special on one backend can mean something entirely
  different, silently, on another.** SQLite FTS5's bareword `AND`/`OR`/`NOT` keyword syntax and a
  from-scratch tantivy tokenizer built with no awareness of that convention will disagree on the
  same literal text with no error, no warning — just a query that's quietly stricter than intended
  on one of the two backends a single abstraction seam is meant to unify.
- **When two code paths are supposed to search with "the same" query, verify it, don't assume
  it.** `agent_runner.rs`'s recall capture and `reason::plan()`'s real search were both single-line
  calls that looked parallel at a glance; only tracing `search_query()`'s actual return value
  (`u.keywords.join(" ")` vs. the untouched `scenario.question`) showed they diverged.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/search.rs` | Bareword `and`/`or` dropped as connector noise before becoming query terms; regression test (verified to fail without the fix) |
| `ekos/crates/runtime/src/reason.rs` | `search_query` made `pub`, documented as the query a caller can ask "what would the pipeline actually search with" |
| `ekos/crates/runtime/src/lib.rs` | Re-exports `reason::search_query` |
| `ekos/crates/evals/src/runners/agent_runner.rs` | Recall capture now searches with `search_query(understand(question))`, matching `plan()`'s own query, not the raw question |
| `ekos/crates/runtime/src/ai.rs` | Doc comment on `search_for_question` records the backend-specific-syntax hazard the `search.rs` fix closes |
| `TODO.md` | Phase 2/3 bullets reconciled: marked done, with what was already shipped vs. fixed this session spelled out |
| `README.md`, `docs/generated/ekos-self-documentation.html` | Note the recall-grading fix and the `and`/`or` connector-noise fix in the eval-harness / retrieval-quality sections |

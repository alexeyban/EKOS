# RFC 0061 — AI Runtime: Extract Search Keywords From Natural-Language Questions

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-20

---

## Motivation

Devlog 60's `ekos ask` Q&A set against the compiled `analytics/` ledger found `ask` consistently
retrieving zero context for ordinary full-sentence questions, even when the underlying object was
trivially findable by name: *"Who are the top contributors to this repository by commit count?"*,
*"Who is Niklas Hambüchen and what did they contribute?"*, *"What is Plausible Analytics and how
does it track visitors without cookies?"* all returned an honest but wrong "I don't have enough
information" — while the bare name `"Niklas Hambüchen"` correctly retrieved the real `Person`
object from the same ledger.

Root cause, read directly from `crates/ledger/src/lib.rs`'s `find_objects` (backed by SQLite
FTS5): any character outside `[alphanumeric, space, *]` — including ordinary sentence punctuation
like `?`, `,`, `'` — triggers `is_simple_term = false`, which escapes the **entire query string**
into one literal FTS5 phrase (`format!("\"{}\"", query...)`). A phrase query requires that exact
text to appear contiguously in indexed content, which a natural-language sentence never does
against object names/content — so every question containing punctuation silently retrieved
nothing, while a bare alphanumeric name (never hitting the escape path) worked correctly. `crates/runtime/src/ai.rs`'s
`gather_context` was passing the raw question straight into `Runtime::find_objects` with no
translation step, even though the surface it's built on (`ekos_search`, the same underlying
`find_objects`, exposed over MCP) already tells callers in its own tool description to *"Use 2-3
keywords, not natural-language questions"* — `ask` is specifically the one caller meant to accept
natural language, so it's the one place responsible for doing that translation, and it wasn't.

A second, related finding surfaced from the same Q&A set: `ask "README.md"` (a bare, punctuation-free
query, so not hit by the phrase-escaping bug) answered from `test/priv/README.md` — an 83-byte
GeoLite2-test-fixture readme — instead of the real project `README.md`, which ranks 13th of 25
FTS5 matches for that literal filename, past `AiRuntimeConfig::max_matches`'s default of 3. This
is a distinct problem (relevance ranking / result-count tuning for ambiguous common filenames, not
a phrase-escaping defect) and is explicitly **not** addressed by this RFC — see Non-goals.

## Scope

Add `AiRuntime::search_for_question` (`crates/runtime/src/ai.rs`), called from `gather_context` in
place of the direct `self.runtime.find_objects(question)` call:

1. `extract_search_terms(question)` — lowercases, splits on every non-alphanumeric character
   (including `_`, matching FTS5's own default `unicode61` tokenizer, which already treats `_` as
   a separator) and on a conservative closed-class English stopword list, drops terms shorter than
   2 characters, and dedupes preserving first-occurrence order.
2. If any terms remain, try an FTS5 **AND** query (all keywords joined by spaces — FTS5's default
   for bareword-separated terms) first, for precision.
3. If that returns nothing and more than one term remains, fall back to an FTS5 **OR** query
   (terms joined by literal ` OR `) for recall.
4. If both return nothing (or no terms were extracted at all), fall back to the original raw
   `question` string — the exact pre-existing behavior — so no previously-working query (a caller
   already passing a bare name/keywords, which never hit the bug) can regress.

## Non-goals

- **Not fixing `find_objects`/`is_simple_term` itself.** Other callers (`ekos query find`, the
  `ekos_search` MCP tool) rely on its literal-phrase-escaping behavior for queries the caller
  deliberately typed with punctuation; changing that shared function's semantics would be a much
  larger, less targeted change than translating `ask`'s specific natural-language input at its one
  call site.
- **Not fixing the `README.md` relevance-ranking finding.** A separate root cause (ambiguous
  common filenames + a fixed `max_matches` cutoff with no relevance weighting by path
  depth/importance), not addressed here — worth its own RFC if it matters enough to a user.
  **Fixed** — `devlog_65` (2026-08-20/21) traced the real root cause to bm25's cross-document
  content-length normalization and added `promote_exact_name_matches` in `Ledger::find_objects`.
- **Not answering genuinely aggregate/analytical questions** ("top contributors by commit count") —
  no keyword-search reformulation can satisfy a question with no single matching object; that
  class of question needs `ekos_ekl`'s structured query language, which an agent host aware of
  MCP's tool descriptions would already reach for instead of `ekos_search`/`ask`. Confirmed live:
  even after this fix, that specific question still correctly retrieves nothing — a correct
  "I don't know" is the right behavior for a question this system's retrieval genuinely cannot
  answer, not a regression.
- **Not adding stemming, synonym expansion, or fuzzy matching** to the keyword extraction — a
  conservative, deterministic word-splitter was sufficient to fix every real failure found; a
  fuzzier match strategy is future work if a real case demonstrates the need.
  _Tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "Analyzers" (the same
  semantic-matching gap RFC 0007 names for identity resolution)._

## Design

```rust
fn search_for_question(&self, question: &str) -> Result<Vec<(KirId, String)>, AiError> {
    let terms = extract_search_terms(question);
    if !terms.is_empty() {
        let hits = self.runtime.find_objects(&terms.join(" "))?;       // AND
        if !hits.is_empty() { return Ok(hits); }
        if terms.len() > 1 {
            let hits = self.runtime.find_objects(&terms.join(" OR "))?; // OR
            if !hits.is_empty() { return Ok(hits); }
        }
    }
    Ok(self.runtime.find_objects(question)?)                            // raw fallback
}
```

`extract_search_terms` splits on `!c.is_alphanumeric()` (not just ASCII punctuation, so accented
names like `Hambüchen` survive intact) and filters against `QUESTION_STOPWORDS`, a short,
deliberately conservative closed-class list (articles, auxiliary verbs, question words,
prepositions, pronouns) chosen so a real content word is never mistaken for a stopword.

## Alternatives Considered

- **Stripping only punctuation, not stopwords** — tested against the real failing questions; still
  produces a low-precision AND query (e.g. "who is niklas hambüchen and what did they contribute
  to this repository" minus only punctuation still has 13 words, most content-free) that would
  need the OR fallback almost every time, defeating the precision-first AND step's purpose.
  Stopword removal is what makes the AND path actually succeed for the real cases tested.
- **Only the OR fallback, skipping the AND-first attempt** — rejected: AND-first gives an exact
  keyword-set match priority when one exists (the common case — one real object whose name
  contains every extracted keyword), only broadening to OR when that fails; going straight to OR
  would rank a partial match ahead of an exact one whenever both exist.
- **Rewriting the question into a search string via the LLM itself** (a query-rewriting prompt
  before the grounding prompt) — rejected as unnecessary complexity and an extra LLM round-trip
  for a problem a deterministic tokenizer/stopword-filter already solves for every real case
  found; worth revisiting only if a case surfaces that this simpler approach can't handle.

## Testing

- `extract_search_terms`: strips stopwords and punctuation (the real Niklas Hambüchen question,
  verbatim), splits on `_` matching FTS5's tokenizer (`imported_browsers` → `imported`/`browsers`),
  dedupes preserving order, leaves an already-bare-keyword query unchanged (the pre-existing
  working case must produce identical terms).
- **Real end-to-end regression tests** against a real in-memory `Ledger` (not a mock): a
  full-sentence question ("What does the orders table depend on?") retrieves the same context a
  bare "orders" query already did; a full-sentence question about an underscore-named table
  ("What columns does imported_browsers have?") retrieves that real object — the exact shape of
  the live `analytics/` bug, reproduced and fixed in a fast, hermetic test.
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.
- Live verification: rebuilt `target/release/ekos`, reran `ekos ask` against the real, compiled
  `analytics/` ledger. *"Who is Niklas Hambüchen and what did they contribute to this
  repository?"* now correctly answers "Niklas Hambüchen is a contributor to this repository,
  having made 2 commits," citing real evidence — previously empty. Independently confirmed via
  `ekos query find "columns OR imported OR browsers"` that the OR-fallback path this RFC adds
  correctly surfaces the real `plausible_events_db.imported_browsers` object for the
  `imported_browsers` question too (the LLM's own response to that specific question was
  separately affected by the small local model used in this environment, not by retrieval — see
  devlog_61 for the full accounting).

## Acceptance Criteria

- [x] `extract_search_terms`, `search_for_question` implemented; `gather_context` now calls
      `search_for_question` instead of `Runtime::find_objects` directly.
- [x] 6 new tests (4 `extract_search_terms` unit tests, 2 real end-to-end `Ledger`-backed
      regressions); 10 total in `ai.rs`'s test module.
- [x] Full workspace `cargo build/test/clippy/fmt` clean.
- [x] Live: rebuilt `target/release/ekos`, confirmed the exact previously-failing real question
      now retrieves and correctly answers from real evidence.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0061-ai-runtime-question-keyword-extraction.md` | This RFC |
| `ekos/crates/runtime/src/ai.rs` | `extract_search_terms`, `QUESTION_STOPWORDS`, `AiRuntime::search_for_question`, `gather_context` updated to call it, 6 new tests |

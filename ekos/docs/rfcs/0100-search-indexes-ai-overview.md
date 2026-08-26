# RFC 0100 — Search indexes `ai_overview`/`ai_usage` (RFC F, redesigned)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC F of this session's Runtime/Retrieval gap-closure plan was originally scoped as full
embedding-based semantic search — a new `EmbeddingProvider` trait, vector storage, an ANN or
brute-force cosine index, and a reciprocal-rank-fusion blend with bm25. Before starting that build,
the user asked to think through a cheaper alternative: extract entity properties with an LLM at
compile time and index them for fuzzy/semantic matching at search time — and pointed out this sounds
like something the codebase might already be close to doing.

It was. `RFC 0088` (`llm_description.rs`) already persists real, evidence-grounded `ai_overview`/
`ai_usage` prose onto every `Module`/`Rollup`/`Symbol` with a real compiled `source_span`, at commit
time, opt-in and cost-gated. The gap wasn't LLM extraction — that already existed and was already
being paid for wherever `[llm-description]` is enabled — the gap was that none of that text was ever
fed into search. `KirObject::indexed_content()` (the single function both ledger backends' search
indexing calls) only pulled from `excerpt`/`symbols`/`ocr_text`.

This RFC replaces the embedding-based RFC F plan with the cheaper piece first: make the LLM text
that's already being generated actually searchable. No new trait, no new storage, no new dependency.

## Design

### `KirObject::indexed_content()` grows two more fields

`ai_overview` and `ai_usage` are appended to the existing space-joined content string, same pattern
as `excerpt`/`symbols`/`ocr_text` before them. Every object without these properties (the
overwhelming majority — `[llm-description]` is opt-in) is completely unaffected; the new fields are
simply absent from the join, same as `ocr_text` already was for non-scanned objects.

### A real bug found and fixed while touching this code, not filed away for later

`FactLedger::index_object` (`crates/ledger/src/fact_ledger.rs`) — the RFC 0016 default backend for
every new workspace — turned out to have its own **independent, duplicated, and already-incomplete**
reimplementation of the indexed-content field list, built inline from the raw JSON payload rather
than calling `KirObject::indexed_content()` the way the SQLite backend's `index_object_fts_v1`/`v2`
always did. That inline copy never included `ocr_text` at all — meaning OCR'd scanned-document text
(RFC 0024's whole point) has been silently unsearchable on the default backend since it shipped,
with no equivalent gap on the older SQLite backend. Fixed at the root: `index_object` now
deserializes the payload into a real `KirObject` and calls its own `indexed_content()` — one shared
field list instead of two independently-maintained copies, closing the whole class of "fixed on one
backend, not the other" bug this content list has now hit at least once, not just the new
`ai_overview`/`ai_usage` gap this RFC set out to close.

### Why this is a real "fuzzy"/semantic capability, not just more text in the index

An object's literal name and source excerpt are fixed; `ai_overview` is the model's own
natural-language description, which routinely uses different, related words than the source does —
"purchases"/"sales transactions" for a table literally named `orders`, "greeting message" for a
function whose only literal text is `println!("... says hello")`. Indexing this prose means bm25 can
match a query against a *concept* the source names differently, without needing embeddings, a
similarity threshold, or a second ranking signal to blend in. It's bounded by what the model actually
wrote in the overview (not a continuous similarity space the way real embeddings are), but it needed
zero new infrastructure and was already being generated.

## Non-goals

- **A dedicated `search_aliases` property** (a short LLM-generated keyword/synonym list, indexed as
  its own boosted tantivy field, separate from prose). Discussed as a real, cheap follow-on — more
  precisely targeted at "alternate names for this thing" than prose incidentally containing a
  synonym — but not attempted here; this RFC ships the free win (text already being generated) first
  and leaves a dedicated extraction pass for a future RFC once real usage shows prose coverage isn't
  enough.
- **Typo-tolerant fuzzy matching** (tantivy's built-in `FuzzyTermQuery`, edit-distance matching).
  Genuinely unrelated to LLM extraction — a separate, small, zero-LLM change to `SearchIndex::query`
  — not bundled into this RFC to keep it about one thing.
- **Full embedding-based semantic search.** The original RFC F scope. Not abandoned, just no longer
  the *first* thing attempted — real usage against this cheaper approach will show whether the
  bounded, prose-based matching here is enough, or whether true continuous-similarity search is worth
  the real new infrastructure it needs.

## Verification

4 new unit tests in `crates/kir` (`ai_overview`/`ai_usage` included in `indexed_content()`; no
regression when they're absent). 2 new regression tests in `crates/ledger` against the real
`FactLedger` backend (not just the pure `indexed_content()` function): a real OCR'd-text search that
would have failed before this RFC's `index_object` fix, and a real `ai_overview`-text search. Full
workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test
--workspace`, 106/106 `ekos-ledger` test groups), `tests/integration` 3/3.

Live-verified against a real, freshly built scratch workspace with `[llm-description]` enabled and a
real local Ollama model: a real Rust `main` function's compiled `ai_overview` reads *"The entry point
of a small command-line interface (CLI) tool, specifically the 'Widget' demo. It prints a **greeting
message** to the console."* — the word "greeting" appears nowhere in the function's source (`fn
main() { println!("Widget says hello"); }`) or its own doc comment. `ekos query find "greeting"`
correctly returns this object as its only match, confirming the search-by-concept capability works
end to end, not just that the code compiles.

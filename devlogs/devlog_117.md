# Devlog 117 — RFC 0100: search finds concepts an object's own text never uses

**Date:** 2026-08-26
**PRs:** RFC 0100
**Branch:** main (direct)

---

## Summary

RFC F of this session's Runtime/Retrieval gap-closure plan was originally scoped as full
embedding-based semantic search — the largest, most novel item in the whole six-RFC plan. Before
starting that build, a design discussion with the user found a much cheaper path already half-built
in the codebase: RFC 0088 already generates real, evidence-grounded `ai_overview`/`ai_usage` prose
at commit time, opt-in and cost-gated — it just was never fed into search. This entry closes that
gap, and along the way finds and fixes a real, previously-undiscovered bug: the default ledger
backend's own search-indexing code had silently drifted from what it was supposed to index.

---

## RFC 0100 — Search indexes `ai_overview`/`ai_usage`

### Problem / motivation

`KirObject::indexed_content()` — the one function both ledger backends use to decide what's
searchable — only pulled from `excerpt`/`symbols`/`ocr_text`. RFC 0088's LLM-generated overview text
was real, already paid for wherever `[llm-description]` is enabled, and completely invisible to
`ekos_search`/`find_objects`.

### What was built

| Component | Change |
|---|---|
| `KirObject::indexed_content()` | Now also joins `ai_overview`/`ai_usage` into the searchable text |
| `FactLedger::index_object` | Rewritten to deserialize and call the real `indexed_content()`, instead of its own drifted inline copy |

### A real bug found while touching this code, not a hypothetical

`FactLedger::index_object` (the RFC 0016 default backend for every new workspace) had its own
independent reimplementation of "what to index," built inline from the raw JSON payload rather than
calling `indexed_content()` the way the SQLite backend's equivalent always did. That inline copy
never included `ocr_text` — meaning OCR'd scanned-document text (the entire point of RFC 0024) has
been silently unsearchable on the default backend since it shipped, with no equivalent gap on the
older SQLite path. This is the same "fixed on one backend, not the other" shape this session has
already hit multiple times with unrelated code (the identity-resolver kind-exclusion list, the
artifact-collector dedup gap) — confirmation that a shared primitive with two independent call sites
is a real, recurring risk class in this codebase, not a one-off. Fixed at the root: `index_object`
now deserializes into a real `KirObject` and calls its own method, so there is exactly one field
list from here on, not two that can silently drift apart again.

### Decisions (alternatives considered, why this choice)

- **Shipped the free win before the expensive one.** The original RFC F plan (real embeddings) is
  real, substantial infrastructure — a new provider trait, vector storage, an ANN or brute-force
  index, a ranking-fusion strategy. This RFC delivers a genuine "search finds a concept the source
  never literally names" capability using text the system was already generating, with zero new
  dependencies, zero new storage, and a same-day implementation. Real embeddings remain a legitimate
  future RFC — this doesn't foreclose it, it just means the cheaper thing gets tried and measured
  against real usage first.
- **A dedicated `search_aliases` LLM property (a short synonym list, its own boosted search field)
  was discussed and deliberately deferred, not folded in.** Prose coverage (what's already indexed
  now) and a targeted alias list are genuinely different tools — prose incidentally contains related
  words; a dedicated extraction pass would deliberately ask for them. Worth a future RFC once real
  usage shows prose alone isn't enough, not before.
- **Typo-tolerant fuzzy matching (tantivy's `FuzzyTermQuery`) was named as a related but separate
  idea and explicitly not bundled in.** It's a zero-LLM, self-contained change to `SearchIndex::query`
  with nothing to do with LLM extraction — keeping this RFC about one thing (LLM-generated text
  becoming searchable) rather than two unrelated "fuzzy" mechanisms at once.

---

## Knowledge Captured

- **A search-indexing field list living behind two independently-maintained implementations (one
  per ledger backend) is a real, recurring risk in this codebase, not a one-off oversight** —
  confirmed by this being at least the third time this session found the identical shape of bug
  (something correct in one place, silently stale in its duplicate). Worth treating "does this
  logic exist in more than one place" as a standing question whenever fixing or extending anything
  that both ledger backends independently implement, the same way `CLAUDE.md` already calls this out
  explicitly for the identity resolver's kind-exclusion list.
- **LLM-generated overview text is a real, low-cost source of query-time synonym coverage** — bounded
  by what the model actually chose to write, not a continuous similarity space the way real
  embeddings are, but proven live end to end (a real "greeting" query finding a function whose only
  literal text is `println!("... says hello")`) with genuinely zero new infrastructure. Worth
  remembering as the first thing to reach for the next time "search doesn't find X even though it's
  conceptually related" comes up, before reaching for embeddings.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/lib.rs` | `indexed_content()` includes `ai_overview`/`ai_usage`; 2 new tests |
| `ekos/crates/ledger/src/fact_ledger.rs` | `index_object` reuses `indexed_content()` instead of a drifted inline copy (fixes a real pre-existing `ocr_text` search gap); 2 new tests |
| `ekos/docs/rfcs/0100-search-indexes-ai-overview.md` | New RFC |

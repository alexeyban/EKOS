# RFC 0009 — AI Runtime

**Status:** Accepted
**Date:** 2026-07-10

---

## Problem

The Runtime (RFC 0005) can reconstruct object state and neighbourhoods, but only if the caller
already knows a `KirId`. There is no path from a natural-language question to an answer. AI
agents and end users need to ask questions like "what does the orders table depend on?" and get
back an answer that is grounded in the ledger and cites the evidence it was built from — never a
free-floating LLM guess, and never a raw connection to enterprise systems (invariant from
`ekos.md`: AI systems consume knowledge through the Runtime only).

---

## Solution

`AiRuntime` sits on top of `Runtime` (Phase 10) and `LlmProvider` (RFC 0008 / Phase 6). It never
touches the ledger or enterprise systems directly — only through `Runtime`.

```rust
pub struct AiRuntime<'a> {
    runtime: &'a Runtime<'a>,
    llm: Arc<dyn LlmProvider>,
    config: AiRuntimeConfig,
}

pub struct AiAnswer {
    pub answer: String,
    pub evidence_refs: Vec<KirId>,
}
```

### Pipeline

1. **Retrieve** — `Runtime::find_objects(question)` (FTS5 over object names/kinds, RFC 0005 +
   Phase 10 close-out) ranks candidate objects. Keyword extraction is intentionally naive in v0:
   the raw question string is passed to FTS5's `MATCH`, relying on FTS5's own tokenizer to ignore
   stopwords poorly rather than well — this is accepted as a v0 limitation (see Non-goals).
2. **Expand** — for each of the top-N matches (`N` = `AiRuntimeConfig::max_matches`, default 3),
   `Runtime::load_neighborhood(id, depth)` (`depth` default 1) pulls in directly connected
   objects so the LLM sees relationships, not just isolated rows.
3. **Ground** — the matched objects' `ObjectState` (object + relationships + evidence, via
   `Runtime::reconstruct_state`) is serialized to JSON and embedded in the user turn of the
   prompt. The system prompt instructs the model to answer *only* from the supplied context and
   to end its response with a citation block.
4. **Ask** — `LlmProvider::complete()` (temperature 0, per RFC 0008).
5. **Parse** — the response must end with:
   ```json
   {"cited_evidence": ["<KirId>", "<KirId>"]}
   ```
   Each cited id is validated against the ledger (`Runtime::load_object`-adjacent evidence lookup
   is not needed — evidence ids are looked up via the `ObjectState.evidence` already gathered in
   step 3, or discarded if unknown). If the block is missing or unparsable, emit a
   `Diagnostic::warning` and return `AiAnswer` with empty `evidence_refs` — the answer text is
   still returned, never dropped.

### Prompt template

Stored in `ekos.toml` under `[ai]`, overridable without a code change:

```toml
[ai]
model = "claude-sonnet-4-6"
max-matches = 3
neighborhood-depth = 1
max-tokens = 1024
system-prompt = """
You are the EKOS Knowledge Runtime assistant. Answer only using the JSON context provided.
Every claim must be traceable to the supplied evidence. End your response with a JSON block:
{"cited_evidence": ["<id>", ...]}
If you cannot answer from the given context, say so explicitly.
"""
```

`AiRuntimeConfig` is built from `[ai]` with hardcoded fallback defaults if the section is absent,
mirroring how `LlmConfig` already falls back to `ANTHROPIC_API_KEY` / `claude-sonnet-4-6`.

### CLI

`ekos ask "<question>"`:
- Builds `Runtime` + `AiRuntime` (provider selection reuses `build_llm_provider` pattern from
  `recover.rs`, minus the disk cache — see Non-goals).
- No LLM provider configured (`AnthropicProvider::from_env` fails) → print
  `"No LLM provider configured. Set ANTHROPIC_API_KEY and provider = 'claude' in ekos.toml."`
  and exit 1 — this is a hard requirement, unlike Phase 6's silent structural-only fallback,
  because there is no non-LLM way to answer a free-text question.
- On success: print the answer, then a `Sources:` section listing each cited evidence's
  `location.path` and `fragment`.
- `--json` flag prints the full `AiAnswer` as JSON instead.

---

## Non-goals

- **Caching** — `CachedLlmProvider` (Phase 6) is not wired in for `ekos ask` in v0. Ask queries
  are user-driven and low-volume compared to the batch `ekos recover` analysis passes; adding
  cache-key design for free-text questions (which are not stable across phrasings) is deferred.
- **Smart keyword extraction** — no NLP/embedding-based retrieval. FTS5 prefix/keyword match is
  the entire retrieval mechanism for v0. A future RFC can add embeddings if FTS5 recall proves
  insufficient in practice.
- **Multi-turn conversation / chat history** — `ask()` is stateless, one question in, one answer
  out.
- **Streaming responses** — matches RFC 0008's existing non-goal.

---

## Alternatives considered

**Let the LLM query the ledger directly (tool use / function calling)** — rejected for v0.
Giving the model a live query tool means answers are no longer reproducible from a single logged
prompt+response pair, and it blurs the invariant that the Runtime is the only consumer-facing
surface. A fixed retrieve-then-ground pipeline keeps every `ekos ask` call auditable: the exact
context sent to the model is fully captured in the prompt.

**Return `evidence_refs` as a hard error if the citation block is missing** — rejected. LLMs are
not perfectly reliable about trailing structured output; failing the whole answer over a missing
citation block would make the feature unusable at the reliability level of current models. A
`Warning` diagnostic plus a best-effort answer is more useful.

---

## Acceptance Criteria

- [ ] `AiRuntime::ask()` retrieves via `Runtime::find_objects`, expands via
      `Runtime::load_neighborhood`, and grounds via `Runtime::reconstruct_state`.
- [ ] Prompt sent to the LLM contains the `ObjectState` JSON context (verified with a mock
      provider in tests).
- [ ] `AiAnswer.evidence_refs` is populated from a valid citation block, and is empty (with a
      `Warning` diagnostic) when the block is missing.
- [ ] `ekos ask "<question>"` prints an answer and a `Sources:` section; `--json` prints the raw
      `AiAnswer`.
- [ ] No API key configured → clear error message, exit 1, no panic.

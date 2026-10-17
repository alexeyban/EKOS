# Devlog 276 — a model per task: routes, Zen wire formats, cost, and a measured bake-off

**Date:** 2026-10-17
**PRs:** none (local commits on `main`, `[skip ci]`)
**Branch:** main
**Design:** RFC 0197 (`ekos/docs/rfcs/0197-llm-task-routing.md`)

---

## Summary

Every LLM call in EKOS went to the one `[llm]` model. Now each call site names its task and
`[llm.tasks.<task>]` can route it to its own model, with fallbacks; `provider = "zen"` reaches
every OpenCode Zen model on its own wire format; every call that reaches a host is priced and
logged, a budget refuses calls that could cross it, and data-collecting models are refused. A
$0.73 bake-off (of $2 approved) measured the candidates on the real passes: Claude Haiku 5.5
answers 98/101 eval scenarios for $0.10 a run, and the previous default model parsed **4 of 33**
document sections — the run also found two bugs in the document pass.

---

## RFC 0197 Phase 1 — routes, wire formats, metering

### Problem / motivation
Bulk passes (one call per document section, ~6,000 calls per description regeneration) need the
least capability and cost the most; `ask` and architecture reasoning are the reverse. Zen serves
~80 models on three wire formats, and EKOS spoke only Chat Completions to it.

### What was built

| Component | What |
|---|---|
| `compiler-core` `LlmConfig` | `tasks` (`LlmRoute` per task), `fallback`, `reasoning-effort`, `prompt-cache`, `budget-usd`, `allow-data-collection`, `prices`; `for_task` lays a route over `[llm]` |
| `recovery::llm::Task` | 12 tasks, each call site passes its own |
| `recovery::route` | `LlmRouter`: the only provider builder; `zen_wire` picks the format from the model id |
| `recovery::usage` | `MeteredLlmProvider` (policy, budget, `llm-usage.jsonl`), `FallbackLlmProvider`, `Budget` |
| `recovery::pricing` | Zen's price table (dated), overrides, data-collection list |
| `recovery::openai_responses` | new Responses API provider (GPT, Grok) |
| `AnthropicProvider` | honours `[llm] model` (was always `claude-sonnet-4-6`), base URL, cacheable system block, cache usage |
| `OpenAiProvider` | cached-token usage in both host shapes, `reasoning_effort`, block content |
| `ekos llm routes|usage|probe` | `commands/llm.rs`, catalogue rows (`probe` is External, calls the model) |

### Implementation details worth remembering
- Wrapping order: `Fallback(Cached(Metered(model)))` per route model — a disk-cache hit is free and
  unlogged; each model keeps its own cache entries (the cache key already includes the model).
- One `LlmRouter` per command, so all tasks share the budget; `recover` builds each task's provider
  on first use (`TaskLlm`), so a disabled pass builds none and a missing key warns once.
- The budget reserves the worst case (input estimated from characters, output = `max_tokens`) and
  settles to the real cost; an unknown price is refused only when a budget is set.
- `commit.rs`'s own provider selection is gone (`select_llm_provider_for_description` delegates).
- `LlmResponse` gained `cache_read_tokens`/`cache_write_tokens` and `Default`: the one struct
  literal in the private binary workspace needs `..Default::default()` (applied there, uncommitted).

### Decisions
- Tasks named at the call site, not inferred from `prompt_version` (renames would silently re-route).
- No built-in default routes and no run-time "cheapest that passes": a model or price can vanish,
  and a compile must not depend on prices. Recommendations are documented configuration.

---

## Bake-off

Ran the real passes under routes rather than building `ekos llm bench` first (deferred):
`ekos eval run` per model for `ask`; `ekos recover` with `[document-semantics]` on three public
RFCs for `doc-semantics`. Results are tabled in RFC 0197.

| | Result |
|---|---|
| `ask` | Haiku 5.5 98/101 (94.2%) $0.096; GPT-6 Luna 91/101 $0.056; DeepSeek V4.1 Flash 97/101 (Oct 12) |
| `doc-semantics` @4096 | Luna, Haiku, GLM-5.3 Flash (effort low), MiniMax M3, GPT-5.4 Nano: 40/40; Qwen3.8 Flash 28/40; DeepSeek V4.1 Flash 16/40 |

The `ask` run was stopped by the host's low-memory guard during the third model; it was not
restarted (the remaining models are in TODO).

---

## Knowledge Captured

- **Zen's free models are unreachable from EKOS**: `FreeTierError … only from within OpenCode`.
- **Model access is per account**: 7 of 20 candidates answered `Model access is disabled`,
  including `deepseek-v4-flash`, which `ekos.zen.toml` still names. `ekos llm probe` first.
- **Claude 5.5 refuses `temperature`** ("deprecated for this model"); Mistral Large 4 refuses it
  with a bare `{"object":"error"}` and also refuses `reasoning_effort`. Providers retry a 400 once
  without temperature and keep going without only if that gets past the 400.
- **Reasoning models eat fixed output caps.** The document pass's hard-coded 2048 cut off 29/33
  DeepSeek answers mid-JSON (reported as parse failures). Cut-offs are `DOCSEM006` now, and the
  cap is the route's `max-tokens`. Check `output_tokens >= max_tokens` before blaming a model.
- **The document pass re-extracted old artifacts**: it read every local-docs artifact ever
  stored, so a changed document was extracted twice (33 → 40 sections). Newest per path now.
- **Zen caches GPT prompts by itself** and bills them as cache writes: Luna showed 3 input and
  1,600–3,300 cache-write tokens per `ask` call.
- **The first 20 eval scenarios are 18 adversarial refusals** — `--limit 20` measures nothing.
- Haiku 5.5 is verbose (~1,400 output tokens per section); size caps per model, not per pass.
- Running full evals while the machine is near its memory limit gets them killed; run them one
  at a time with nothing else heavy open.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0197-llm-task-routing.md` | the RFC, probe findings, bake-off tables |
| `ekos/crates/compiler-core/src/config.rs` | routes, prices, budget, data policy in `LlmConfig` |
| `ekos/crates/recovery/src/{llm,route,usage,pricing,openai_responses}.rs` | tasks, router, metering, prices, Responses provider |
| `ekos/crates/recovery/src/{anthropic,openai}.rs` | model/base URL, temperature retry, cache usage, block content, effort |
| `ekos/crates/recovery/src/document_semantics_analyzer.rs` | route `max-tokens`, `DOCSEM006`, newest artifact per document |
| `ekos/crates/cli/src/commands/llm.rs`, `app.rs`, `catalogue.rs` | `ekos llm routes|usage|probe` |
| `ekos/crates/cli/src/commands/{recover,commit,ask,eval,…}.rs` | every call site passes its task |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | model-per-task docs, follow-ups |

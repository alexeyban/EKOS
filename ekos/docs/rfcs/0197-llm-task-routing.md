# RFC 0197 — LLM task routing, cost accounting and a model bake-off

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-17
**Builds on:** RFC 0021/0046 (provider selection), RFC 0145 (OpenAI-compatible hosts, OpenCode
Zen), RFC 0138 (eval harness), RFC 0164 (anti-invention), RFC 0170 (cite-or-drop)

## Problem

Every LLM call in EKOS goes to the one `[llm]` model. The calls are not alike:

| Task | Call site | Volume | Output | What it needs |
|---|---|---|---|---|
| `sql-naming` | `sql_analyzer` | one per schema file | JSON | structured extraction |
| `doc-semantics` | `document_semantics_analyzer` | **one per document section** | JSON, ≤2048 | cheap, valid JSON |
| `description` | `llm_description` (commit) | **one per module / rollup / symbol** | grounded prose | cheap, faithful |
| `semantics` | `semantics_llm` (cite-or-drop) | one per concept | ≤400 | cheap, cites |
| `session-capture` | `session` capture | one per transcript slice | strict JSON | cheap |
| `ask` | `runtime::ai` (`ekos ask`, eval) | one per question | cited answer | reasoning over evidence |
| `architecture` | `architecture_reasoning` | one per `recover` | classification | quality; volume trivial |
| `reconstruct` | `migrate reconstruct` | one per routine | SQL | code; anti-invention gate |
| `nl-sql` | `clickhouse-query` | interactive | SQL | code |
| `docs-prose` | `docs generate --prose` / findings | rare | prose | quality |
| `marketing` | `marketing publish` | rare | short prose | quality |

The bulk tasks dominate cost on a large corpus and need the least capability; the rare ones are
the reverse. One model is either too expensive for the first group or too weak for the second.
Measured on this repository's own cache (1,019 calls): ~2.9k input and ~560 output tokens per
call; a full 101-question eval costs ≈ $0.07 on `deepseek-v4-flash` — a reasoning model whose
hidden reasoning is billed as output (why `[ai] max-tokens = 8192` was needed, RFC 0145).

OpenCode Zen offers ~80 models behind one key, but on **three wire formats**
(opencode.ai/docs/zen, read 2026-10-17): Anthropic Messages (Claude; Qwen 3.5–3.8 Flash/Plus),
OpenAI Responses (GPT, Grok, Muse), and Chat Completions (DeepSeek, GLM, Kimi, MiniMax,
Mistral, Qwen Max, the free models). EKOS speaks only the third to Zen: `AnthropicProvider`
hard-codes `api.anthropic.com` and **ignores `[llm] model`** (always `claude-sonnet-4-6`), and
there is no Responses client.

Also found: `commit.rs` carries its own copy of provider selection for `[llm-description]`,
the duplication RFC 0145 already removed from `docs`/`marketing` once.

## Found before building it (2026-10-17, live probes of `https://opencode.ai/zen/v1`)

- **Free models are not reachable from EKOS**: every `*-free` model answers `FreeTierError:
  OpenCode's free tier can only be used from within OpenCode`. There is no $0 cloud tier; the
  only free route is local Ollama. §5's data policy still matters for `big-pickle` (not free-tier
  gated) and for any model a later host offers.
- **Claude 5.5 rejects `temperature`** (`temperature is deprecated for this model`), and
  `AnthropicProvider` always sends `temperature: 0` — every call to a current Claude model would
  fail. The Messages request omits it for models that refuse it.
- **Model access is per account**: on this key, 13 of 20 candidates answered, the rest returned
  `Model access is disabled` — including `deepseek-v4-flash`, the model this repository's own Zen
  config names. `ekos llm probe` (a one-request call per model, a few tokens each) reports what a
  key can reach, and `doctor` runs it for the configured routes.
- Usage blocks differ: Messages `cache_read_input_tokens`/`cache_creation_input_tokens`;
  Responses `input_tokens_details.cached_tokens`/`cache_write_tokens` and
  `output_tokens_details.reasoning_tokens`; Chat Completions `prompt_tokens_details.cached_tokens`
  or a top-level `cache_read_input_tokens` (DeepSeek). Responses' answer is the `output_text`
  item of the `message` output, after a `reasoning` item.
- Some models carry a provider-side system prompt (`grok-build-0.1`: 189 input tokens for
  "Say OK.") — a fixed per-call overhead that matters for bulk tasks.

## Design

### 1. Tasks and routes

```toml
[llm]                       # the default route, unchanged
provider = "zen"
model = "deepseek-v4-flash"
api-key-env = "OPENCODE_API_KEY"
budget-usd = 2.00           # optional: hard stop for metered calls, see §4

[llm.tasks.doc-semantics]   # any field omitted is inherited from [llm]
model = "glm-5.3-flash-free"
fallback = ["qwen3.8-flash"]

[llm.tasks.architecture]
model = "claude-sonnet-5-5"
```

`ekos_recovery::llm::Task` names the tasks above. `build_llm_provider_for(config, task, dir)`
is the **only** selection function: it merges `[llm.tasks.<task>]` over `[llm]` and builds the
provider; `build_llm_provider` is `…_for(Task::Default)`. `commit.rs`'s copy goes. Every call
site in the table passes its task. A route's `fallback` models are tried in order on a 429 or
5xx, never on a 4xx that would recur.

### 2. `provider = "zen"`

Picks the wire format from the model id (Claude and Qwen 3.5–3.8 non-Max → Messages at
`…/zen/v1/messages`; `gpt-*`, `grok-*`, `muse-*` → Responses at `…/zen/v1/responses`; others →
Chat Completions), base URL `https://opencode.ai/zen/v1` unless `base-url` says otherwise.
`AnthropicProvider` gains `base_url` and honours the configured model; a new
`OpenAiResponsesProvider` speaks Responses. Gemini's native API is out of scope (route Gemini
through nothing until needed).

### 3. Prompt caching

Bulk tasks resend one system prompt per item. The Messages request marks the system block
`cache_control: {type: "ephemeral"}` when the system prompt is long enough to be cached;
OpenAI-style hosts cache prefixes automatically. `LlmResponse` gains `cache_read_tokens` and
`cache_write_tokens` (default 0, so old disk-cache entries still parse) read from each format's
usage block. The disk cache (`CachedLlmProvider`) is unchanged and keyed by model, so a route
change never serves another model's answer.

### 4. Cost, usage and the budget

- A **price table** (`ekos_recovery::pricing`): Zen's published per-million prices for the
  models this RFC routes to, dated, overridable by `[llm.prices."<model>"] input/output/
  cache-read/cache-write`. Ollama is free. A metered model with no price is **unknown**, not free.
- Every call that reaches a provider (not a disk-cache hit) appends one line to
  `<state>/llm-usage.jsonl`: time, task, provider, model, tokens (input, output, cache read/write),
  cost, latency, prompt version. Nothing of the prompt or answer — usage is metadata only.
- `ekos llm usage [--since] [--json]` totals it by task and model.
- `[llm] budget-usd` (or `--budget-usd` on `llm bench`): before each metered call the
  **worst case** (input tokens estimated from characters, output = `max_tokens`) is added to the
  spend recorded in the current window; a call that could cross the budget is refused with
  `LlmError::Budget`, and a metered model with an unknown price is refused whenever a budget is
  set. The window is the process by default; `llm bench` uses its own run.

### 5. Data policy

Zen's free models differ: some providers keep no data and train on none (GLM-5.3-Flash Free,
DeepSeek V4.1 Flash Free, Space Bunny, LongCat); others may use what they receive (Big Pickle,
MiMo, Nemotron — "do not submit personal or confidential data"). `pricing` records the policy;
a model marked `collects` is refused unless `[llm] allow-data-collection = true`. EKOS sends
enterprise content; the default must not hand it to a model that keeps it.

### 6. `ekos llm bench`

`ekos llm bench --task <task> --models a,b,c [--sample N] [--budget-usd X]` replays the task's
real prompts — the requests the task made in this workspace, read back from a sample of
**the task's own inputs** rebuilt by the pass (not the disk cache, which stores answers only) —
against each model and scores each answer with the check that task already has:

| Task | Score |
|---|---|
| `doc-semantics`, `sql-naming`, `session-capture` | parses into the pass's JSON schema; entities found that name text in the input |
| `description`, `semantics` | cite-or-drop: share of sentences whose citations verify |
| `ask` | RFC 0138's evaluators (`ekos eval run --model`) |
| `reconstruct` | anti-invention accepted and the gate passes |

plus cost, latency and output tokens. Results go in a table in this RFC, and the documented
starting configuration follows them. Recommendations are configuration, never hard-coded
defaults: EKOS keeps working with only `[llm]`.

## Rollout

- **Phase 1** — tasks + routes + single selector; `zen` provider with the three formats;
  `AnthropicProvider` model/base URL fix; usage log, prices, budget, data policy; prompt caching.
  No model call in tests (wire formats against recorded fixtures and a local mock server).
- **Phase 2** — `ekos llm probe`, `ekos llm bench` and the bake-off: local Ollama ($0) and the
  paid models the key reaches, within a **$2 total** the maintainer approved (2026-10-17).

## Bake-off results (2026-10-17, Phase 2 run by hand)

`ekos llm bench` was not built: the bake-off ran the real passes under routes instead —
`ekos eval run` for `ask`, `ekos recover` with `[document-semantics]` for `doc-semantics`, each
model in its own workspace or config, every call logged by `llm-usage.jsonl`. Total metered
spend: **$1.08** of the approved $2.

### `doc-semantics` — three public RFCs, the same 40 sections for every model

| Model | Sections parsed, cap 2048 | Sections parsed, cap 4096 | Cost (both rounds) |
|---|---|---|---|
| `gpt-6-luna` | 28 / 33 | **40 / 40** | $0.029 |
| `claude-haiku-5-5` | 26 / 33 | **40 / 40** | $0.032 |
| `glm-5.3-flash` (effort low at 4096) | 15 / 33 | **40 / 40** | $0.034 |
| `minimax-m3` | 32 / 33 | **40 / 40** | $0.050 |
| `gpt-5.4-nano` | 29 / 33 | **40 / 40** | $0.067 |
| `qwen3.8-flash` (effort low at 4096) | 10 / 33 | 28 / 40 | $0.056 |
| `deepseek-v4.1-flash` (effort low at 4096) | **4 / 33** | 16 / 40 | $0.159 |
| `mistral-large-4` | 0 (400) | 0 (400) | — |

Every parse failure in both rounds was an answer cut off at the cap (`output_tokens >= max_tokens`):
reasoning models spend it on hidden reasoning. With this repository's default model the pass
silently dropped 88% of sections.

### `ask` — RFC 0138's 101 scenarios on this repository's ledger

| Model | Passed | Answer correctness | Hallucination | Cost per run | Median latency |
|---|---|---|---|---|---|
| `minimax-m3` | **99 / 101** | 95.8% | 0 | $0.169 | 4.2 s |
| `claude-haiku-5-5` | 98 / 101 | 94.2% | 0 | $0.096 | 5.2 s |
| `deepseek-v4.1-flash` (2026-10-12 run, earlier ledger) | 97 / 101 | 93.1% | 0 | ≈ $0.23 (estimated) | — |
| `glm-5.3-flash` | 95 / 101 | 94.2% | 2% | $0.107 | 6.4 s |
| `qwen3.8-flash` | 92 / 101 | 87.8% | 1% | $0.139 | 19.2 s |
| `gpt-6-luna` | 91 / 101 | 85.7% | 1% | $0.056 | 3.6 s |
| `gpt-5.4-nano` | 87 / 101 | 88.9% | 5% | $0.089 | 2.6 s |

`code-004` fails on every model (a fact the ledger does not record). MiniMax's other miss is
`lin-011`; Haiku's are `arch-015` and `lin-004`, which DeepSeek misses too. The API key expired on
OpenCode's side mid-run (`401 api_key_expired` from 16:57 UTC); the 49 affected calls were rerun
on the same configuration after the key was replaced — answers already given came from the disk
cache, so no scenario was answered twice.

### Starting configuration these support

```toml
[llm]
provider = "zen"
model = "claude-haiku-5-5"      # every task without a route: 98/101 at $0.10 a run
api-key-env = "OPENCODE_API_KEY"

[llm.tasks.ask]                 # highest score (99/101), 1.8x Haiku's cost; or leave on Haiku
model = "minimax-m3"
fallback = ["claude-haiku-5-5"]

[llm.tasks.doc-semantics]       # bulk extraction: cheapest that parsed everything
model = "gpt-6-luna"
max-tokens = 4096
fallback = ["claude-haiku-5-5"]
```

`description` (≈ 6,000 calls per regeneration of this repository) has the same shape as
`doc-semantics` and is the largest saving: at list prices about $2 on Haiku 5.5 or Luna against
about $6 on `deepseek-v4.1-flash`, before its truncation losses. Not measured.

### Found while benchmarking

- **The document-semantics pass reprocessed old artifacts.** It read every local-docs artifact the
  store ever held, so a document whose artifact changed between two `recover` runs was extracted
  from both (33 sections became 40, half of them paid twice, the old text kept). Per document path,
  only the newest artifact's sections count now.
- **The pass had a fixed 2048-token cap.** It takes the route's `max-tokens` now, and a cut-off
  answer is `DOCSEM006` ("raise `[llm.tasks.doc-semantics] max-tokens`"), not a parse failure.
- **Not every model says it refuses `temperature`**: Mistral Large 4 on Zen answers a bare
  `{"object":"error"}`. Every provider now retries a 400 once without `temperature`, and keeps
  going without only if that retry gets past the 400. Mistral also refuses `reasoning_effort`, so
  it stays unreachable; and it returns `content` as blocks (`thinking`, `text`), which Chat
  Completions parsing now accepts.
- **Zen writes GPT prompts to its cache by itself** and bills them at the cache-write price
  (`gpt-6-luna`: 3 input tokens and ~1,600–3,300 cache-write tokens per `ask` call).
- **A 20-scenario screen is useless**: the first 20 scenarios are 18 adversarial refusals.

## Decided against

- **Routing by prompt version** inside one provider: invisible in config and breaks when a
  prompt is renamed. Tasks are named where the call is made.
- **Automatic model choice at run time** (cheapest that passes): quality must be measured once,
  deliberately, and written down; a run-time choice would make a compile depend on prices.
- **Built-in default routes**: a price or a model can disappear; the defaults stay `[llm]`.

## Out of scope

Gemini's native API, streaming for the Responses format beyond what `ekos ask` needs, the
Lakehouse gold drafter (its own repository; it adopts `Task::Reconstruct`-style routing later).

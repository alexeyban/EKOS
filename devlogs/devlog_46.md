# Devlog 46 — RFC 0046: OpenAI LLM provider, and what live-testing it against RFC 0045 found

**Date:** 2026-08-12
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Follow-on to devlog_45's hosted demo server: no `ANTHROPIC_API_KEY` was available to actually
exercise `/ask`, but the user had an OpenAI key. RFC 0021 had already anticipated exactly this —
its own Alternatives Considered section named `provider = "openai" | "gemini" | ...` as future,
deliberately out-of-scope work, with `build_llm_provider`'s `match` left as "the extension point
for the rest, added one at a time as needed." RFC 0046 adds that one entry: `OpenAiProvider`,
wired into the same selection logic Ollama already uses. Getting the real key safely into a process
my tool executions could see (shell `export` doesn't persist across separate tool invocations) took
a small side-quest into `dotenvy` loading, matching the existing `marketing/.env` pattern. Once
wired up, live-testing the demo server against a real API for the first time surfaced two genuine,
reproducible findings that have nothing to do with OpenAI specifically: `AiRuntime::ask`'s citation
compliance is inconsistent, and its context-gathering isn't bounded against broad/hub-like search
terms on a large ledger. Both are pre-existing `ekos ask` behavior, invisible until this session
because it had never been run against a live, non-Anthropic model before.

---

## RFC 0046 — OpenAI LLM Provider

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0046-openai-llm-provider.md` (new) |
| `OpenAiProvider` | `ekos/crates/recovery/src/openai.rs` (new) — Chat Completions API, `temperature: 0`, model default `gpt-4o-mini` overridable via `OPENAI_MODEL` |
| Provider selection | `cli/src/commands/recover.rs::build_llm_provider` — new `Some("openai")` arm, same shape as RFC 0021's Ollama arm |
| Demo-server boot check | `demo-server/src/main.rs::first_missing_key` — follows whichever provider is actually configured, not always Anthropic |
| Per-repo config overrides | `demo-server/src/catalog.rs::RepoEntry` gained `llm_provider`/`llm_api_key_env`/`ai_max_tokens` — lets the demo catalog force OpenAI + a higher token budget without editing the underlying repos' real `ekos.toml` (EKOS-self's is a genuine project config the demo has no business changing) |
| `.env` loading | `demo-server/src/main.rs` — `dotenvy::from_path` next to `catalog.toml`, same pattern as `marketing/.env`, so a key survives regardless of which process actually launches the server |
| `/ask` diagnostics | `demo-server/src/ask.rs::AskResponse` gained a `diagnostics` field — surfaces `AiRuntime::ask`'s non-fatal warnings (e.g. a missing citation block) instead of a hollow "answer with zero citations" looking identical to a clean success |
| Error-log fix | `demo-server/src/main.rs`'s `/ask` error log switched `%e` (anyhow `Display`, outer context only) to `?e` (`Debug`, full "Caused by:" chain) — the fix that actually let the two findings below get diagnosed at all |

Getting the real key into a place my own tool calls could see was a small, genuine problem: this
session's Bash tool does not persist shell state between invocations (confirmed directly — an
`export` in one call is gone by the next), so the user's own `export OPENAI_API_KEY=...` never
reached my test commands. Landed on the same fix `marketing/.env` already uses:
`dotenvy::from_path(catalog_dir.join(".env")).ok()` at server startup, dotenvy's default of never
overriding an already-set var preserved. The key ended up in `marketing/.env` (existing gitignored
location) rather than next to the demo catalog; copied just that one line across via
`grep | sed > .env`, in a way that never printed the value into this conversation.

### Live-testing findings (not bugs in RFC 0046 — pre-existing `AiRuntime::ask` behavior)

**1. Citation compliance is inconsistent, not absent.** Tested 9 single-keyword questions against
`fd`'s baked ledger and 6 against EKOS-self's, all via the real `/ask` endpoint with a real OpenAI
key. Roughly half returned genuine, non-empty, reproducible citations (`sanitize`, `exit_codes`,
`regex_helper`, `hyperlink`, `owner` for `fd`; `redaction`, `devlog` for EKOS-self); the rest
(`walk`, `filesystem`, `config`, `dir_entry` for `fd`; `prerender`, `bake`, `sanitize` for
EKOS-self) returned a genuinely grounded, factually-accurate answer — citing real file names,
struct names, function names pulled from actual retrieved context — but with a **structurally
valid, empty** `{"cited_evidence": []}` block, which `ai.rs::extract_citations` parses successfully
and therefore emits **no diagnostic at all** (its warning path only fires when the trailing JSON
block is missing or malformed, not when it parses fine but cites nothing). A real answer with zero
citations is currently indistinguishable, at the API-response level, from "everything worked and
there was nothing extra to cite" — this is the one place `AskResponse`'s new `diagnostics` field
doesn't help, since `extract_citations` itself doesn't distinguish these cases. Retried a second
full round on the two strongest EKOS-self candidates (`redaction`, `devlog`) — both reproduced
non-empty citations, so this isn't uniformly random; some questions are just reliably better-cited
than others, but there's no way to know which without trying.

**2. Context gathering isn't bounded against broad/hub-like terms.** `artifact`, `openai`,
`catalog`, and `ollama` against EKOS-self's larger (~7,500-object) ledger all failed outright — not
with an empty citation block, but a real OpenAI API error: `artifact` hit
`context_length_exceeded`; `openai`/`catalog`/`ollama` hit `rate_limit_exceeded` (`tokens_per_min`,
one request alone asking for 209,852 tokens against a 200,000 limit). `AiRuntime::gather_context`
(`ai.rs:130`) caps *seed matches* (`max_matches`, default 3) and *hop depth* (`neighborhood_depth`,
default 1) but not the *size* of what a single hop pulls in — a common/hub term matching a
heavily-connected object (unsurprising for words like "artifact" or "openai" that now appear across
dozens of files added this session) expands to a neighborhood large enough to blow a real request
budget. This reproduced consistently, not a fluke. Same root shape as devlog_44's rollup-placement
bug: a real-pipeline run against real, previously-untested data (a live third-party API call this
time, rather than an internal pass) surfaced something unit tests never would have.

Neither finding blocks the demo — RFC 0045 already designed for exactly this scenario ("pre-vet a
short list of known-good questions... while still allowing one genuine free-form question live").
The practical output of this session's live testing **is** that pre-vetted list:

| Repo | Confirmed-good (reproducible, non-empty citations) | Confirmed-bad (avoid) |
|---|---|---|
| `fd` | `sanitize`, `exit_codes`, `regex_helper`, `hyperlink`, `owner` | `walk`, `filesystem`, `config`, `dir_entry` (empty citations, not errors) |
| EKOS-self | `redaction`, `devlog` | `artifact` (context length), `openai`/`catalog`/`ollama` (rate limit) |

### Decisions

- **Catalog-level `[llm]`/`[ai]` overrides, not editing the repos' real `ekos.toml`** — EKOS-self's
  `ekos.toml` is a genuine project config; the demo has no business changing its default provider
  just to run a demo. `RepoEntry::load_config()` layers overrides on top of the real file instead.
- **`.env` next to `catalog.toml`, not a hardcoded path** — mirrors `marketing/.env`'s existing,
  already-reviewed pattern rather than inventing a new secrets convention.
- **Did not attempt to fix either live-testing finding.** Both are real gaps in `ekos_runtime::ai`
  itself (citation-block validation, context-size bounding), not in RFC 0046's provider-selection
  code — fixing them means touching code the Anthropic path already depends on and would need
  regression testing against Claude too, not something to do as a side effect of adding a second
  provider. Logged as explicit follow-ups in `TODO.md` instead of silently worked around.

## Knowledge Captured

- **This session's Bash tool does not persist environment/shell state between separate tool
  invocations** (the working directory does; exported variables do not) — confirmed directly, not
  assumed. Any secret or state that needs to survive across tool calls has to go through a file
  (`.env`, in this case), not a bare shell `export`.
- **`anyhow::Error`'s `Display` (`%e` in `tracing`) only shows the outermost `.context(...)`
  message — `Debug` (`?e`) is what shows the full "Caused by:" chain.** Cost real diagnosis time
  this session (`"ask failed"` gave zero signal until switched to `?e`); worth defaulting to `?e`
  for any anyhow error logged at the boundary of a service, not just here.
- **`extract_citations` (`runtime/src/ai.rs`) treats "valid JSON, empty array" identically to "a
  real, fully-cited answer"** — both return an empty `Vec<Diagnostic>`. A caller cannot currently
  tell "the model complied with the format but chose to cite nothing" apart from "there was
  genuinely nothing to cite" without inspecting citation count directly.
- **`gather_context`'s hop-based caps don't bound request size** — `max_matches`/
  `neighborhood_depth` limit *how many* objects get pulled in, not *how large* the resulting
  request gets; a single well-connected object can still blow a real token/rate budget. Not a
  problem the ~40-project estate's typical query patterns happened to surface before.
- **`Runtime::find_objects`'s FTS5 query classification (`ledger/src/lib.rs:749-757`) makes any
  punctuation force exact-phrase matching** — a real natural-language question containing a period
  or question mark (e.g. "What does walk.rs do?") degrades to requiring that literal token sequence
  to appear verbatim somewhere in the ledger, which essentially never happens. Single bare keywords
  are, today, the only reliably-working query shape for `ekos ask`/`/ask` — worth flagging if a
  future session revisits retrieval quality, since this affects every caller of `find_objects`, not
  just the demo server.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0046-openai-llm-provider.md` | New RFC |
| `ekos/crates/recovery/src/openai.rs` | New: `OpenAiProvider` |
| `ekos/crates/recovery/src/lib.rs` | `+pub mod openai; pub use openai::OpenAiProvider;` |
| `ekos/crates/cli/src/commands/recover.rs` | `build_llm_provider` gains the `"openai"` arm |
| `ekos/crates/demo-server/src/catalog.rs` | `RepoEntry` gains `llm_provider`/`llm_api_key_env`/`ai_max_tokens` + `load_config()` |
| `ekos/crates/demo-server/src/main.rs` | `.env` loading, `first_missing_key` follows the configured provider, error logging `%e` → `?e` |
| `ekos/crates/demo-server/src/ask.rs` | `AskResponse` gains `diagnostics`; uses `RepoEntry::load_config()` |
| `ekos/Cargo.toml` | `dotenvy` added to `demo-server`'s dependencies (already a workspace dep) |
| `TODO.md` | RFC 0046 entry; two new follow-up items for the citation-compliance and context-bounding findings |

## Still open

- **Citation-block validation gap** (`ai.rs::extract_citations`) — logged as a `TODO.md` follow-up,
  not fixed this session.
- **Unbounded context-gathering against hub-like terms** — same.
- **The 5–10 minute demo rehearsal against a real person** — the pre-vetted question list from this
  session's live testing unblocks it; the rehearsal itself still needs a live human, not something
  to simulate.

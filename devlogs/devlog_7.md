# Devlog 7 — Phase 11: AI Runtime

**Date:** 2026-07-10
**PRs:** —
**Branch:** main

---

## Summary

Implemented Phase 11 — the AI Runtime. `AiRuntime` sits on top of the Phase 10 `Runtime` and the
Phase 6 `LlmProvider`, turning a free-text question into a grounded, evidence-cited answer without
ever touching the ledger or an enterprise system directly. New `ekos ask "<question>"` CLI command.
Wrote RFC 0009 first per the mandatory workflow. Also consolidated a stray `docs/rfcs/` directory
that had drifted to `ekos/docs/rfcs/` in an earlier session (RFC 0005 was the only file there — moved
back to the repo-root `docs/rfcs/` alongside 0000–0003 and 0006–0008 so RFC numbering lives in one
place). 4 new runtime tests (`ai::tests::*`), all workspace tests pass, clippy clean, verified
end-to-end against a real built ledger.

---

## PR — AiRuntime, `[ai]` config, `ekos ask`

### Problem / motivation

Phase 10 gave the Runtime query methods (`load_object`, `load_neighborhood`, `reconstruct_state`,
`find_objects`), but only for callers who already know a `KirId`. There was no path from a
natural-language question to an answer. This is the final assembly phase before v1.0: AI agents
and users need to ask questions and get answers that are traceable to evidence, never a free-floating
LLM guess and never a direct connection to raw enterprise systems.

### What was built

| Component | Change |
|---|---|
| `docs/rfcs/0009-ai-runtime.md` | RFC 0009 (accepted) — pipeline design, prompt format, non-goals |
| `crates/runtime/src/ai.rs` | `AiRuntime`, `AiRuntimeConfig`, `AiAnswer`, `AiError`, citation parsing |
| `crates/runtime/src/lib.rs` | `pub mod ai`; `ObjectState` now `Serialize` (needed for prompt grounding) |
| `crates/runtime/Cargo.toml` | Added `ekos-compiler-core`, `ekos-recovery`, `serde`, `serde_json`, `async-trait`; `tokio` as dev-dep |
| `crates/compiler-core/src/config.rs` | New `AiConfig` + `EkosConfig.ai` field (`[ai]` section in `ekos.toml`) |
| `crates/cli/src/commands/ask.rs` | `ekos ask "<question>"` — builds `AnthropicProvider` from env, prints answer + `Sources:`, `--json` flag |
| `crates/cli/src/bin/ekos.rs` | Wired `Commands::Ask { question, json }` |
| `docs/rfcs/0005-runtime.md` | Moved from `ekos/docs/rfcs/` to repo-root `docs/rfcs/` (see Knowledge Captured) |

### Implementation details worth remembering

**Pipeline (retrieve → expand → ground → ask → parse):**
1. `Runtime::find_objects(question)` — the raw question string goes straight into FTS5 `MATCH`
   as-is (relying on the Phase 10 close-out fix that escapes special characters into a literal
   phrase). No keyword extraction in v0 — this is an explicit RFC 0009 non-goal.
2. Top `max_matches` (default 3) results are each expanded via `Runtime::load_neighborhood(id,
   depth)` (default depth 1) to pull in directly connected objects, deduplicated by object id.
3. Every gathered object is turned into a full `ObjectState` via `Runtime::reconstruct_state` and
   serialized to JSON as the grounding context embedded in the user turn of the prompt.
4. `LlmProvider::complete()` (temperature 0, per RFC 0008 — `AnthropicProvider` already enforces this).
5. The response is parsed for a trailing `{"cited_evidence": [...]}` block: find the last `{` in
   the string, split there, try to parse the tail as JSON. On success, cited ids are filtered
   against the evidence ids actually present in the gathered context (unknown/bogus ids are
   silently dropped, not errored). On failure, the *entire* response text becomes the answer and
   a `Diagnostic::warning("AI001", ...)` is attached — the answer is never discarded just because
   the model forgot the citation footer.

**`AiRuntime<'a>` borrows `&'a Runtime<'a>`**, mirroring the existing `Runtime<'a>` → `&'a Ledger`
lifetime pattern from RFC 0005. No new `Arc`/ownership refactor needed since CLI commands construct
all three (`Ledger`, `Runtime`, `AiRuntime`) in the same scope.

**No caching for `ekos ask`.** `CachedLlmProvider` (Phase 6) is deliberately not wired in — cache
keys are keyed on exact prompt text, and free-text questions aren't stable across phrasings the way
batch SQL/Git analysis prompts are. Documented as a non-goal in RFC 0009; can revisit if ask-query
volume becomes a cost concern.

**`ekos ask` reuses `AnthropicProvider::new(model, api_key)` directly** rather than
`AnthropicProvider::from_env_var`, because the model name needs to come from `[ai].model` /
`AiRuntimeConfig::default()`, not the provider's own hardcoded default — otherwise `[ai].model`
overrides in `ekos.toml` would be silently ignored.

### Decisions

**Tool-use / function-calling for the LLM to query the ledger directly** — rejected. A fixed
retrieve-then-ground pipeline keeps every `ekos ask` call fully auditable: the exact JSON context
sent to the model is captured in one prompt, with no follow-up queries the model could make that
aren't logged. This matches the existing invariant that the Runtime is the only consumer-facing
surface — an LLM with a live query tool would effectively bypass that boundary.

**Hard-fail on missing API key (unlike Phase 6's silent structural-only fallback for `ekos
recover`)** — there's no non-LLM way to answer a free-text question, so `ekos ask` exits 1 with a
clear message instead of returning an empty/degraded answer.

---

## Knowledge Captured

- **RFC numbering had split across two directories.** `docs/rfcs/0005-runtime.md` had landed under
  `ekos/docs/rfcs/` in devlog 5's session (working directory was probably `ekos/` at the time),
  while every other RFC lives in the repo-root `docs/rfcs/`. Moved it back and removed the empty
  `ekos/docs/` tree. Worth double-checking `pwd` before writing to `docs/rfcs/NNNN-*.md` in future
  sessions — CLAUDE.md's mandatory workflow assumes a single canonical RFC directory.
- **FTS5 query safety (from Phase 10 close-out) is a hard dependency for Phase 11.** `AiRuntime`
  passes raw user questions straight into `Runtime::find_objects`, so any FTS5 syntax character in
  a question (hyphens, colons, quotes) would have broken `ekos ask` if the escaping fix from
  devlog 6 hadn't already landed. This is exactly the kind of coupling the "just-in-time RFC"
  process is supposed to catch by building phases in order.
- **`ObjectState` needed retrofitting with `#[derive(Serialize)]`.** It was `Debug, Clone` only in
  Phase 10 because nothing serialized it before. Any future Runtime type intended for prompt
  grounding needs to be added with `Serialize` from the start.
- **Phase 11 is fully complete.** All acceptance criteria from RFC 0009 hold: prompt grounding
  verified via mock LLM tests, citation parsing verified for present/absent/bogus-id cases,
  `ekos ask` verified end-to-end against a real built ledger (SQL schema → recover → resolve →
  compile → commit → ask), and the missing-API-key path was manually confirmed to exit 1 with the
  exact message specified in the TODO validation criteria.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0009-ai-runtime.md` | New RFC (accepted) |
| `docs/rfcs/0005-runtime.md` | Moved from `ekos/docs/rfcs/` |
| `crates/runtime/src/ai.rs` | New: `AiRuntime`, `AiRuntimeConfig`, `AiAnswer`, `AiError`, 4 tests |
| `crates/runtime/src/lib.rs` | `pub mod ai`; `ObjectState` derives `Serialize` |
| `crates/runtime/Cargo.toml` | New deps: `ekos-compiler-core`, `ekos-recovery`, `serde`, `serde_json`, `async-trait`, `tokio` (dev) |
| `crates/compiler-core/src/config.rs` | New `AiConfig` + `EkosConfig.ai` |
| `crates/cli/src/commands/ask.rs` | New: `ekos ask` command |
| `crates/cli/src/commands/mod.rs` | `pub mod ask;` |
| `crates/cli/src/bin/ekos.rs` | `Commands::Ask { question, json }` |
| `TODO.md` | Ticked all 4 Phase 11 items — Phase 11 fully complete |

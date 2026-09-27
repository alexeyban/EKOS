# Devlog 221 — Hygiene sweep: one provider selection, stale Migrate docs, a clean `unsafe` audit

**Date:** 2026-09-27
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The first step of a project-wide gap audit (drawbacks, gaps and missing pieces across `TODO.md`'s
127 open items). Mostly documentation catch-up, plus one real bug: `docs generate --prose` and
`ekos marketing publish` never routed `[llm] provider = "openai"`. This repo's own `ekos.toml` uses
that provider, so both commands were broken here.

---

## The bug: two private copies of provider selection

`recover.rs::build_llm_provider` knows three providers. `docs.rs::select_llm_provider_for_prose`
and `marketing.rs::select_llm_provider` were written as copies of it back when there were two, and
never gained the third. With `provider = "openai"`, both built an `AnthropicProvider` from
`ANTHROPIC_API_KEY`, or from whatever `api-key-env` named, which is the OpenAI-compatible key. They
then failed on the first request.

TODO.md had flagged these two copies before, but only for the Ollama *model* half, and that half had
since been fixed. The *provider* half was never noticed. Nothing checks that a copy stays in sync, so
a copy gets fixed for whichever bug someone happened to hit.

**Fix:** `build_llm_provider_strict` returns `Result` and never mocks. `build_llm_provider` is now
that function plus the mock fallback, and both former copies call the strict one. There is one
routing table, reached from every site.

`doctor` had the same shape in miniature: a literal `"ANTHROPIC_API_KEY"` default regardless of
provider (devlog_180 fixed the identical bug at another call site). It now uses
`recover::default_key_env`.

## Other items

| Item | Result |
|---|---|
| README "EKOS Migrate (foundation only)" | Rewritten against `ekos migrate --help`: all 12 verbs, what each guarantees, what is not built |
| Capabilities page Migrate section | Same, as a feature grid per RFC; was also "foundation only" |
| `CLAUDE.md` | devlog 193 → 220, RFC 0150 → 0167, one crate-map row for the 8 Migrate crates |
| `TODO.md` | 6 already-shipped Migrate items de-duplicated/ticked (verified in devlog_217/218 + code); load **throttling** split out as genuinely open |
| `web/ui/coverage/` ingestion (2,519 junk `JsSymbol`s) | `coverage` added to this repo's `[observe] ignore-patterns`; takes effect next rebuild |
| `unsafe` audit | **Clean.** The one production use is the documented `memmap2` map in `ledger/src/segment/map.rs` under a crate-level `deny(unsafe_code)`. Every other use is `env::set_var`/`remove_var` in `#[cfg(test)]`, which edition 2024 requires, each with a SAFETY note. |

---

## Knowledge Captured

- **A copied function is only fixed for the bug someone hit.** The TODO note on
  `select_llm_provider_for_prose` described one divergence (model) while a second divergence
  (provider) sat in the same lines. When a TODO names a copy, remove the copy instead of patching the
  named difference.
- **`ignore-patterns` match a bare directory name, never a path.** `web/ui/coverage` as a pattern
  matches nothing. A generic name like `coverage` is safe only after checking that the workspace
  contains no real source directory with that name. Do that check per workspace; never add such a
  name to the built-in defaults.
- **The audit's first guess about `unsafe` was wrong.** Grepping for `unsafe` finds 26 lines, and
  nearly all of them are test-only env mutation. Read each hit in context before calling something a
  rule violation.
- **The docs can fall a whole sub-project behind.** The README and the capabilities page stayed on
  "foundation only" through 15 feature devlogs (206–220), even though the devlog rule requires
  updating both. Check them per devlog, not per phase.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/recover.rs` | `build_llm_provider_strict` (no mock); `build_llm_provider` wraps it; `default_key_env` → `pub(crate)`; 1 test |
| `ekos/crates/cli/src/commands/docs.rs` | `select_llm_provider_for_prose` delegates to the strict builder |
| `ekos/crates/cli/src/commands/marketing.rs` | `select_llm_provider` delegates to the strict builder; unused imports dropped |
| `ekos/crates/cli/src/commands/doctor.rs` | Per-provider default key env; 1 test |
| `ekos.toml` | `coverage` ignore pattern, with the safety check recorded |
| `README.md` | Migrate section rewritten |
| `docs/generated/ekos-self-documentation.html` | Migrate section rewritten |
| `CLAUDE.md` | Status, RFC range, Migrate crate-map row |
| `TODO.md` | De-duplication; coverage, doctor and the provider-selection items ticked |

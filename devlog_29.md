# Devlog 29 — RFC 0027: Marketing Agent v1 (devlog → tweet → approval → X)

**Date:** 2026-08-04
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

The user handed over a design doc (`ekos-marketing-agent-plan.md`, by Alexey Banaev) for a v1
marketing agent: watch for a new `devlog_N.md`, draft an X (Twitter) post, get human approval,
publish, never double-post. This session turned it into RFC 0027 and built it end to end — a new
`ekos-marketing` crate, an `ekos marketing publish` CLI command, and real OAuth 1.0a signing for
the X API — adapted to two things the source doc got wrong about this specific repo: the actual
devlog format, and where configuration lives. 44 new tests, all green; `cargo build/test/clippy
-D warnings/fmt --check` all pass across the full workspace; the CLI was exercised live against
real devlog content (both the LOW-importance skip and the duplicate-detection skip paths ran
through the actual binary, not just unit tests).

---

## RFC 0027 — Marketing Agent v1

### Problem / motivation

EKOS ships real work every session (28 devlogs at the time this one started) but nothing tells
the outside world when it happens — release communication is manual. The source design doc scoped
a deliberately small v1: one devlog in, one tweet out, human-gated, no LinkedIn/threads/images/
scheduling/analytics for now.

### What was built

| Component | File | Detail |
|---|---|---|
| Devlog parser | `ekos/crates/marketing/src/devlog.rs` | `DevlogSummary { number, title, date, summary, section_titles }` — targets the real `## Summary` structure, not the doc's illustrative `## Added`/`## Changed` example |
| Importance classifier | `ekos/crates/marketing/src/importance.rs` | Deterministic keyword heuristic (Low/Medium/High); Low short-circuits before any LLM call |
| Prompt builder | `ekos/crates/marketing/src/prompt.rs` | System prompt transcribed verbatim from the source doc's "Tweet Prompt" rules |
| Tweet drafting + validation | `ekos/crates/marketing/src/tweet.rs` | One `LlmProvider::complete` call (reuses RFC 0008's trait from `ekos-recovery`), then hard server-side validation (≤280 chars, mentions EKOS, includes GitHub URL, ≤3 hashtags), one retry on failure |
| OAuth 1.0a signing | `ekos/crates/marketing/src/oauth1.rs` | RFC 5849 from spec — HMAC-SHA1, percent-encoding, signature base string, `Authorization` header |
| Publisher | `ekos/crates/marketing/src/publisher.rs` | `Publisher` trait; `TwitterPublisher` (`POST /2/tweets`); `NoopPublisher` for dry-run/disabled |
| Duplicate store | `ekos/crates/marketing/src/store.rs` | `marketing/posted/tweets.json` — load/record/save, dedup by devlog number |
| CLI command | `ekos/crates/cli/src/commands/marketing.rs` | `ekos marketing publish [devlog] [--yes] [--dry-run]` — full orchestration, stdin Y/N/E approval, plain-text run log |
| Config | `ekos/crates/compiler-core/src/config.rs` | `[marketing]` / `[marketing.twitter]` in `ekos.toml`, following the exact pattern `[document-semantics]` already established |
| `docs/rfcs/0027-marketing-agent.md` | new | Full RFC, written first, updated twice mid-session as real devlog structure and a test bug surfaced better answers |

### Implementation details worth remembering

- **The source doc's example devlog format doesn't exist in this repo.** `CLAUDE.md`'s own
  `## PR #N — <title>` template is aspirational — a `grep -h '^## ' devlog_*.md` survey across all
  28 files found free-form headings instead (`## RFC 0025 — ...`, `## Phase 6 — ...`, `## Bug 2 —
  ...`, `## Part 3 — ...`). Only three level-2 headings are universal: `Summary`, `Knowledge
  Captured`, `Files Changed`. The parser leans on that: `summary` comes from `## Summary` alone,
  and `section_titles` collects every `## ` heading except those three universal meta-sections —
  robust to whichever convention a given devlog happened to follow, rather than parsing a pattern
  the corpus doesn't actually use.
- **Config lives in `ekos.toml`, not a new `marketing/config.yaml`.** The source doc specified a
  standalone YAML config; this repo has exactly one config file, one format, and one established
  pattern for opt-in features (`[llm]`, `[document-semantics]`). Introducing a second format for
  one feature would be needless divergence, so `[marketing]`/`[marketing.twitter]` went into
  `EkosConfig` instead, `deny_unknown_fields`-compatible like everything else there. `marketing/`
  on disk is state only now (`posted/tweets.json`, `logs/marketing.log`).
- **`generate_tweet` retries once on validation failure**, feeding the specific rejection reason
  (e.g. "tweet was 312 characters, maximum is 280") back into the prompt — matches the source
  doc's "Tweet too long → Regenerate" rule, tested with a mock that always returns an over-length
  draft to confirm `generate_tweet` surfaces `ValidationFailed` rather than looping forever.
- **Reusing `recover.rs`'s LLM-provider selection was a real bug, caught by actually running the
  CLI.** `build_llm_provider` in `recover.rs` falls back to a hardcoded mock
  (`{"entities":[],"relationships":[]}`) when no API key is set — correct for knowledge recovery,
  which has a legitimate "structural analysis only" degraded mode. Tweet drafting has no such
  degraded mode: reusing that fallback would have made `ekos marketing publish` silently call an
  LLM stand-in shaped for a different feature, then fail downstream with a confusing "missing
  field `tweet`" JSON error instead of a clear one. Fixed by giving `marketing.rs` its own
  `select_llm_provider` that mirrors `recover.rs`'s Ollama/Anthropic routing but returns a clear
  `anyhow!("ANTHROPIC_API_KEY not set and no [llm] provider = \"ollama\" configured ...")` instead
  of falling back to a mock. Verified live: `ekos marketing publish 28 --dry-run --yes` in this
  sandbox (no `ANTHROPIC_API_KEY`) now fails with that exact clear message, after correctly
  parsing devlog 28 and classifying it High.
- **This project's own GitHub URL (`.../EKOS`) made a validation test lie to itself.**
  `validate_tweet`'s "must mention EKOS" check is a case-insensitive substring match, and the
  default GitHub URL ends in `/EKOS` — so any tweet fixture containing the GitHub link
  automatically also "mentions EKOS" via the URL, even with zero prose mentioning it. A test meant
  to isolate the missing-EKOS-mention case had to drop the GitHub link from its fixture text
  entirely to actually test what it claimed to test. Not a code bug — a reminder that this
  project's own name being a repo-path suffix makes naive substring checks against it easy to
  fool accidentally.
- **RFC 2202's canonical HMAC-SHA1 test vector output is 40 hex characters
  (`b617318655057264e28bc0b6fb378c8ef146be00`), not 39.** Transcribed it from memory with the
  trailing `0` dropped on the first pass; `cargo test` caught it immediately (`hmac`/`sha1` are
  RustCrypto crates, trustworthy — the typo was mine, not theirs). A reminder that "known test
  vector, recalled from memory" still needs to actually run before being trusted.
- **No Rust toolchain existed in this environment at the start of the session** — `cargo` wasn't
  on `PATH`, no toolchain under `~/.cargo`. Installed via `rustup` (stable, `aarch64-apple-darwin`,
  1.97.1) mid-session specifically so this RFC's code could be built, tested, clippy'd, and
  fmt-checked for real rather than reviewed by eye — which is how the two bugs above were actually
  caught.
- **`TwitterPublisher`'s OAuth 1.0a signer has not been exercised against a live X account** — no
  credentials are available in this environment. It's implemented from RFC 5849 and unit-tested
  against the RFC 2202 HMAC-SHA1 vector plus internal determinism/sensitivity checks (same inputs
  → same signature; changing any one input changes it), but end-to-end verification against the
  real API is explicitly called out as open in the RFC, not silently assumed correct.

### Decisions (alternatives considered, why this choice)

- **Not a `CompilerPass`/`Observer`.** Devlogs are release notes about the project, not enterprise
  knowledge to compile into the ledger — forcing this through `PassManager`/KIR/`Evidence` would
  be architecture cosplay. A plain CLI command matches what the feature does: read one file, call
  one LLM, call one HTTP API.
- **OAuth 1.0a over OAuth 2.0 Authorization Code + PKCE** for X — a browser consent flow and token
  refresh handling is disproportionate for a single-account bot; long-lived user-context tokens
  from the X Developer Portal are the established simpler path and the v2 endpoints still accept
  them.

---

## Knowledge Captured

- This repo's real devlogs do not follow `CLAUDE.md`'s own `## PR #N — <title>` template — see
  above. Anything parsing devlog structure going forward should target `## Summary` (universal)
  and treat other `## ` headings as free-form, not assume a fixed sub-heading convention.
- `ekos-recovery`'s `LlmProvider`/`LlmRequest`/`MockLlmProvider`/`strip_json_fences`/
  `AnthropicProvider`/`OllamaProvider`/`CachedLlmProvider` are all re-exported at the crate root
  (`ekos_recovery::*`), designed to be reused by other crates wanting an LLM call — `ekos-marketing`
  is the first crate outside `recovery`/`cli` to depend on it, and it worked without any changes
  needed to `ekos-recovery` itself.
- `recover.rs`'s `build_llm_provider` mock-fallback pattern is correct *for recovery* but is not a
  generic "no API key" fallback to reuse elsewhere — any new LLM-backed feature should ask whether
  it has a legitimate degraded mode before copying that pattern, or write its own clear-error
  version like `select_llm_provider` here.
- Percent-encoding for OAuth 1.0a must escape `!` in addition to the more obvious space/`"`/`#`/
  etc. — easy to miss since `!` isn't reserved in most other encoding contexts (e.g. it's fine
  unescaped in a URL fragment), but RFC 5849's unreserved set is strictly `A-Z a-z 0-9 - . _ ~`.
- This environment ships without any Rust toolchain by default; `rustup` install over network
  (`curl https://sh.rustup.rs | sh`, then `source "$HOME/.cargo/env"`) takes about two minutes and
  is a one-time cost per environment — worth doing early in any session that will write Rust, since
  it's the difference between reviewing code by eye and actually proving it works.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0027-marketing-agent.md` | New RFC |
| `CLAUDE.md` | (untouched this session — devlog rule already covers this workflow) |
| `ekos.toml` | Added commented `[marketing]`/`[marketing.twitter]` example |
| `ekos/Cargo.toml` | Added `crates/marketing` workspace member; `ekos-marketing`, `hmac`, `sha1`, `base64`, `percent-encoding`, `rand` workspace deps |
| `ekos/crates/marketing/**` | New crate: `devlog.rs`, `importance.rs`, `prompt.rs`, `tweet.rs`, `oauth1.rs`, `publisher.rs`, `store.rs`, `lib.rs` — 37 tests |
| `ekos/crates/compiler-core/src/config.rs` | Added `MarketingConfig`/`TwitterConfig` to `EkosConfig`, 2 tests |
| `ekos/crates/cli/src/commands/marketing.rs` | New `publish` command — devlog resolution, orchestration, approval prompt, logging; 5 tests |
| `ekos/crates/cli/src/commands/mod.rs` | Registered `marketing` module |
| `ekos/crates/cli/src/bin/ekos.rs` | Added `Commands::Marketing { Publish }` subcommand + dispatch |
| `ekos/crates/cli/Cargo.toml` | Added `ekos-marketing` dependency |
| `marketing/README.md`, `marketing/templates/tweet.md` | New — usage docs + reference template |

# RFC 0027 — Marketing Agent v1 (Automatic X Release Announcements)

**Status:** Accepted
**Author:** EKOS team (adapted from Alexey Banaev's design doc, `ekos-marketing-agent-plan.md`)
**Created:** 2026-08-04
**Gating:** none (additive; auxiliary tooling outside the core compiler pipeline — does not touch
the ledger, KIR, or any `Observer`/`CompilerPass`. Reuses RFC 0008's `LlmProvider` trait and RFC
0021's Ollama/Anthropic provider selection.)

---

## Motivation

EKOS ships real work every session — 28 devlogs and counting — but nothing tells the outside
world when it happens. Release communication today is manual: someone has to notice a devlog
landed, read it, and decide whether it's worth a post. That's the kind of repetitive,
judgment-light task EKOS's own thesis says an agent should do, and it's a natural first
"production-grade autonomous agent built with EKOS" (per the source design doc's long-term
vision) — small enough to ship in one session, useful enough to run for real.

The source design doc (`ekos-marketing-agent-plan.md`, provided by the project author) specifies
the v1 scope precisely: watch for a new `devlog_XXX.md`, draft one X (Twitter) post, get human
approval, publish, never post the same release twice. This RFC adopts that scope essentially
unchanged and adapts two things to this repository's actual conventions:

1. **The devlog format.** The design doc's example devlog (`## Added` / `## Changed` / `## Fixed`
   bullet lists) is not what this repo's devlogs look like — `CLAUDE.md`'s Devlog Rule mandates
   `Summary` / `PR #N` / `Knowledge Captured` / `Files Changed` sections (see `devlog_28.md` for a
   real example). The parser in this RFC targets the real format, not the illustrative one.
2. **Configuration.** The design doc proposes a standalone `marketing/config.yaml`. This
   introduces a second config format and a second config file into a codebase that already has
   exactly one (`ekos.toml`, TOML, `EkosConfig`, `deny_unknown_fields`) and one established pattern
   for opt-in features (`[llm]`, `[document-semantics]`). This RFC adds `[marketing]` to
   `ekos.toml` instead and keeps `marketing/` on disk for state only: `posted/tweets.json`,
   `logs/marketing.log`. This is the only material deviation from the source doc; everything else
   (scope, workflow, prompt rules, error handling, CLI shape) is preserved.

## Design

### Pipeline

```
ekos marketing publish [devlog|latest]
        │
        ▼
DevlogParser::parse           — deterministic, no LLM (devlog.rs)
        │
        ▼
classify_importance           — deterministic keyword heuristic (importance.rs)
        │                        LOW → stop, no tweet
        ▼
PostedStore::is_posted?        — dedup check against marketing/posted/tweets.json (store.rs)
        │                        already posted → stop
        ▼
generate_tweet                — one LlmProvider::complete call (prompt.rs + tweet.rs)
        │                        validated: ≤280 chars, mentions "EKOS", includes GitHub URL,
        │                        ≤3 hashtags; regenerated once on validation failure
        ▼
Human approval (Y/N/E)        — stdin prompt, or --yes/--dry-run flags (cli marketing.rs)
        │
        ▼
Publisher::publish            — TwitterPublisher (OAuth 1.0a, POST /2/tweets) or
        │                        NoopPublisher (dry-run/disabled) (publisher.rs)
        ▼
PostedStore::record + append marketing/logs/marketing.log
```

This mirrors the source doc's architecture diagram (`Developer → Claude Code → Marketing Skill →
Read devlog → Generate Tweet → Ask Approval → X API → Store Metadata`) with "Marketing Skill"
realized as a new `ekos-marketing` library crate plus an `ekos marketing publish` CLI command,
consistent with how every other capability in this repo is exposed (a crate + a `cli/commands/*`
wrapper), not a Claude-only skill with no underlying tested code. A thin Claude Code skill can
still shell out to this command later; the value (parsing, classification, validation, dedup,
publishing) lives in Rust so it's tested and works headlessly.

### `ekos-marketing` crate (`ekos/crates/marketing/`)

| Module | Responsibility |
|---|---|
| `devlog.rs` | `DevlogSummary { number, title, date, summary, section_titles }`; `parse(text) -> Result<DevlogSummary, DevlogParseError>`; `find_latest(dir) -> Option<PathBuf>` scans `devlog_*.md`, picks highest `N` |
| `importance.rs` | `Importance { Low, Medium, High }`; `classify(&DevlogSummary) -> Importance` |
| `prompt.rs` | Builds the exact system/user prompt from the source doc's "Tweet Prompt" section, parameterized by `MarketingConfig` (github URL, hashtags) |
| `tweet.rs` | `TweetDraft`; `generate_tweet(&dyn LlmProvider, &MarketingConfig, &DevlogSummary) -> Result<TweetDraft, MarketingError>`; `validate_tweet(&str, &MarketingConfig) -> Result<(), TweetValidationError>` |
| `oauth1.rs` | RFC 5849 OAuth 1.0a request signing (HMAC-SHA1) for the X API |
| `publisher.rs` | `Publisher` trait (`async fn publish(&self, text: &str) -> Result<String, PublishError>`); `TwitterPublisher`; `NoopPublisher` |
| `store.rs` | `PostedStore` — `marketing/posted/tweets.json` load/save, `is_posted`, `record` |

`ekos-marketing` depends on `ekos-recovery` for `LlmProvider`/`LlmRequest`/`MockLlmProvider` (RFC
0008) rather than redefining the LLM boundary — that trait and its Anthropic/Ollama backends
already exist and are exactly what this needs.

### Devlog parsing

Targets the real structure from `CLAUDE.md`'s Devlog Rule:

```markdown
# Devlog 28 — RFC 0025/0026: more document formats, ...
**Date:** 2026-08-03
**PRs:** worked on `main` ...
...
## Summary
<2–5 sentence overview>
...
## PR #N — <title>
### Problem / motivation
...
```

`DevlogSummary` captures: `number` (from the `# Devlog N` heading, falling back to the filename's
`devlog_N.md`), `title` (text after the em-dash on the `#` line), `date` (`**Date:**` line),
`summary` (the `## Summary` section body — the one section every real devlog has, verified across
all 28), and `section_titles` (every other `## `-level heading). A survey of all 28 existing
devlogs found `CLAUDE.md`'s own `## PR #N — <title>` template is aspirational, not actually
followed — real second-level headings are free-form (`## RFC 0025 — ...`, `## Phase 6 — ...`,
`## Bug 2 — ...`, `## Part 3 — ...`). Rather than parse a pattern the corpus doesn't use, the
parser collects *every* `## ` heading except the three that are universal meta-sections across
all 28 files — `Summary`, `Knowledge Captured`, `Files Changed` — into `section_titles`, giving
downstream classification/prompting real signal regardless of which convention a given devlog
happened to follow. Parsing never calls an LLM — it's plain Markdown section extraction,
deterministic and unit-tested against a real devlog fixture.

### Importance classification (deterministic, per source doc)

```
LOW    — title/summary/section-titles mention only doc/test/refactor/chore/typo-class words,
          and none of {"RFC", "feat", "add", "new", "implement"}  →  no tweet
MEDIUM — default: some feature-shaped change
HIGH   — title or a section title references an accepted RFC number, or an explicit
          "feat:"/"new capability" signal
```

This is a heuristic, not a claim of perfect judgment — it exists so the LOW case (a devlog that's
pure chores) short-circuits before any LLM call, matching the source doc's rule table exactly
("documentation only / tests / refactoring → no tweet"). In practice, this repo's own Devlog Rule
already filters out purely-minor sessions before a devlog is even written, so most real devlogs
will classify MEDIUM or HIGH; LOW mainly guards a `chore:`-only devlog someone writes anyway.

### Tweet generation

`prompt.rs` builds the system prompt verbatim from the source doc's "Tweet Prompt" section
(experienced DevRel voice, no hype, no clickbait, max one 🚀, ≤280 chars, focus on developer
value, never invent features, always mention EKOS, always include GitHub, ≤3 hashtags). The user
message is the devlog's `summary` + `section_titles`, never the full devlog (raw text can run to
10K+ words; the summary is written precisely to be the elevator pitch).

The LLM is asked for bare JSON (`{"tweet": "..."}`) — same contract as every other LLM-backed pass
in this repo (RFC 0008) — parsed through the existing `strip_json_fences` helper. `validate_tweet`
then checks the four hard constraints server-side (length, EKOS mention, GitHub URL, hashtag
count) rather than trusting the model to have followed the prompt; on failure, `generate_tweet`
retries once with the validation failure appended to the user message ("Tweet too long →
Regenerate", per the source doc's error table), then gives up with `MarketingError`.

### Human approval

The CLI prints the same preview block as the source doc:

```
Tweet Preview
---------------------------------
<tweet text>
---------------------------------
Approve? [Y]es / [N]o / [E]dit
```

`ekos marketing publish` reads one line from stdin by default. `--yes` skips the prompt (approve
as-drafted — needed for any non-interactive/cron use, which the source doc doesn't rule out for
later versions but v1's default remains human-gated exactly as specified: "No automatic
publishing."). `--dry-run` forces a `NoopPublisher` regardless of `[marketing.twitter] enabled`,
for rehearsal without touching X or `posted/tweets.json`... actually `--dry-run` still updates
`posted/tweets.json`-adjacent logs but does **not** record a tweet id or mark the devlog posted,
so a real run afterward is not blocked by dedup.

### Publishing

```rust
#[async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, text: &str) -> Result<String, PublishError>; // -> tweet id
}
```

`TwitterPublisher::new(api_key, api_secret, access_token, access_secret)` signs `POST
https://api.twitter.com/2/tweets` with OAuth 1.0a user-context (the standard approach for
posting-as-a-user without a browser OAuth2 flow; X's v2 endpoints still accept OAuth 1.0a).
Credentials come from `TWITTER_API_KEY` / `TWITTER_API_SECRET` / `TWITTER_ACCESS_TOKEN` /
`TWITTER_ACCESS_SECRET` env vars, mirroring how `ANTHROPIC_API_KEY` is read elsewhere — never
committed, never logged. `oauth1.rs` implements RFC 5849 signature-base-string construction and
HMAC-SHA1 signing from spec; it is unit-tested against the RFC 2202 HMAC-SHA1 test vector and for
internal determinism/sensitivity (same inputs → same signature; changing any one input changes
the signature), but **has not been exercised against a live X account** — no credentials are
available in this environment. This is called out explicitly as an open item below, not silently
assumed correct.

`NoopPublisher` (used when `[marketing.twitter] enabled = false` or `--dry-run`) prints what would
be posted and returns a fake `dry-run-<uuid>` id, never touching the network.

### Duplicate detection

`marketing/posted/tweets.json`:

```json
[
  { "devlog": "028", "tweet_id": "19381283712", "date": "2026-08-04", "feature": "RFC 0025/0026: document formats + semantics" }
]
```

`PostedStore::is_posted(devlog_number)` is checked immediately after parsing, before any LLM call
— an already-posted devlog costs nothing to re-run against.

### Configuration (`ekos.toml`, deviation from the source doc — see Motivation)

```toml
[marketing]
github = "https://github.com/alexeyban/EKOS"   # default if omitted
hashtags = ["Rust", "AI", "MCP"]                # default if omitted

[marketing.twitter]
enabled = false     # default: no posting until explicitly turned on
dry-run = false
```

Added to `EkosConfig` as `MarketingConfig` / `TwitterConfig`, following the exact
`#[serde(default)]` + `deny_unknown_fields`-compatible pattern `DocumentSemanticsConfig` already
uses.

### CLI

```
ekos marketing publish [DEVLOG]   # DEVLOG: path, bare number ("28"), or omitted = latest devlog_*.md
    --yes         # skip interactive approval, publish as drafted
    --dry-run     # never call the real Publisher or record a posted entry
```

### Logging

Every run appends one line per step to `marketing/logs/marketing.log` (plain text, matching the
source doc's example exactly), in addition to the normal `tracing` output every other `ekos`
subcommand produces.

## Alternatives Considered

- **`marketing/config.yaml` as specified.** Rejected — see Motivation. Would require a new
  `serde_yaml` dependency and a second config-loading code path for one feature.
- **Modeling this as a `CompilerPass`/`Observer`.** Rejected — devlogs are release notes about the
  project, not enterprise knowledge to compile into the ledger. Forcing this through
  `PassManager`/`KIR`/`Evidence` would be architecture cosplay, not a fit; a plain CLI command
  matches what the feature actually does (read one file, call one LLM, call one HTTP API).
- **OAuth 2.0 Authorization Code + PKCE for X.** Rejected for v1 — requires an interactive browser
  consent flow and token refresh handling, disproportionate for a single-account bot; OAuth 1.0a
  user-context tokens (generated once in the X Developer Portal) are the established simpler path
  and remain supported by the v2 endpoints.

## Open Questions

- [x] Config file format/location — resolved: `[marketing]` in `ekos.toml`, not a separate YAML file.
- [x] Devlog format the parser targets — resolved: this repo's real `Summary`/`PR #N` structure.
- [ ] `TwitterPublisher` has not been run against a live X account (no credentials available in
      this environment) — the OAuth 1.0a signer is spec-implemented and unit-tested in isolation,
      but end-to-end verification against the real API is deferred to whoever holds
      `TWITTER_*` credentials.
- [ ] Importance-classification heuristic is a first cut; if it proves too coarse in practice
      (e.g. misclassifies a real HIGH devlog as MEDIUM), a v1.1 could route the classification
      decision through the LLM instead of a keyword heuristic.

## Acceptance Criteria

- [x] `ekos marketing publish <devlog>` runs end-to-end against a real devlog in this repo,
      producing a validated tweet draft, an approval prompt, and (in `--dry-run`) a printed
      preview with no network call.
- [x] Re-running against an already-posted devlog is a no-op (`Skip`, per the source doc's error
      table).
- [x] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` all
      pass with the new crate included.
- [x] Design is consistent with `ekos.md`'s compiler architecture (this RFC explicitly documents
      why the feature sits *outside* the compiler pipeline rather than forcing a fit).

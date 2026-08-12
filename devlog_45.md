# Devlog 45 — RFC 0045: hosted demo server, a two-repo peer-validation MVP

**Date:** 2026-08-12
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

The user's ask was strategic, not a bug report: narrow EKOS down to one very painful task and ship
a server-side MVP for it, aimed at getting 3–5 of ~20 peer architects/senior engineers to say "I
want to run my repo through this" after a 5–10 minute demo. Grounded the pitch in the project's own
words — devlog_44's framing of the context-window-ceiling problem — and in real code, not
assumption: two research passes over `docs-gen`, `AiRuntime::ask`, the MCP server, and the
workspace's dependency graph confirmed exactly what existed (curated zero-LLM docs, evidence-cited
`ask`) and what didn't (no HTTP server anywhere in the workspace, `ask` always makes a live LLM
call with no no-LLM mode). RFC 0045 built a small, fixed two-repo hosted demo server on top of that
— reusing `AiRuntime::ask` and its evidence-mapping pattern unmodified rather than duplicating
logic. A repo-selection spike, run against real candidates, surfaced a genuine product finding
along the way: two of three well-known Rust OSS repos tried (`ripgrep`, `bat`) hit the exact
identity-over-merge failure class CLAUDE.md already documents (`pcre2`/`ignore`/`bat` name
collisions vs. `Technology`/`Crate`), while `fd` baked clean. `fd` became the demo's second repo.

---

## RFC 0045 — Hosted Demo Server

### Problem / motivation

The painful task, in the user's own earlier words (devlog_44): *"Claude can reverse-engineer a
codebase into documentation, but hits its own context-window ceiling on extra-huge projects and on
many projects at once."* EKOS already addresses this — `docs-gen --layout curated`
(RFC 0035/0037/0042/0044) compiles README/Architecture/API pages once, deterministically, zero LLM
calls; `AiRuntime::ask` answers point questions with evidence citations instead of an unverified
guess — but nothing let a peer *see* either without already running the EKOS CLI locally. Confirmed
by direct code read: no HTTP server framework anywhere in the workspace (only `tokio`/`reqwest` as
an HTTP *client*); `AiRuntime::ask` always makes a live Anthropic call with no ledger-only mode,
and silently degrades to `MockLlmProvider`'s canned empty response if the API key is missing —
acceptable for an internal recovery pass, an embarrassing live-demo failure otherwise;
`docs-gen --layout curated --format html` is an explicit open item (`docs.rs:88` errors on that
combination) — curated output is Markdown-only today.

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0045-hosted-demo-server.md` (new) |
| New crate | `ekos/crates/demo-server/` (new workspace member) |
| Axum HTTP server | `demo-server/src/main.rs` — static docs + `POST /ask`, boot-time key check, per-IP rate limit |
| `/ask` adapter | `demo-server/src/ask.rs` — thin wrapper around `AiRuntime::ask`, unmodified |
| Bake-time Markdown→HTML | `demo-server/src/bin/prerender.rs` (new `prerender` binary, `pulldown-cmark`) |
| Repo catalog | `demo-server/src/catalog.rs` — fixed two-repo TOML catalog, not general self-serve ingestion |
| Visibility widening | `cli/src/commands/recover.rs::build_llm_provider`, `cli/src/commands/ask.rs::ai_config` — `pub(crate)` → `pub`, reused by `demo-server` rather than duplicated |

**Reuse over reimplementation**: the RFC's design goal — "`/ask` is a thin adapter, not new business
logic" — held up under implementation. `ask.rs::answer_question` calls the exact same
`build_llm_provider`/`ai_config`/`open_store`/`AiRuntime::ask` chain `ekos ask` already uses,
just serialized to JSON instead of printed. The only new dependency chain was HTTP itself
(`axum` + `tower-http`, one addition to the workspace's otherwise-client-only HTTP surface) and
Markdown rendering (`pulldown-cmark`, bake-time only, never on the live request path).

**A real `Send`-bound wall, found by building it, not by design review**: `handle_ask` initially
failed to compile as an axum handler — `dyn KnowledgeStore` (and `Runtime<'a>`, which borrows it)
isn't `Sync`, because its only prior consumers (the CLI's single-threaded execution, the MCP
server's one-message-at-a-time stdio loop) never needed it to be. Axum requires handler futures to
be `Send`, which fails the moment non-`Sync` state is held across an `.await`. Retrofitting
`Send`/`Sync` across the ledger/runtime stack was out of this RFC's scope, so each `/ask` request
instead runs on its own `tokio::task::spawn_blocking` thread with its own throwaway
single-threaded runtime — the non-`Send` state never crosses an axum-visible await boundary. A
narrow, request-scoped adapter-layer fix, not a change to shared infrastructure.

**Boot-time guardrail, verified, not just described**: ran the compiled `demo-server` binary with
`ANTHROPIC_API_KEY` unset — it printed a clear refusal and exited 1, rather than starting and
serving `MockLlmProvider`'s blank answers. Confirmed the same binary with a placeholder key boots
cleanly, serves both repos' pre-rendered static docs (HTTP 200), returns a clean 404 JSON error for
an unknown `?repo=` slug, and — after 10 requests from one IP within the 60-second window — starts
returning 429 exactly as designed. (Caveat worth recording honestly: `AnthropicProvider::from_env_var`
only checks that the env var is *present*, not that the key is *valid* — an invalid-but-present key
still only surfaces as a request-time error, not a boot-time one. Not a false claim, just a scope
boundary worth knowing next time this comes up.)

### Repo-selection spike — a real product finding, not just a demo-prep task

Timed dry-run baked three well-known small Rust CLI repos (`ekos init/build/recover/resolve/
compile/commit`, real binary, each `observe_paths`-scoped to its own clone):

| Repo | Bake time | Resolve conflicts |
|---|---|---|
| `sharkdp/fd` | ~2.4s | **0** |
| `BurntSushi/ripgrep` | ~15s | **4** — `pcre2`, `serde json`, `ignore` flagged as multiple kinds (`RustSymbol`/`Technology`/`Crate`) |
| `sharkdp/bat` | ~26s | **3**, plus a UTF-8 decode warning on a test-snapshot fixture and a SQL Transformation IR only 1% mapped (test fixtures, not real SQL) |

`ripgrep` and `bat` both hit the exact identity-over-merge failure class CLAUDE.md already
documents for `Section`/`TransformNode`/`RustSymbol`/`RustModule`/`Crate` — common-word crate names
(`ignore`, `bat` itself) colliding with the same-kind structural-score fallback. Real-data testing
against a repo EKOS had never seen before caught it again, the same way every prior instance was
caught: not by inspection. `fd` was chosen for the demo specifically because it was clean —
reliability over marginal name-recognition for a demo that must not hit a rough edge in front of a
peer — but `ripgrep`'s conflicts are logged as a genuine follow-up candidate for `identity`'s
resolver, independent of this MVP.

### Decisions (alternatives considered, why this choice)

- **A fixed two-repo catalog, not general self-serve ingestion** — the RFC's explicit Non-goal.
  Rejected general ingestion for this pass because it reopens `devlog_12`'s `odoo/odoo`
  ~40,000-file unfiltered-checkout stall as a *general* problem instead of a scoped one, and adds
  real isolation/auth/quota surface a 5–10 minute peer demo doesn't need to prove the core claim.
- **axum over hand-rolled `tokio::net`** — the standard, well-supported minimal choice for "few
  routes, JSON in/out, static file serving," given `tokio` was already a workspace dependency and
  nothing else pulled in an HTTP framework.
- **Bake-time Markdown→HTML pre-render over fixing `docs-gen`'s `--layout curated --format html`
  gap** — only two fixed repos need HTML output once; fixing the general renderer is real, separate
  scope this RFC deliberately left alone (`docs.rs:88`'s existing error message stands).
- **Keep the real LLM-backed `/ask` path over building a no-LLM/ledger-only mode** — a no-LLM mode
  would sidestep the cost/availability guardrails entirely, but building it is separate, real scoped
  work, and the existing path is a strictly better demo (natural-language cited answers, not a raw
  fact dump) as long as the guardrails hold.

---

## Knowledge Captured

- **`Router<S>` type inference across a loop-based builder chain works fine in axum 0.7** — the
  earlier `Handler<_, _>` compile error was not about that pattern; it was a genuine `Send`-bound
  failure from holding `Runtime<'a>` (which borrows `dyn KnowledgeStore`, not `Sync`) across an
  `.await`. `#[axum::debug_handler]` (needs the `macros` feature) turns axum's otherwise-opaque
  `Handler` trait errors into the real underlying bound failure — reach for it first next time an
  axum handler won't compile, rather than guessing at extractor ordering.
- **`ConnectInfo<SocketAddr>` needs `into_make_service_with_connect_info::<SocketAddr>()` at
  `axum::serve`, not just the extractor in the handler signature** — compiles either way, but
  panics at request time if the make-service wrapper is missing.
- **`AnthropicProvider::from_env_var` only checks presence, not validity** — a real network-validated
  key check would need an actual API round-trip; documented as a known boundary rather than
  discovered live.
- **Real-data repo testing keeps finding the same identity-over-merge bug class** — this is now the
  fourth+ time (`Section`/`TransformNode`/`RustSymbol`/`RustModule`/`Crate`, now also flagged live
  against `ripgrep`/`bat`) the same root cause (name-prefix similarity + same-kind structural-score
  fallback of 1.0) has surfaced only by running the pipeline against real, previously-unseen data —
  never by code inspection or unit tests. Worth treating as a standing signal that `identity`'s
  resolver needs a structural fix, not another one-off exclusion-list entry, next time this comes up.
- **`tokio::task::spawn_blocking` + a throwaway `Builder::new_current_thread()` runtime is a clean,
  scoped way to bridge a `Send`-constrained async caller (axum) to non-`Send` synchronous-by-design
  internals** without retrofitting thread-safety across shared infrastructure the caller doesn't own.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0045-hosted-demo-server.md` | New RFC |
| `ekos/Cargo.toml` | New `crates/demo-server` workspace member; `axum`, `tower-http`, `pulldown-cmark` added; `ekos` (cli) added as a path dependency for reuse |
| `ekos/crates/demo-server/` | New crate: `main.rs` (axum server), `ask.rs` (thin `/ask` adapter), `catalog.rs` (fixed two-repo catalog + tests), `page.rs` (shared CSS/HTML chrome), `src/bin/prerender.rs` (bake-time Markdown→HTML) |
| `ekos/crates/cli/src/commands/recover.rs` | `build_llm_provider`: `pub(crate)` → `pub`, reused by `demo-server` |
| `ekos/crates/cli/src/commands/ask.rs` | `ai_config`: `pub(crate)` → `pub`, reused by `demo-server` |
| `think-about-how-crispy-lake.md` (plan) | Repo-selection spike results recorded alongside the approved plan |

## Still open (tracked, not silently dropped)

- **Live-question pre-vetting against a real Anthropic key** (RFC 0045 Acceptance Criteria) — no
  `ANTHROPIC_API_KEY` was available in this session's environment. Every other guardrail (routing,
  evidence-mapping code path, boot-time key check, rate limiting, error handling) was verified
  end-to-end with a placeholder key; only actual answer *content* is unverified. Needs a real key on
  whatever machine hosts the demo before rehearsal.
- **Full 5–10 minute demo rehearsal against someone unfamiliar with EKOS** — blocked on the above,
  and needs a live human, not something to simulate.
- **`ripgrep`/`bat`'s identity-conflict finding** — not fixed this session (out of RFC 0045's
  scope), logged here and worth a `TODO.md` line as a candidate follow-up for `identity`'s resolver.

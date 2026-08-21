# RFC 0045 — Hosted Demo Server (read-only, two-repo MVP)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-12

---

## Motivation

The user's framing: narrow EKOS down to one very painful task and build a server-side MVP for it,
such that at least 3–5 of ~20 architects/senior engineers shown a 5–10 minute demo say "I want to
run my repository through this." The painful task, in the user's own earlier words (devlog_44):
*"Claude can reverse-engineer a codebase into documentation, but hits its own context-window
ceiling on extra-huge projects and on many projects at once."* EKOS already addresses this
structurally — `docs-gen`'s curated layout (RFC 0035/0037/0042/0044) compiles
README/Architecture/API/SequenceDiagrams pages **once, deterministically, with zero LLM calls**,
and `AiRuntime::ask` (`crates/runtime/src/ai.rs:98`) answers point questions with evidence
citations instead of an unverified LLM guess. Both exist and both work today. What doesn't exist is
any way to *show* this to someone who isn't already running the EKOS CLI locally.

Two research passes over the current codebase established what's real versus what's a genuine gap,
so this RFC builds on fact, not assumption:

**Already real and working:**
- `docs-gen --layout curated` (RFC 0042/0044) — confirmed by direct inspection of a real, committed
  output at `/home/legion/PycharmProjects/EKOS/doc/` (2,229 files), generated from EKOS's own
  repo: 39 crates, 2,192 files, 1,324 Rust symbols, 46 rollups, real contributor stats. This
  artifact requires **no new work** to use as a demo asset.
- `AiRuntime::ask` (`ai.rs:98`) — a working retrieve→expand→ground→answer pipeline
  (`find_objects` → `load_neighborhood` → `reconstruct_state`, `ai.rs:130-158`) that returns a
  structured `AiAnswer { answer, evidence_refs: Vec<KirId>, diagnostics }` (`ai.rs:62`).
  `crates/cli/src/commands/ask.rs:29` shows the existing pattern for resolving `evidence_refs` into
  displayable `path`/`fragment`/`confidence` via `ledger.get_evidence(id)`.
- `tokio` (full features) and `reqwest` (HTTP client, rustls) are already workspace dependencies
  (`ekos/Cargo.toml`, pulled into `crates/cli/Cargo.toml`).

**Confirmed gaps this RFC exists to close:**
- **No HTTP server anywhere in the workspace.** `ekos mcp serve` (`crates/cli/src/commands/mcp.rs`)
  is a blocking stdio JSON-RPC loop for AI-agent consumption, not a web-reachable surface. A `grep`
  across every `Cargo.toml` in the workspace for axum/hyper/warp/actix returns nothing.
- **`AiRuntime::ask` always makes a live LLM call** — there is no ledger-only/no-LLM citation mode.
  Without a configured `ANTHROPIC_API_KEY`, `build_llm_provider` (`recover.rs:766`) silently falls
  back to `MockLlmProvider`, which returns a canned empty response. Silent degradation is acceptable
  for an internal recovery pass; it is not acceptable for a public-facing demo endpoint, where it
  would read as a broken product.
- **`docs-gen --layout curated` output is Markdown-only.** `crates/cli/src/commands/docs.rs:88`
  explicitly errors if `--format html` is combined with `--layout curated` ("HTML curated output is
  an open item"). `--layout objects --format html` *is* fully self-contained HTML
  (`docs-gen/src/lib.rs:785`, `render_html_object_page`, inline CSS, zero external assets), but
  that's the older, flatter RFC 0035 per-object page style, not the newer curated narrative that is
  the more impressive, most differentiated output.
- **A precedent failure mode for public-repo ingestion**: `devlog_12.md` records that checking out
  the full, unfiltered `odoo/odoo` monorepo (~40,000 files) stalled `ekos build`. Any external repo
  this server points at must be scoped, not handed to the pipeline unfiltered.

## Scope

1. A small, fixed-catalog hosted demo service: two pre-baked repos, static curated-docs browsing,
   and one read-only `/ask` endpoint per repo.
2. A one-time, offline "bake" step (pipeline run + Markdown→HTML pre-render) for the one repo that
   doesn't already have output committed (EKOS-self's `doc/` already exists and needs no bake).
3. A thin, new HTTP server crate/subcommand — static file serving plus a single `/ask` adapter
   around the existing `AiRuntime::ask`, with startup-time and request-time guardrails specific to
   exposing a live-LLM endpoint publicly.
4. A minimal single-page frontend: pick one of two repos, browse pre-rendered docs, or ask a
   question and see the cited answer.

## Non-goals (this pass)

- **General multi-tenant / self-serve ingestion.** Visitors cannot point this server at an arbitrary
  repo of their choosing — the catalog is fixed at two pre-baked repos. Arbitrary-repo self-serve
  ingestion is a natural, larger follow-up (and would need to solve the `odoo`-style unfiltered-scan
  stall generally, plus auth/quota/isolation), not this MVP's job.
- **Auth, accounts, or any write path.** The server is read-only end to end, consistent with the
  existing invariant that the Runtime never mutates the ledger — same posture as `ekos mcp serve`
  minus its one write tool (`ekos_identity_review`, not exposed here at all).
- **A no-LLM/ledger-only answer mode for `/ask`.** Building one is real, scoped work (bypassing
  `AiRuntime::ask`'s LLM call entirely and returning raw neighborhood facts) that this RFC
  deliberately does not attempt; the guardrails below (Design, "Live-LLM-endpoint guardrails")
  exist specifically because this pass keeps the real LLM-backed path instead.

_Both general self-serve ingestion and the no-LLM answer mode are tracked as backlog: see
`TODO.md` → "Promoted from RFC Non-Goals" → "Demo server". (This RFC also reaffirms RFC 0037's
still-open "no HTML output for curated docs" non-goal — same tracked item.)_
- **Server-side Markdown rendering.** Curated Markdown is rendered to static HTML once, offline, as
  part of the bake step — the live server never parses Markdown on a request path.
- **General HTML support in `docs-gen --layout curated`** (the `docs.rs:88` gap noted above) — out
  of scope for `docs-gen` itself; this RFC works around it at the bake-script layer instead of
  fixing the underlying renderer, since only two fixed repos need it once, not a general feature.

## Design

### Repo catalog

Two repos, fixed at build time, each playing a different role in the demo:

1. **EKOS itself.** Its curated docs already exist at `doc/` — no bake needed for the docs side. Its
   ledger (`.ekos/` from EKOS's own last `commit`) is opened read-only for its `/ask` slot.
   Framing: depth/scale proof point ("this explains its own 2,000+ file production codebase, once,
   with zero LLM calls"). Known, honestly-scoped caveat to carry into any copy shown alongside it:
   the CI/CD analyzer (RFC 0042) only covers GitHub Actions workflows (EKOS's repo has 2 pipelines,
   no Docker/K8s/Terraform, because EKOS's own repo has none) — this is what was compiled, not a
   general infra-coverage claim.
2. **One external, small/bounded, well-known OSS repo** — the trust proof point, for the live `/ask`
   question, where an audience member can sanity-check output against knowledge they already have.
   Selection constraints, each grounded in a confirmed analyzer limit or failure mode (not a
   preference):
   - Must be `observe_paths`-scoped in its `ekos.toml`, never handed to `ekos build` as a full
     unfiltered monorepo checkout (the `odoo/odoo` ~40,000-file stall from `devlog_12.md` is the
     concrete failure mode being avoided).
   - Should avoid heavily macro-driven Rust: `rust_analyzer`'s `Calls` edges only resolve
     same-file, and `syn` never expands macros (confirmed by the analyzer's own test
     `call_inside_macro_invocation_is_not_recorded`) — a macro-heavy codebase would visibly
     under-report its own call graph live, in front of the one audience able to notice.
   - If Python is chosen instead of Rust for this slot, it should be genuinely PySpark-flavored if
     the demo intends to show DataFrame-chain recovery (RFC 0038/0040) — `python_analyzer`'s chain
     tracing never crosses function/file boundaries and `spark.sql(f"...")` is always `Unmapped` by
     design, so a non-PySpark Python repo would only exercise plain AST/symbol extraction.
   - Final selection is a short, separate spike (timed dry-run bake, not a design decision this RFC
     needs to pre-commit).

### Bake step (offline, one-time per repo added to the catalog)

A small standalone script/binary (not part of the live server's request path):

1. Run the existing, unmodified pipeline: `ekos init && ekos build && ekos recover && ekos resolve
   && ekos compile && ekos commit` against the external repo's `observe_paths`-scoped workspace.
   `devlog_14.md`'s real numbers (a ~40-project/5GB estate: `build` 6m01s, `recover` 20m50s cold —
   5s on a cached re-run — `compile` 0.7s, `commit` 48s) confirm `recover` dominates and must never
   run on a request path; a single modest repo will be faster but the "bake ahead, serve instantly"
   shape is non-negotiable regardless of size.
2. Run `ekos docs generate --layout curated` (unmodified) to produce the repo's
   README/Architecture/API/SequenceDiagrams + entity Markdown pages.
3. Pre-render that Markdown to static HTML once, using a small markdown-to-HTML crate (e.g.
   `pulldown-cmark`), styled to match `docs-gen`'s existing embedded-CSS look
   (`docs-gen/src/lib.rs:785`'s `EMBEDDED_CSS`/`html_document`) for visual consistency between the
   pre-baked pages and anything the live server renders itself.
4. Output: a static HTML directory per repo, plus the repo's `.ekos/` ledger directory, both
   read-only inputs to the live server below.

EKOS-self skips steps 1–3 entirely (output already exists at `doc/`); only step 3's HTML
pre-render needs running once against its existing Markdown, plus pointing at EKOS's own existing
`.ekos/` ledger for its `/ask` slot.

### Live server (new: `ekos/crates/demo-server`, or a `demo serve` subcommand under `cli`)

**axum** on the existing `tokio` workspace dependency — the natural minimal choice, since `tokio`
is already present and nothing else in the workspace pulls in an HTTP framework. Two
responsibilities, both read-only:

- Serve the pre-rendered static HTML per repo (no templating, no request-time Markdown parsing).
- `POST /ask?repo=<slug>` — a thin adapter, not new business logic: open the selected repo's
  `.ekos/` ledger read-only, call `AiRuntime::ask(question)` unmodified, map `evidence_refs` into
  displayable citations using the exact pattern `ask.rs:29` already implements, serialize to JSON.

### Live-LLM-endpoint guardrails (new — the reason this isn't a bare adapter)

Exposing `AiRuntime::ask` behind a public URL, unlike its existing CLI/MCP callers, means:

- **Fail loudly at boot, not softly at request time.** If `ANTHROPIC_API_KEY` (or `[llm]`'s
  configured equivalent) is unset or invalid, the server must refuse to start rather than silently
  serving `MockLlmProvider`'s canned empty answers — a missing key must never surface as a live
  demo's answer box going blank in front of a peer.
- **Rate limiting**, since each novel question is a real, billed Anthropic API call. A simple
  per-IP/per-session cap is sufficient for a ~20-person peer demo; this is not a general
  DoS-hardening exercise.
- **A pre-vetted question list per repo**, run and confirmed (non-empty, sensible `evidence_refs`)
  before demo day, while still allowing one genuine free-form question live per session —
  `CachedLlmProvider` (`crates/recovery/src/cache.rs`, SHA-256 on model+prompt+system+user) already
  absorbs repeat identical questions across visitors at no extra cost.

### Minimal frontend

One page: a two-option repo picker, a static-doc browser (iframe or simple router over the
pre-rendered HTML), and an ask box that calls `/ask` and renders the answer with its citations
inline. No new design system — visually match `docs-gen`'s existing embedded CSS so the pre-baked
pages and the live chrome around them feel like one product.

## Alternatives Considered

- **General self-serve ingestion (visitor pastes any repo URL) as the MVP** — rejected for this
  pass. It reopens the `odoo`-style unfiltered-scan stall as a *general* problem instead of a
  scoped one, and adds real isolation/auth/quota surface that a 5–10 minute peer demo doesn't need
  to prove the core claim. A fixed two-repo catalog proves the same painful-task story with far
  less new surface; self-serve is the natural next RFC if the peer reaction validates further
  investment.
- **A no-LLM, ledger-only `/ask` mode** — considered as a way to sidestep the cost/availability
  guardrails entirely, rejected for this pass because it's separate, real scoped work (bypassing
  `AiRuntime::ask`'s generation step and hand-formatting raw neighborhood facts) and the existing
  LLM-backed path is a strictly better demo of the actual product (natural-language answers with
  citations, not a raw fact dump) as long as the guardrails above are respected. Worth revisiting if
  live-LLM cost/latency turns out to be a real obstacle in rehearsal.
- **Hand-rolling the HTTP layer directly on `tokio::net::TcpListener`** instead of adding axum —
  rejected; axum is the standard, well-supported minimal choice for exactly this shape of
  "few routes, JSON in/out, static file serving" server, and the marginal dependency-surface cost
  of one well-maintained crate is smaller than the correctness cost of hand-rolling HTTP parsing.
- **Serving `--layout objects --format html` instead of pre-rendering curated Markdown** — rejected;
  it's already self-contained HTML with zero new code, but it's the older, flatter RFC 0035 style,
  not the newer curated narrative (README/Architecture/API) that is the more differentiated,
  most-recently-shipped output and the stronger demo asset.

## Testing

- New crate/subcommand: unit tests for the `/ask` adapter's evidence-mapping logic (mirrors the
  existing `ask.rs:29` pattern) against a small fixture ledger.
- Boot test: server started with `ANTHROPIC_API_KEY` unset must exit with a startup error, not serve
  requests.
- Bake-script dry run for the external repo: confirm it completes without attempting to walk an
  unfiltered large monorepo (the concrete `devlog_12.md` failure mode) and produces both a valid
  `.ekos/` ledger and non-empty pre-rendered HTML output.
- Manual: every question on the pre-vetted list, against both repos' baked ledgers, returns
  non-empty `evidence_refs` that resolve to real, sensible citations via `/ask`.
- Full workspace: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --check` from `ekos/`, matching every prior RFC in this project.

## Acceptance Criteria

- [ ] EKOS-self's pre-existing `doc/` output is served as static HTML from the live server with no
      loss of content versus the committed Markdown.
- [ ] The external repo is baked end-to-end (`init` through `commit`, then `docs generate
      --layout curated`) via the offline bake script, `observe_paths`-scoped, with a recorded wall-clock
      time for the dry run.
- [ ] `/ask` returns cited answers for both repos, sourced from `AiRuntime::ask` unmodified.
- [ ] Server refuses to start without a valid `ANTHROPIC_API_KEY` rather than degrading silently.
- [ ] Rate limiting is in place and does not block the intended single-session demo flow.
- [ ] Full 5–10 minute demo script (per the accompanying plan) rehearsed end-to-end against someone
      unfamiliar with EKOS, timed under 10 minutes including one live free-form question.

## Files Changed (planned)

| File | Change |
|---|---|
| `ekos/docs/rfcs/0045-hosted-demo-server.md` | This RFC |
| `ekos/crates/demo-server/` (new crate) or `ekos/crates/cli/src/commands/demo.rs` (new subcommand) | axum HTTP server: static file serving + `POST /ask` adapter, startup key check, rate limiting |
| `ekos/crates/demo-server/src/bake.rs` (or a standalone script outside the workspace) | Offline bake step: pipeline run + `docs generate --layout curated` + Markdown→HTML pre-render for the external repo |
| `ekos/Cargo.toml` | New workspace member (if a separate crate) + `axum`, `pulldown-cmark` (or equivalent) added to workspace deps |
| Minimal frontend assets (served as static files, path TBD alongside the new crate) | Repo picker, doc browser, ask box |

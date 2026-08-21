# Devlog 60 — Proving the core loop, cold, on the whole `analytics/` repo

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Prior sessions (devlog 56-59) proved EKOS's ClickHouse dialect parsing against `analytics/`
(Plausible Analytics, a real, unmodified AGPL-3.0 repo) — one file, one component. This session
went further: a genuinely cold `init → build → recover → resolve → compile → commit` run over the
*whole* 2,045-file repo, timed stage by stage, followed by a real `ekos ask` + MCP question set
graded against ground truth read directly from the repo. Total cold pipeline time: **~107 seconds**
for 2,045 files → 1,529 compiled objects, 3,977 relationships, 2,075 evidence records. The run
surfaced three new, previously-unknown gaps in one sitting, all reported honestly rather than
patched or hidden: a Postgres `sqlparser` parse failure (separate from the already-fixed ClickHouse
one), identity resolution over-merging real people and unrelated documents (not just ClickHouse
tables, as devlog_59 first found), and a retrieval-brittleness bug in `ekos ask` itself. Published
as `docs/presentations/analytics-full-loop.html`, backed by 19 real, unedited transcript files.

---

## What was done

- Backed up `analytics/ekos.toml`, moved the existing `.ekos/` workspace aside (non-destructive —
  `mv`, not `rm`, so the prior session's cache stays recoverable), and ran `ekos init` fresh, to get
  a genuinely cold timing number rather than reusing a same-day warm cache (an earlier attempt at
  `ekos build` skipped 2021/2022 files as unchanged — 11.6s, not a real first-run number).
- Timed every pipeline stage individually (`build`: 33.6s, `recover`: 0.69s, `resolve`: 0.11s,
  `compile`: 0.19s, `commit`: 71.3s) against the full repo tree, not just its SQL files.
- Ran `ekos identity scan` (RFC 0029) for the first time against this workspace: 3,645 objects
  scanned, 32 cross-system candidates found.
- Designed and ran a 7-question `ekos ask` set spanning ClickHouse, Postgres, git history,
  identity-resolution ground truth, README content, and Elixir business logic — graded each answer
  against ground truth read directly from `analytics/`'s real source, not assumed.
- Drove three real MCP stdio JSON-RPC sessions: `ekos_search`/`ekos_state`/`ekos_neighborhood`
  reproducing the CLI's findings over the protocol an agent host actually uses; `ekos_ekl` for a
  structured aggregate query; and `ekos_identity_review` — the one write-capable MCP tool —
  confirming/rejecting a real cross-system candidate live, writing a real ledger Event.
- Published `docs/presentations/analytics-full-loop.html` (9 slides, following this repo's existing
  hero → numbered-stage → stat-grid → honest-gap convention), backed by 19 raw transcript files
  under `docs/presentations/examples/analytics-full-loop/`, and wired it into all three existing
  link points (`docs/presentations.html`, `docs/index.html`'s announce-grid, `README.md`).

---

## Finding 1: a second, previously-unknown SQL parser gap — Postgres, not ClickHouse

`priv/repo/structure.sql` (the actual Postgres application schema — `sites`, `api_keys`, and every
other core table) fails whole-file: `sql parser error: Expected: end of statement, found: INCREMENT
at Line: 116` — an `IDENTITY ... INCREMENT BY` clause `sqlparser`'s Postgres dialect doesn't
support. `sql-analyzer` falls back to an empty graph; `sql-transform-analyzer` degrades to
per-statement recovery and maps just 1 of 1,282 statements (0.078% coverage). This is unrelated to
RFC 0057/0058 — those RFCs only ever touched the ClickHouse dialect crate — and was not previously
known. **Right now EKOS has zero structured knowledge of this real repo's actual Postgres schema.**
Not fixed this session; reported and tracked (see TODO.md), consistent with this repo's convention
of naming gaps rather than silently patching or hiding them.

## Finding 2: identity over-merging generalizes past ClickHouse `Table`s — to `Person` and `Document`

devlog_59 found `crates/identity`'s `DefaultResolver` merging 6 real ClickHouse `imported_*` tables
into one identity. This session's whole-repo run shows the same 0.85-threshold mechanism hits every
object kind it compares, not just `Table`:

- **A real contributor erased.** Merge proposal #7 combined `Niklas Hambüchen <mail@nh2.me>` (2 real
  commits) with `Niklaas Baudet von Gersdorff <me@niklaas.eu>` (1 real commit) at confidence 0.85 —
  verified via `git log --author` that these are genuinely different people with different emails.
  After the merge, `ekos query find "Niklaas"` / `"Baudet"` return **zero** results — his identity
  and his one real commit are gone from the ledger under his own name. The surviving identity's
  `commit_count` property shows 2 (Niklas's own count only) with exactly 1 evidence entry — the
  merged-away contributor's contribution isn't double-counted, it's silently dropped, the same
  "surviving object swallows the rest" shape devlog_59 documented for `imported_visitors`.
- **27 unrelated documents merged into one, at confidence 0.98** — the single worst proposal in this
  run. `tracker/README.md`, `tracker/ARCHITECTURE.md`, `tracker/npm_package/CHANGELOG.md`,
  `tracker/npm_package/LICENSE.md`, and 23 test-fixture HTML files (cookie-consent variants,
  scroll-depth variants, engagement variants — genuinely unrelated content) all collapsed into one
  `Document` identity.
- Of the 14 same-source merge proposals this run produced, several looked plausibly correct on
  inspection (`RobertJoonas`/`Robert`, `Adam Rutkowski`/`Adam`/`Adam from Buildjet`, `Vini
  Brasil`/`Vinicius Brasil` — nickname/username variants of the same real person) alongside several
  that looked wrong on the same pattern as Niklas/Niklaas (`Felix Haase`/`Felix Krull`, `Martin
  DONADIEU`/`Martin Packman`, `David Janda`/`Davy Landman`, `Andrea Mazzarella`/`Andrey
  Meshkov`/`andreas-ementio`) — not individually re-verified against git history this session, but a
  concrete, ready-made batch for whoever picks up the identity-resolution threshold work next.

Still not fixed — same reasoning as devlog_59: a real design decision (per-kind stricter threshold,
near-total- vs. majority-overlap requirement, or something else) with estate-wide blast radius, not
a scoped preprocessing fix. Tracked in TODO.md alongside the original ClickHouse finding.

## Finding 3: RFC 0029's cross-system resolver has the same failure shape, confirmed live via MCP

`ekos identity scan` (a mechanistically separate pass, `crates/identity/src/cross_system.rs`, not
`resolve`'s same-source `DefaultResolver`) proposed `SameAs` between `plausible_events_db.sessions_v2`
(the real 43-column core events table) and `imported_visitors`/`imported_sources`/`ingest_counters`
— genuinely distinct real ClickHouse tables, even inside a single-vendor, single-system workspace
(Postgres never parsed, so there wasn't a second real system to cross-match against here). Confirmed
via `ekos_state` (real 43 columns vs. real 8) and rejected live over real MCP stdio using
`ekos_identity_review`, the one write-capable MCP tool — the decision was recorded as a real ledger
Event, exactly the "reviewable, never auto-merged" contract RFC 0029 promises. The other 31
candidates from this scan were not individually reviewed this session.

## Finding 4: `ekos ask` is brittle to natural-language phrasing — a retrieval bug, not a data gap

`ekos ask`'s `gather_context` passes the entire question string verbatim into
`Runtime::find_objects()` with no keyword extraction (`crates/runtime/src/ai.rs:131`). Every
full-sentence question tested — *"Who are the top contributors to this repository by commit
count?"*, *"Who is Niklas Hambüchen and what did they contribute?"*, *"What is Plausible Analytics
and how does it track visitors without cookies?"* — returned **zero** retrieved context and a
correctly-honest "I don't have enough information" answer, even though every one of those objects
is trivially findable via `ekos query find` or the `ekos_search` MCP tool with 2-3 keywords (MCP's
own tool description already says so: *"Use 2-3 keywords, not natural-language questions"*).
Reformulating the same questions as bare names/keywords (`"Niklas Hambüchen"`, `"README.md"`,
`"lib/plausible/stats.ex"`) immediately succeeded. One reformulated query surfaced a second, related
issue: asking `"README.md"` bare answered from `test/priv/README.md` (an 83-byte GeoLite2
test-fixture readme) instead of the real project `README.md`, which ranks 13th of 25 matches for
that literal filename — past the `max_matches: 3` default and not relevance-weighted by path
depth/importance. Not fixed this session; a real, generalizable gap (any agent host driving `ask`
naturally, in full sentences, will hit this), tracked in TODO.md.

## What worked, cleanly, on real complexity

- ClickHouse SQL: 15/15 tables, real columns/types/evidence (already proven, devlog_57-59 —
  reconfirmed on a fully cold run).
- Git analysis: 500 real commits, 124 real contributors recovered correctly.
- Local docs (41 documents), CI/CD (14 real GitHub Actions pipelines), dependency scanning (162
  files) — all completed without incident.
- The MCP surface: all 13 tools (`ekos_search`, `ekos_ekl`, `ekos_neighborhood`, `ekos_state`,
  `ekos_dependents`, `ekos_impact`, `ekos_diff`, `ekos_status`, transformation-explain/diff,
  `ekos_identity_review`, `ekos_clickhouse_query`) listed and callable over real stdio JSON-RPC,
  including the one write path, and consistently reproduced the same findings the CLI did — one
  ledger, one truth, wrong or right.
- Elixir/Phoenix business logic (`lib/plausible/stats.ex`): correctly summarized despite having no
  deep AST analyzer (unlike Python/Rust/SQL) — because the file is small enough that the LLM's raw
  excerpt was sufficient. A right answer for a shallow reason, worth distinguishing honestly from
  the AST-backed ClickHouse answers.

---

## Knowledge Captured

- **A same-day warm-cache rerun is not a cold-ingestion number, and the difference is large enough
  to matter for any published claim.** `build.rs`'s fingerprint-based skip-if-unchanged path
  (content-hash comparison against the ledger, independent of `ekos clean`'s artifact-cache
  clearing) skipped 2021/2022 files on a same-day rerun, producing an 11.6s number that looks like
  "ingestion time" but isn't. Getting a true cold number required moving the whole `.ekos/`
  workspace aside and reinitializing, not just `ekos clean`.
- **`rm -rf` is blocked by this environment's permission policy regardless of destination or
  context** — even on a regenerable, untracked, local-only directory the user had just approved
  wiping via AskUserQuestion. `mv` to a timestamped backup path achieves the same "force a cold
  rebuild" effect without tripping the block, and is strictly safer (reversible) besides.
- **`OLLAMA_MODEL` must be set explicitly; the built-in default (`llama3.1:8b`) isn't assumed to be
  installed.** `ekos ask` failed outright (404) against this environment's actual installed models
  (`qwen2.5:1.5b`, `llama3:latest`, `glm-4.7-flash`) until `OLLAMA_MODEL` was set. Separately, the
  smallest installed model (`qwen2.5:1.5b`) returned genuinely empty completions for this
  grounding+citation prompt shape — too small to follow the format — while `llama3:latest` handled
  it correctly (including declining honestly when context was empty). Model capacity is a real,
  separate variable from grounding-pipeline correctness; this session used `llama3:latest`
  throughout after the first empty response, and that choice is a confound worth stating plainly
  in any published "what could the agent answer" claim.
- **A compiler pass finally succeeding surfaces bugs in unrelated, untouched passes — a pattern now
  confirmed twice, on two different object kinds, by two different resolvers.** devlog_59 first
  named this (RFC 0057/0058's ClickHouse parser fix exposing `crates/identity`'s Table over-merge).
  This session shows the same shape isn't unique to that one pass/kind pair: `resolve`'s same-source
  merge over-merges `Person` and `Document` too, and RFC 0029's separately-coded cross-system
  resolver independently produces the same class of false positive. Three data points is enough to
  call this a property of the 0.85 default-threshold design, not a coincidence tied to one code
  path.
- **"Ask the CLI a natural question" and "have an agent use the MCP tools correctly" are different
  reliability stories, and conflating them overstates the weaker one.** Every `ekos ask` failure
  this session was a retrieval-phrasing problem, not a missing-data problem — the same questions,
  asked as keywords via `ekos_search`/`ekos query find`, succeeded. An agent host that reads MCP's
  own tool descriptions (which already say "2-3 keywords, not natural-language questions") and
  reformulates accordingly would not hit this; a naive integration that pipes user questions
  straight into `ask` would, every time.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/presentations/analytics-full-loop.html` | New deck: cold whole-repo pipeline run, `ekos ask`/MCP Q&A grading, three new honestly-reported gaps |
| `docs/presentations/examples/analytics-full-loop/*` | 19 real, unedited transcript files backing the deck (pipeline stages, `ask` Q&A, MCP JSON-RPC sessions) |
| `docs/presentations.html`, `docs/index.html`, `README.md` | New deck listing entries (all three link points, per the `1cf5f66` catch-up lesson) |
| `TODO.md` | New tracked-but-not-fixed items: Postgres `INCREMENT` parse gap, `ekos ask` retrieval brittleness; existing identity-merge item updated to note it generalizes past `Table` |
| (in `analytics/`, not this repo) `.ekos/` | Rebuilt from a genuinely cold state; prior session's cache preserved at `.ekos.bak-20260820`, not deleted |

## Still open (tracked, not silently dropped)

- **Postgres `structure.sql` doesn't parse at all** (`INCREMENT` clause) — new this session, not
  fixed.
- **Identity over-merging** (Table, Person, Document; same-source and cross-system resolvers alike)
  — not fixed; devlog_59's original ClickHouse-Table finding plus this session's Person/Document/
  cross-system confirmations are one underlying design gap, not several.
- **`ekos ask` retrieval brittleness** to full-sentence phrasing — new this session, not fixed.
- The other 31 of 32 `ekos identity scan` candidates and 13 of 14 `resolve` merge proposals from
  this run were not individually verified against ground truth — a ready-made batch for follow-on
  work, not assumed correct or incorrect.

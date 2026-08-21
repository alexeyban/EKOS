# Devlog 63 — GitHub connector, live, end to end (RFC 0062)

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Item 3 of the strategic roadmap: ship one connector "beyond Git/code" against live, real data —
not a mock. No account exists for Salesforce/SAP/Oracle/Fabric/Snowflake/Confluence/Jira, but this
environment has a real, authenticated `gh` CLI token, so GitHub — already real HTTP-calling code,
just never run live — is what got shipped. Investigating what a live run would need found two real
gaps before writing any code (a reference-detection blind spot and zero pagination), fixed both,
then running live at real scale (1,600 real issues/PRs from `github.com/plausible/analytics`)
surfaced a third, much more severe gap — the same identity-over-merge class RFC 0060 already fixed
for SQL/Person/Document objects, this time collapsing 96% of GitHub items into one identity — found
and fixed the same session, not deferred. The result: a genuine, live-verified four-way chain —
external documentation ↔ GitHub issues/PRs ↔ source code ↔ compiled SQL schema — plus an honest
report of exactly where the fix is incomplete, using real numbers, not glossed over.

---

## What was done

- Fixed `github_analyzer.rs`'s reference detection: added `find_bare_issue_numbers` alongside the
  existing closes-keyword scan, so a real PR body like `"Migration for #3828"` (no keyword) now
  produces a `References` edge, not silence. 3 new tests + 1 behavior-changed.
- Fixed `plugins/github`'s zero pagination: `GitHubApiClient::with_pagination`, `issues_url`,
  `parse_next_page_url` (standard `Link: rel="next"` header following), bounded by
  `EKOS_GITHUB_MAX_PAGES`. 5 new tests.
- Vendored two real pages from https://plausible.io/docs (`google-analytics-import`,
  `csv-import`) unmodified under a sibling directory, and pointed `analytics/ekos.toml`'s
  `[observe] paths` at them as a second real source — no new code, reusing the existing
  `localdocs` connector and RFC 0044 multi-project support.
- Wrote RFC 0062, ran the full pipeline live against `github.com/plausible/analytics` with a real
  token from `gh auth token`: `EKOS_GITHUB_PER_PAGE=100`, `EKOS_GITHUB_MAX_PAGES=16` — 1,600 real
  issues/PRs fetched, ~23 minutes wall-clock (dominated by the per-PR `list_files` call, ~0.5s
  each, sequential, no concurrency — a real, accepted cost for a one-time documented run, not
  engineered around).
- Found and fixed a third gap, live, mid-session (see below).
- Live-verified the result: a real PR → real file `References` edge, and a single real-text search
  surfacing real docs + real code + real GitHub items together.

---

## Finding 1 (known going in): reference detection missed the dominant real-world case

`find_closed_issue_numbers` only matched `#<number>` immediately preceded by a documented GitHub
auto-close keyword (`closes`, `fixes`, `resolves`, ...). Real PR bodies sampled live from
`plausible/analytics` mostly don't use that vocabulary: PR #3834's real body is literally
`"Migration for #3828"` — bare, no keyword. Fixed by scanning for every `#N` occurrence
independent of keyword, emitting a `References` edge with a distinguishing evidence fragment
(`"mentions #N"` vs. `"...closing #N"`) reusing the existing relationship kind rather than adding
a new one.

## Finding 2 (known going in): zero pagination

`GitHubApiClient::list_items` issued one unparameterized request — GitHub's implicit default (30
items, newest-first) silently truncated anything beyond the most recent page. `plausible/analytics`
has on the order of 4,600 real issues/PRs. Fixed with standard `Link`-header-following pagination,
bounded by an explicit page cap — never an unbounded crawl.

## Finding 3 (found live, mid-session, not planned): 96% of real GitHub items collapsed into one identity

Running the fixed connector live for the first time at real scale (1,600 real items) surfaced this
immediately: `ekos resolve` proposed merging **1,533 of the 1,600 real items — 96% — into a single
identity at confidence 1.00**. This wasn't a subtle edge case; it was the single largest fact about
the compiled result.

**Root cause, read directly from the code**: `Custom("Issue")`/`Custom("PullRequest")` objects are
named `"{owner}/{repo}#{number}: {title}"` (`github_analyzer.rs`). Every item in one repo shares
the `"{owner}/{repo}#"` prefix — and unlike `Table`'s schema qualifier (RFC 0060's earlier fix,
`plausible_events_db.imported_visitors`), this one also swallows the item's own number, making the
shared, uninformative prefix proportionally *much* longer relative to a typical short PR title.
Jaro-Winkler's prefix bonus inflated every pairwise comparison regardless of how different the
real titles were — "Bump docker/login-action from 3 to 4" and "Reduce noise in 2FA enforce
notifications" scored as similar as two names of the same real thing, because both start with
`"plausible/analytics#NNNN: "`.

**Fixed the same session**, extending RFC 0060's own extension point (`name_for_similarity`,
`crates/identity/src/lib.rs`) with a third case: for `Issue`/`PullRequest`, find the first `#`,
then the first `": "` after it, and compare only what follows. Verified: the catastrophic
1,533-object group is gone; the largest remaining real group is 174 (still large, see below).

### The honest part: this fix is not complete either, and the original demo PR paid the cost

PR #5158 ("time-on-page: `imported_pages` new columns") was chosen as this RFC's primary example
before the live run started. After both fixes, `ekos resolve` still merges it into a real,
16-object group of other "time-on-page:"-prefixed sibling PRs — a real, smaller-scale instance of
the exact same class of problem, now confirmed on `Issue`/`PullRequest` objects. Querying the
surviving canonical identity (`#5100: time-on-page: Ingestion logic for engagement_time`) shows
only *its own* excerpt and properties — #5158's real body and its real migration-file
`References` edges are gone, exactly matching RFC 0060's already-documented "surviving object
swallows the rest" pattern.

**This is stated plainly rather than swapped out silently for a cleaner-looking example.** The
live-verified positive chain in the published deck uses a different, real, standalone-surviving
PR (#6421, "Assert how we store data imported from GA4" → real `References` edge →
`test/plausible/imported/google_analytics4_test.exs`, confirmed via `ekos query neighbourhood`) —
and separately, `#5158`'s fate is reported as its own honest finding, not hidden behind the swap.

**Not chased further this session**: 1,439 real merge groups remain post-fix (down from a single
1,533-object catastrophe, but still substantial — dependency-bump PRs sharing a `"Bump X from Y to
Z"` template and feature-prefix conventions like `"time-on-page:"` still cluster via transitive
Union-Find chaining even where no single pair scores dramatically wrong). Same reasoning RFC 0060
already gave for not pursuing a complete identity-resolution fix: a real design decision with
estate-wide blast radius, not a scoped bugfix, and this large a real sample is exactly the kind of
data a future, deliberate RFC on this problem should use — not something to hand-tune against in
the middle of an unrelated connector RFC.

---

## What worked, live, cleanly

- Real `Custom("Issue")`/`Custom("PullRequest")` objects, real evidence, real state/excerpt
  properties, compiled from 1,600 real GitHub items.
- Real `References` edges: PR #6421 → `test/plausible/imported/google_analytics4_test.exs`,
  confirmed graph-traversable via `ekos query neighbourhood`.
- A single `ekos_search`/MCP `ekos_neighborhood` call for `"google analytics import"` surfaced —
  in one query, no hand-curation — the real external docs page (`google-analytics-import.html`,
  vendored from `plausible.io/docs`), real code (`lib/plausible/imported/google_analytics4.ex`,
  `lib/plausible_web/templates/email/google_analytics_import.html.heex`), and a real GitHub PR
  (#5305) together. This emergent result is a stronger demonstration than the single hand-picked
  chain this RFC set out to build.
- Real evidence-cited MCP stdio JSON-RPC session, matching the `analytics-full-loop` deck's
  precedent.

---

## Knowledge Captured

- **A shared, uninformative name prefix that also swallows a unique identifier (not just a
  namespace) is proportionally more dangerous than a shorter one.** RFC 0060's `Table` fix
  stripped `"plausible_events_db."` — a namespace only. GitHub's `"{owner}/{repo}#{number}: "`
  additionally swallows the item's own unique number, making the *informative* part of the name
  (the title) a smaller fraction of the string, and the *uninformative* shared part
  correspondingly larger and more dominant in Jaro-Winkler's prefix bonus. Worth checking this
  specific shape (shared prefix + unique-but-uninformative-for-similarity infix) explicitly
  whenever extending `name_for_similarity` to a new kind.
- **Fixing a connector's real bugs can immediately produce a new, bigger data point for an
  already-known-incomplete fix elsewhere in the system** — the same lesson devlog_59-61 drew from
  a different angle (RFC 0057/0058's parser fix exposing RFC 0060's identity issue), now shown a
  third time, and at a much larger real scale (1,600 items, not a handful). A system's individual
  fixes compound in ways worth re-checking, not assuming independent.
- **A pass-level cache keyed only on artifact content, not on the compiler binary's own version,
  will silently serve stale results after a code change with no input-data change.** Rebuilding
  `target/release/ekos` after the identity fix and rerunning `ekos compile` showed `skipping pass
  (cached)` and returned pre-fix numbers — the `SemanticCompilerPass`'s `cache_inputs` are
  recover-stage artifact IDs, which the identity fix didn't change, so the manifest never
  invalidated. Worked around by moving `.ekos/artifacts/pass-manifests/` aside (non-destructively)
  to force a true fresh recompute — a real gap in the caching design, out of scope to fix here, but
  worth naming as a known trap for any future session that changes compiler-pass logic without
  changing its inputs.
- **Sequential per-item HTTP calls with no concurrency are the real cost of a "beyond Git/code"
  live connector at any real scale** — 1,600 items took ~23 minutes, almost entirely the per-PR
  `list_files` round-trip at ~0.5s each. A one-time documented session tolerates this; a routine
  re-run workflow would need real concurrency, not attempted here.
- **`nohup`/`disown` plus a `Monitor`-based wait-for-completion loop is the reliable pattern for a
  long-running foreground command in this environment** — both direct `Bash` invocations and
  `run_in_background` wrappers were killed by session/tool-level timeouts mid-commit (a ~1-2 minute
  soft ceiling observed in practice) before a ~1-2 minute `ekos commit` on this larger dataset could
  finish; fully detaching the process from the tool's own process tree let it run to completion
  independent of any single tool call's timeout.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0062-github-live-cross-system-verification.md` | RFC, updated to reflect the third finding and its fix |
| `ekos/crates/recovery/src/github_analyzer.rs` | `find_bare_issue_numbers`, new `References` edge, 3 new + 1 behavior-changed test |
| `ekos/plugins/github/src/lib.rs` | Pagination (`issues_url`, `parse_next_page_url`, `with_pagination`), 5 new tests, doc comments updated to live-verified status |
| `ekos/crates/cli/src/commands/build.rs` | `EKOS_GITHUB_PER_PAGE`/`EKOS_GITHUB_MAX_PAGES` env var wiring |
| `ekos/crates/identity/src/lib.rs` | `name_for_similarity` extended for `Issue`/`PullRequest`, 2 new tests |
| `docs/presentations/github-live-cross-system.html` | New deck |
| `docs/presentations/examples/github-live-cross-system/*` | Real transcripts backing every claim in the deck |
| `docs/index.html`, `docs/presentations.html`, `README.md`, `TODO.md` | Site wiring, roadmap tracking |
| (not in this repo) `/home/legion/PycharmProjects/analytics-docs/*.html` | Real, unmodified `plausible.io/docs` pages |
| (not in this repo) `/home/legion/PycharmProjects/analytics/ekos.toml`, `.ekos/` | `[observe] paths` extended; ledger rebuilt with real GitHub data |

## Still open (tracked, not silently dropped)

- **1,439 real GitHub-item merge groups remain** after the prefix fix (largest 174) — a smaller-
  scale instance of RFC 0060's already-documented, not-yet-fully-solved identity-resolution
  limitation. PR #5158 specifically is affected (merged into a 16-object group, its own evidence
  lost under the surviving identity).
- **No GitHub secondary (abuse-detection) rate-limit backoff.** Accepted risk for a one-time run;
  not engineered around.
- **No concurrency in per-PR file fetching.** ~23 minutes for 1,600 items; fine for a documented
  one-time session, not fine for a routine re-run workflow.
- **Full-URL issue/PR references** (e.g. `"Extracted from https://github.com/.../pull/6591"`,
  confirmed real in this repo) are still not detected — only bare `#N` and keyword-qualified `#N`
  are. Left for a future RFC if it turns out to matter.

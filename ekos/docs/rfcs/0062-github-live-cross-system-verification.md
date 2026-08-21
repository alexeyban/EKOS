# RFC 0062 — GitHub Connector: Live Verification + Real Cross-System Chain

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-20

---

## Motivation

Item 3 of the project's own strategic roadmap: ship one connector "beyond Git/code" against
**live, real data**, not a mock. `README.md` names Salesforce/SAP/Oracle/Microsoft
Fabric/Snowflake as "scaffolded, mock-tested — none yet exercised against a live account," and
no account exists for any of them, nor for Confluence/Jira. But this environment has a real,
authenticated `gh` CLI token (`repo`/`read:org`/`workflow` scopes, 5,000 req/hr, confirmed
working against `api.github.com`) — GitHub is the connector actually live-testable here. Unlike
the Salesforce-family connectors, `ekos/plugins/github`'s `GitHubApiClient` is already real
`reqwest`-based HTTP code written to the documented API shape, not a proof-of-concept mock shape
— its own module doc simply states it has "never been run against the live API."

Investigating what a live run would actually need, before writing any code (per the mandated
workflow), found two real gaps — not hypothesized, confirmed live against
`github.com/plausible/analytics`, the real repo this session's RFC 0059/0060/0061 already use:

1. **`github_analyzer.rs`'s reference detection misses the dominant real-world case.**
   `find_closed_issue_numbers` (`crates/recovery/src/github_analyzer.rs`) only matches
   `#<number>` immediately preceded by one of GitHub's documented auto-close keywords
   (`close(s|d)`, `fix(es|ed)`, `resolve(s|d)`). Real PR bodies sampled live from this repo mostly
   don't use that vocabulary: PR #3834's real body is literally `"Migration for #3828"` — a bare
   `#N` reference, no keyword anywhere. PR #6606 references `"PR #6514"` mid-sentence. PR #6597
   references a full URL, `"Extracted from https://github.com/plausible/analytics/pull/6591"`.
   With the pre-RFC-0062 code, none of these produce a `References` edge.
2. **`plugins/github`'s `GitHubApiClient::list_items` has zero pagination.** One unparameterized
   `GET .../issues?state=all` call — no `per_page`, `page`, or `Link`-header handling anywhere in
   the file (confirmed by direct read). GitHub's implicit default is 30 items, newest-first.
   `plausible/analytics` has on the order of 4,600 real issues/PRs; an unmodified live run would
   only ever observe the ~30 most recent, silently truncating everything else.

**A third gap was found only once the first two were fixed and the connector was actually run live
at real scale** — not hypothesized, not visible from reading the code, only from the compiled
result of 1,600 real items: `crates/identity`'s `DefaultResolver` (the same resolver RFC 0060 fixed
for `Table`/`Person`/`Document`/`Pipeline`) collapsed **1,533 of the 1,600 real GitHub items — 96%
— into a single identity at confidence 1.00**. Root cause: `Custom("Issue")`/`Custom("PullRequest")`
objects are named `"{owner}/{repo}#{number}: {title}"` (`github_analyzer.rs`); every item in one
repo shares the `"{owner}/{repo}#"` prefix, which — unlike `Table`'s shorter schema qualifier —
also swallows the item number, making it proportionally much longer relative to a typical short PR
title, and inflating Jaro-Winkler's prefix bonus regardless of how different the real titles are.
This is RFC 0060's exact fix pattern (`name_for_similarity`, `crates/identity/src/lib.rs`),
previously scoped to `Table` only, now needed for `Issue`/`PullRequest` too.

**The demo chain** ties into real content this session already has deep, verified knowledge of —
the `imported_visitors`/`imported_browsers`/`imported_pages` ClickHouse tables RFC 0060's
identity-resolution fix already uses as ground truth — via the Google Analytics / CSV import
feature:

- **Real external documentation** (not in the git repo — a second real source):
  https://plausible.io/docs/google-analytics-import and `.../csv-import`, fetched live and
  vendored unmodified under `/home/legion/PycharmProjects/analytics-docs/` (outside the
  `analytics/` checkout, so the real, unmodified repo stays pristine).
- **Real code**: `lib/plausible/imported/google_analytics4.ex`, `csv_importer.ex`.
- **Real GitHub PRs/issues**, all confirmed live: **PR #5158** "`imported_pages` new columns" —
  merged, real body (`"This PR adds 2 migrations: Adding new columns to imported_pages..."`),
  real changed files are exactly two migrations that ALTER `priv/ingest_repo/migrations/...` for
  `imported_pages` (an already-supported PR→file `References` edge, no new code needed for this
  specific hop); also #6421 "Assert how we store data imported from GA4", #6106 "Reduce imported
  opts queries", #6073 "...imported data + fix 500", #5305 "GA4 import: refresh google auth..." —
  all closed, all real, all about this exact feature.
- **Real schema**: `plausible_events_db.imported_pages`/`imported_visitors`/`imported_browsers` —
  already compiled `Table` objects in this workspace's ledger.

This gives a genuine, verifiable four-way chain — **external docs ↔ GitHub issues/PRs ↔ code ↔
compiled SQL schema** — live-verified end to end (see Acceptance Criteria). **What actually
happened live differs honestly from the plan in one respect, worth stating plainly rather than
retrofitting the narrative**: PR #5158 itself, chosen as the primary example, was caught by the
third gap above (the pre-fix 96% collapse) and, even after the `name_for_similarity` fix, remains
merged into a smaller (16-object) group of "time-on-page:"-prefixed sibling PRs — a real,
still-open residual instance of RFC 0060's already-documented "not a complete fix" finding, now
confirmed on `Issue`/`PullRequest` objects too. The live-verified positive chain uses **PR #6421**
instead (`References` → `test/plausible/imported/google_analytics4_test.exs`, confirmed via
`ekos query neighbourhood`), and a single `ekos_search "google analytics import"` independently
surfaces the real docs page, real code (`lib/plausible/imported/google_analytics4.ex`), and real
GitHub items together — a stronger, emergent demonstration than the single hand-picked chain this
RFC originally set out to show.

## Scope

1. `find_bare_issue_numbers` + a new, weaker `References` edge in `github_analyzer.rs`.
2. Bounded, opt-in pagination in `GitHubApiClient` (`plugins/github`).
3. `name_for_similarity` (`crates/identity/src/lib.rs`, RFC 0060's extension point) extended to
   strip the `"{owner}/{repo}#{number}: "` prefix for `Issue`/`PullRequest` objects — found
   necessary only once gaps 1-2 were fixed and the connector was run live at real scale.
4. Two real `plausible.io/docs` pages vendored as a second observed source.
5. One documented live run against `plausible/analytics`, transcripts captured.
6. Doc-comment updates reflecting live-verified status.
7. New deck + site wiring + devlog + `TODO.md` entry.

## Non-goals

- **Not fetching comments, reviews, labels, milestones, or timeline events** — only what
  `GitHubItem` already carries (title/body/state/changed-files).
- **Not extending `identity/src/cross_system.rs`'s `matchable_name()`** for `Issue`/`PullRequest`
  kinds. The demo chain works entirely through existing `References` edges and `file_kir_id`
  reuse — no new identity-resolution logic is needed.
- **Not building a general GitHub-Flavored-Markdown reference parser.** Plain digit-after-`#`
  scanning is enough for every real case found. A known, accepted limitation: `#3828a1` matches as
  `3828` (no word-boundary check after the digits) — documented, not engineered around. Full-URL
  references (PR #6597's shape) are also out of scope for this pass — a real, less common pattern
  than bare `#N`, left for a future RFC if it turns out to matter.
  **Full-URL references: fixed** — `devlog_65` (2026-08-20/21) added `find_full_url_issue_numbers`
  in `github_analyzer.rs`. The `#3828a1`-style word-boundary limitation remains open.
- **Not sweeping the connector's full ~6,600-item history.** Bounded, newest-first pagination
  (a capped page count) reaches every real example in this RFC without an unbounded crawl.
- **Not adding GitHub secondary (abuse-detection) rate-limit backoff/retry logic.** None exists
  today; accepted as a real risk for a one-time documented run, not engineered around.
  _Tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "Connector-specific gaps"._
- **Not fully solving GitHub-item identity over-merging.** `name_for_similarity`'s prefix strip
  fixes the catastrophic case (96% collapse into one identity) but is explicitly not a complete
  fix — RFC 0060's already-stated limitation on this exact threshold/formula applies here too.
  Confirmed live: 1,439 smaller merge groups remain post-fix (largest 174), including the specific
  demo PR (#5158, merged into a 16-object "time-on-page:"-prefixed sibling group). Reported
  honestly, not chased further — the same design-decision-not-a-bugfix reasoning RFC 0060 already
  gave for not pursuing a complete fix in that RFC either.
  _Tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "Identity resolution"
  (same underlying gap as RFC 0060's)._
- **Not inventing a new automated/repeatable live-test framework.** No `#[ignore = "requires
  live..."]` pattern exists anywhere in this codebase. Following RFC 0056's precedent exactly:
  one real, documented live session, verified manually, recorded as checked acceptance criteria
  below — not a new CI-style live-testing convention.
- **Not scraping all of plausible.io/docs.** Two real pages are enough; vendored unmodified, not
  trimmed or synthesized.

## Design

**`github_analyzer.rs`** — `find_bare_issue_numbers(body: &str) -> Vec<u64>` scans every `#N`
occurrence in `body`, deduplicated, regardless of any preceding keyword. Wired into the existing
second pass immediately after the closes-keyword loop: for each bare match, skip if it's a
self-reference or already covered by a closes-keyword edge, then emit a `References` relationship
— the same kind the closes-keyword edge already uses (not a new `RelationshipKind`; the two edge
shapes already coexist under one kind today, distinguished only by evidence text, and adding a
second kind would fragment every existing consumer — `ekos_neighborhood`, `ekos_impact`, deck
rendering — for no real benefit). Evidence fragment: `"#{X} body mentions #{Y}"`, vs. the existing
`"...references closing #{Y}"` — grep-distinguishable in transcripts without a new field.

**`plugins/github`** — two pure helpers: `issues_url(owner, repo, per_page) -> String` (byte-
identical to the pre-existing URL when `per_page` is `None` — zero behavior change for any caller
that doesn't opt in) and `parse_next_page_url(link_header: &str) -> Option<String>` (standard
`Link: <url>; rel="next"` parsing). `GitHubApiClient::with_pagination(per_page, max_pages)` is an
opt-in builder method; `list_items` loops while a `next` link exists and `pages_fetched <
max_pages`, capturing the response's `Link` header before consuming its body. New optional env
vars in `build.rs`: `EKOS_GITHUB_PER_PAGE`, `EKOS_GITHUB_MAX_PAGES` — GitHub's default sort
(newest-first) already suits the chosen recent-issue demo chain, so no sort/direction env vars
were added.

**`crates/identity/src/lib.rs`'s `name_for_similarity`** (RFC 0060's extension point) gains a third
case alongside `Table`'s schema-qualifier strip: for `Custom("Issue")`/`Custom("PullRequest")`
objects, find the first `#`, then the first `": "` after it, and return everything after that
separator (the real title only). Matches `github_analyzer.rs`'s exact naming format
(`"{owner}/{repo}#{number}: {title}"`); safe against titles that themselves contain `": "` later
(e.g. `"time-on-page: imported_pages new columns"`) since only the *first* separator — the
number/title boundary — is stripped.

**Documentation ingestion** — no new code. `analytics/ekos.toml`'s `[observe] paths` extended
from `["."]` to `[".", "../analytics-docs"]` (RFC 0044 multi-project support, already used
elsewhere); the existing `localdocs` connector and `local_docs_analyzer` pick up the two real
HTML pages unmodified.

## Alternatives Considered

- **A new `RelationshipKind::Mentions`** instead of reusing `References` — rejected: fragments
  every existing `RelationshipKind`-filtering consumer for no real benefit; evidence text already
  carries the distinction.
- **A full historical sweep (ascending from issue #1) to reach an older demo chain** (an earlier
  candidate used issue #3828/PR #3834 from 2024) — rejected once a stronger, more-recent real
  chain was found (the Google Analytics import feature) that needs only bounded, newest-first
  pagination — simpler, faster, and doesn't require a large one-time API sweep just to reach one
  example. PR #3834's real body is still used as the literal fixture for the bare-mention unit
  test, since it's a clean, real, already-verified example of the exact gap.
- **Chasing the residual identity over-merge further** (e.g. requiring near-total rather than
  majority title-token overlap, or a per-kind stricter threshold for `Issue`/`PullRequest`) —
  rejected for this pass, same reasoning RFC 0060 gave: a real design decision with estate-wide
  blast radius, not a scoped fix, and 1,439 real groups is too large a sample to hand-tune against
  without risking a new, differently-wrong threshold. Reported honestly instead.
- **A targeted "fetch these specific issue numbers" feature** (`GET /issues/{number}` directly) —
  considered as a way to deterministically include an old chain regardless of pagination depth;
  rejected as unneeded complexity once the demo chain moved into recent history.

## Testing

- `github_analyzer.rs`: `bare_mention_without_keyword_emits_reference_edge_real_data` (PR #3834's
  real body, literal), `unrecognized_phrasing_emits_bare_mention_not_closing_edge` (regression:
  the previously-empty-relationships case now correctly emits one mention edge, not zero),
  `closes_keyword_hit_does_not_also_emit_duplicate_bare_edge`, `self_mention_does_not_emit_self_loop_edge`.
  Pre-existing tests (`finds_closes_keyword_case_insensitively`, `pr_files_changed_emit_references_edges`,
  `same_item_across_two_runs_gets_same_object_id`, `body_closes_keyword_emits_reference_to_issue`,
  `ignores_unrecognized_phrasing`) all still pass unchanged.
- `plugins/github`: `issues_url_with_no_per_page_matches_the_legacy_url_exactly` (pins the exact
  legacy string), `issues_url_appends_per_page_when_set`, `parse_next_page_url_finds_rel_next_among_multiple_links`,
  `parse_next_page_url_returns_none_without_a_next_rel`, `parse_next_page_url_returns_none_for_empty_header`.
  The multi-page HTTP loop itself is verified only by the live run's captured transcripts (no
  HTTP-mocking crate exists in this repo; not adding one for this).
- `crates/identity`: `name_for_similarity_strips_owner_repo_number_prefix_for_github_items`,
  `real_github_pull_requests_do_not_all_merge_into_one_identity` (three real, genuinely unrelated
  PR titles sampled live, asserting zero merge proposals).
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.
- Live verification: rebuilt `target/release/ekos`, ran the full pipeline against the real
  `analytics/` workspace with `EKOS_GITHUB_*` env vars set from `gh auth token`. Confirmed real
  `Custom("Issue")`/`Custom("PullRequest")` objects with real evidence; confirmed PR #6421 →
  `test/plausible/imported/google_analytics4_test.exs` is a graph-traversable `References` edge
  via `ekos query neighbourhood`; confirmed a single `ekos_search`/MCP `ekos_neighborhood` call for
  `"google analytics import"` surfaces the real docs pages, real code, and real GitHub items
  together; confirmed (honestly, not smoothed over) that PR #5158 specifically remains merged into
  a 16-object sibling group even after the `name_for_similarity` fix, and that its own real
  evidence does not survive under the surviving canonical identity — the residual-limitation
  finding this RFC's Non-goals section states plainly.

## Acceptance Criteria

- [x] `find_bare_issue_numbers` implemented and wired; module doc updated.
- [x] `GitHubApiClient::with_pagination`, `issues_url`, `parse_next_page_url` implemented.
- [x] `name_for_similarity` extended for `Issue`/`PullRequest` (found necessary live, not planned
      up front).
- [x] New unit tests pass (9 in `github_analyzer.rs`, 9 in `plugins/github`, 2 in `crates/identity`),
      full workspace `cargo build/test/clippy/fmt` clean.
- [x] Two real `plausible.io/docs` pages fetched and vendored under `analytics-docs/`;
      `analytics/ekos.toml` updated to observe them.
- [x] Live: full pipeline run against the real `analytics/` workspace with real GitHub credentials
      (1,600 real issues/PRs fetched, ~23 minutes wall-clock); the PR #6421 → file chain and the
      docs↔code↔GitHub cross-search both confirmed queryable, real evidence cited throughout;
      recorded in `devlog_63.md`.
- [x] `plugins/github/src/lib.rs`'s module doc updated to reflect live-verified status.
- [x] New deck (`docs/presentations/github-live-cross-system.html`) + site wiring + `devlog_63.md`
      + `TODO.md` entry.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0062-github-live-cross-system-verification.md` | This RFC |
| `ekos/crates/recovery/src/github_analyzer.rs` | `find_bare_issue_numbers`, new `References` edge, 3 new + 1 behavior-changed test |
| `ekos/plugins/github/src/lib.rs` | `issues_url`, `parse_next_page_url`, `GitHubApiClient::with_pagination`, paginated `list_items`, 5 new tests |
| `ekos/crates/cli/src/commands/build.rs` | `EKOS_GITHUB_PER_PAGE`/`EKOS_GITHUB_MAX_PAGES` env var wiring |
| `ekos/crates/identity/src/lib.rs` | `name_for_similarity` extended for `Issue`/`PullRequest`, 2 new tests |
| `docs/presentations/github-live-cross-system.html` + `docs/presentations/examples/github-live-cross-system/*` | New deck + real transcripts |
| `docs/index.html`, `docs/presentations.html`, `README.md`, `TODO.md`, `devlog_63.md` | Site wiring, devlog, roadmap tracking |
| `/home/legion/PycharmProjects/analytics-docs/*.html` (not in this repo) | Real, unmodified `plausible.io/docs` pages |
| `/home/legion/PycharmProjects/analytics/ekos.toml` (not in this repo) | `[observe] paths` extended to the second real source |

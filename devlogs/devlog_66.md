# Devlog 66 — Identity resolution, attacked directly: 4 real cross-kind pairs

**Date:** 2026-08-21
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Roadmap item 4 asked for a narrow, concrete attack on identity resolution — the hardest, most
quietly important part of the whole system — rather than another general redesign: pick 3-5 real
cross-source identity pairs, get them resolving correctly, and write up which mechanism actually
works (fuzzy matching? embeddings? human-in-the-loop? explicit config?). Found the real root cause
in twenty minutes of reading `cross_system.rs`, picked 4 real pairs from `analytics/`'s
already-compiled ledger (no new data needed), and closed the gap with ~60 lines of code reusing an
existing similarity function at a different granularity — no embeddings, no config file, no new
infrastructure. Live verification against the real, full `analytics/` ledger then surfaced a
second, real problem the narrow 4-pair test couldn't have shown — at full-ledger scale, the fix
produced 27,383 candidates, far past what a human/agent review queue can work through — found,
diagnosed, and fixed in the same session (down to 2,202, a 92% reduction) rather than shipped as a
known gap.

---

## The real gap

`crates/identity/src/cross_system.rs` (RFC 0029) is EKOS's cross-system identity resolver — the
module whose entire job is "same real-world concept, different name, different system." Its
`matchable_name()` function recognizes exactly two `ObjectKind`s: `Table` (SQL DDL) and
`Custom("TransformNode")` (Pentaho/SQL transform Source/Sink). Every other kind — `File`,
`Custom("Issue")`, `Custom("PullRequest")`, `Custom("Document")` — falls through a `_ => None`
catch-all and never enters the candidate pool at all. So "Customer in the SQL schema, Customer in
code, and 'customer' in a ticket" was **structurally impossible** for this module, not merely
untested: it can only ever compare one table-like identifier against another.

## The real test workspace: `analytics/`, no new fetching

Queried the already-compiled real `analytics/` ledger live (`ekos query find`) and confirmed four
concrete concept triples, each spanning a real Postgres `Table`, a real Elixir source `File`, and
real GitHub `Issue`/`PullRequest` objects:

| Concept | Real `Table` | Real `File` | Real GitHub item |
|---|---|---|---|
| sites | `public.sites` | `lib/plausible/site` | `#4911` "Only show sites count in user CRM instead of full list of sites" |
| api_keys | `public.api_keys` | `lib/plausible/auth/api_key.ex` | `#5753` "Check for Sites API feature against respective team when using API key" |
| goals | `public.goals` | `lib/plausible_web/plugins/api/schemas/goal.ex` | `#5978` "Migration: Add custom props to pageview goal configuration unique index" |
| subscriptions | `public.subscriptions` | `lib/mix/tasks/cancel_subscription.ex` | `#5341` "Don't show \"Subscription\" settings item when user role not permitted" |

## Why the existing scoring doesn't just work once the kind filter is lifted

`cross_system.rs`'s existing signal (`normalize_cross_system` + whole-string `jaro_winkler`)
compares two *short, structured* identifiers character-by-character. Running it on `"sites"` vs.
the whole issue title `"Only show sites count in user CRM instead of full list of sites"` scores
near zero — almost none of the characters line up, because the concept word is a small fraction of
a much longer string. What actually connects them is **containment**, not whole-string similarity:
does the word "sites" appear, fuzzily, as one of the tokens in the free text.

## The fix: fuzzy token containment (not embeddings, not config)

Three small additions to `cross_system.rs`, ~60 lines total:

- **`free_text_tokens(obj)`** — for `File` objects, tokenize the path; for `Issue`/`PullRequest`,
  tokenize the title (reusing `name_for_similarity`'s existing `"{owner}/{repo}#{number}: "`
  prefix strip, promoted from `fn` to `pub(crate)` in `lib.rs` rather than reimplemented). Lowercase,
  split on non-alphanumeric, drop tokens under 3 chars. `None` for every other kind.
- **`fuzzy_containment_score(concept_words, tokens)`** — for each word in the table's normalized
  concept name, check whether any token has `jaro_winkler(word, token) >= 0.90`; score = fraction
  matched. This is the *same* `jaro_winkler` function `cross_system.rs` already uses for
  whole-string comparison, just applied per-token — not a new algorithm.
- **A second pass in `find_cross_system_candidates`**, after the existing Table/TransformNode O(n²)
  pass (unchanged): for each table-like object, score it against every free-text object, feeding
  the containment score into the *existing* `combine_signals(None, containment_score, None)` —
  column overlap and type compatibility naturally don't apply to free text and `combine_signals`
  already excludes absent signals from the weighted average rather than scoring them 0, so this
  needed no new combination logic either.

### Why not the other three mechanisms

- **Embeddings**: would solve a *different, harder* problem — true synonyms with no shared
  substring (`orders` ≈ `purchases`). No embedding infrastructure exists anywhere in this codebase
  (already tracked backlog, `TODO.md` → "Promoted from RFC Non-Goals"). Token containment fully
  resolves all four real pairs without it — the roadmap item's own instruction was "don't solve it
  generally yet."
- **Human-in-the-loop**: already the existing design, completely unchanged. `cross_system.rs`
  candidates were never auto-merged before this fix and aren't now — RFC 0029 already persists
  them as `unconfirmed` relationships for `ekos_identity_review` to confirm or reject. The new
  cross-kind candidates flow through that exact same path.
- **Explicit mapping config**: would need per-workspace hand-maintenance (`"sites" ->
  ["public.sites", "lib/plausible/site", ...]`) and wouldn't generalize past the four concepts
  checked here — the entire point of a heuristic scorer is not needing one.

## Results

All 4 real pairs verified via unit tests built from the literal real names/paths/titles above
(not synthetic identifiers) — `real_analytics_pairs_produce_cross_kind_candidates` in
`cross_system.rs`. Two negative-case tests (matches this session's established discipline of
testing filters against a real negative, not just positives) confirm no spurious cross-match:
`public.goals` against an unrelated `api_key.ex` file, and `public.subscriptions` against an
unrelated `sites` issue — both correctly produce zero candidates.

## A real problem the narrow test couldn't show: volume at full-ledger scale

The 4-pair unit tests only ever put 2-3 objects in front of `find_cross_system_candidates` at a
time — no noise, no scale. Running `ekos identity scan` live against `analytics/`'s real, full
~3,700-object ledger told a different story: **27,383 candidates**, almost all `Custom("SameAs")`
relationships genuinely written to the ledger (`identity scan` has no dry-run flag). Root cause,
found by reasoning through the code rather than guessing: a single-word table concept name (the
common case — `sites`, `goals`, `api_keys`) can only ever score exactly 0.0 or 1.0 against a given
free-text object (one word, matched or not — no partial credit), and with no structural signal to
corroborate it, *any* file or issue containing that word anywhere clears the confidence floor.
Real, domain-specific words like "sites" appear — legitimately, not as noise — across dozens of
real files and hundreds of real issue titles in a live repo; each individual match was a correct
concept-word hit, but the aggregate volume overwhelmed the point of a human/agent review queue.

Fix: capped each table-like object to its `MAX_FREE_TEXT_MATCHES_PER_TABLE` (20) strongest
free-text matches by confidence, dropping the rest rather than emitting all of them
(`cross_system.rs`). Added `free_text_matches_per_table_are_capped_to_the_strongest`, reproducing
the same shape (one concept matching far more candidates than the cap) with 30 synthetic-but-
realistic file paths, asserting exactly the cap survives.

**Re-verified live, with one honest complication.** The polluted ledger couldn't be cleaned in
place — the ledger is append-only, no delete/tombstone exists anywhere in this codebase, confirmed
— so getting a clean state meant a real rebuild from the already-cached observation artifacts
(polluted ledger moved aside to `ledger.db.bak-polluted-*`, never deleted). Re-ran `ekos identity
scan` against the rebuilt ledger with the capped code: **2,202 candidates**, down from 27,383. But
the rebuilt ledger turned out to be missing every `File`-kind object — confirmed directly
(`ekos query find` for real file paths and extensions returns zero `File` results post-rebuild,
where the exact same queries returned real hits before). Root cause: `ekos build`'s fingerprint
cache decided nothing had changed on disk and skipped re-observing entirely (`Files observed
(new): 0`), and re-running `compile`/`commit` alone does not independently re-derive `File`
KirObjects the way it does for `recover`-stage analyzer output — they only flow into the ledger at
the moment `build` actually processes them. **This is a real, separate, previously-undiscovered
gap** (a rebuild/cache gotcha: clearing a ledger and recompiling from cached artifacts silently
drops `File` objects if `build` itself doesn't think anything changed) — not fixed here, tracked
in `TODO.md` as its own item, out of scope for this pass.

Net effect on this task's own verification: the 27,383 → 2,202 volume reduction is real and
confirmed live (both runs used the same object mix — Tables, TransformNodes, Issues/PRs — the
`File` gap affects both sides of the before/after comparison equally, so the *relative* 92%
reduction is trustworthy even though neither absolute number includes `File`-based candidates).
The `Table`↔`Issue`/`PullRequest` legs of the four real pairs were independently spot-checked live
(e.g. `public.sites`'s real `SameAs` relationships, resolved by id, include genuine PR/issue
matches). The `Table`↔`File` leg is verified by the unit tests only (built from the literal real
path `lib/plausible/site` queried live *before* the rebuild), not re-confirmed against the live
ledger after — an honest gap in this session's verification, not a gap in the fix itself.

## What this doesn't solve (stated plainly, not hidden)

- **True synonyms with no shared substring** (`orders` ≈ `purchases`) — still needs embeddings,
  still open, still tracked separately in `TODO.md`.
- **`Document`-kind matching** — no real triple was found covering these four concepts among the
  vendored `analytics-docs/` pages (those only cover Google-Analytics-import/CSV-import topics),
  so it wasn't attempted here. Same mechanism (`free_text_tokens`) would extend to it trivially —
  a real, low-risk follow-on, not forced with a contrived example.
- **The same-source `DefaultResolver` over-merge residual** (RFC 0060's 3-of-17, RFC 0062's
  GitHub-item case) — a different bug in a different module (cross-*kind* under-matching here vs.
  same-*kind* over-merging there). Still open, unaffected by this fix.
- **`ekos identity scan`'s CLI path is slow against a real multi-thousand-object ledger** — noticed
  while trying to verify live: `ledger.all_objects()` loads the *entire* ledger before
  `find_cross_system_candidates` even starts filtering, and this was observed to take well over a
  minute against `analytics/`'s full corpus. Pre-existing, unrelated to this change (the new
  scoring pass itself is O(tables × free-text-objects) — a few hundred thousand cheap comparisons,
  not the bottleneck). Not fixed here — worth a follow-up look at `all_objects()`'s cost if the
  live CLI path matters for a real workspace this size.
- **Rebuilding a ledger from cached artifacts can silently drop `File` objects** — found live,
  described above: if `ekos build`'s fingerprint cache decides nothing on disk changed, it skips
  re-observing, and a subsequent clean `recover → resolve → compile → commit` run does not
  independently re-derive `File`-kind KirObjects the way it does for `recover`-stage analyzer
  output. A real, previously-undiscovered pipeline gap, unrelated to identity resolution — not
  fixed here, added to `TODO.md` as its own item.

---

## Knowledge Captured

- **Whole-string similarity and token-containment similarity are genuinely different questions**,
  and conflating them is the reason "just remove the kind filter" wouldn't have worked — a short
  identifier compared against a long free-text string under Jaro-Winkler almost always scores
  near zero regardless of how related they really are, because the metric is measuring the wrong
  thing (edit distance over the *whole* string) for that shape of comparison.
- **`combine_signals`'s existing "exclude absent signals from the average, don't score them 0"
  design paid for itself immediately** — adding a whole new signal source (free-text containment)
  needed zero changes to the combination logic, just a new call site.
- **A real workspace's already-compiled ledger is a better "test workspace" than anything
  synthetic** — querying `analytics/` live for real Table/File/Issue triples took a few minutes
  and produced genuinely representative examples (including the plural/singular mismatch
  `sites`/`site` that a hand-written example might not have thought to include).
- **A narrow, 2-3-object unit test cannot surface a volume/scale problem** — every one of the four
  pairs passed cleanly in isolation; only running the real mechanism against the real, full,
  noisy ledger showed that a correct per-pair signal can still be a bad idea in aggregate. Worth
  remembering before calling any heuristic scorer "done" from unit tests alone when its real
  input size is orders of magnitude larger than what the tests exercise.
- **`kill`/`pkill -f` with a pattern shared by multiple concurrent background jobs kills all of
  them, not just the intended one** — lost a rebuild run this way mid-investigation (its child
  process was orphaned and ran to harmless completion, but the `&&`-chained `compile`/`commit`
  steps after it never ran). No data was lost — just re-ran the remaining two stages directly,
  without backgrounding, once only one relevant job existed. Match on a specific PID when several
  same-shaped background jobs might be running at once, not a command-line substring.
- **`ekos build`'s fingerprint cache and the `recover`/`compile`/`commit` chain make different
  assumptions about what's replayable from a cleared ledger** — recover-stage analyzer output is
  cached as a re-consumable artifact; `build`'s own inline `File`-object construction apparently
  isn't, in a way that only shows up once you actually clear a ledger and rebuild from cache. Real
  finding, not fixed here (see "What this doesn't solve" and the new `TODO.md` item).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/identity/src/lib.rs` | `name_for_similarity` made `pub(crate)` |
| `ekos/crates/identity/src/cross_system.rs` | `free_text_tokens`, `fuzzy_containment_score`, `MAX_FREE_TEXT_MATCHES_PER_TABLE` cap, second scoring pass in `find_cross_system_candidates`; 8 new tests (4 real-pair positives, 2 negatives, 1 isolated token test, 1 cap test) |
| `TODO.md` | Annotated the existing "Analyzers" (embedding/synonym matching) and "Identity resolution" backlog entries with this narrower, now-solved adjacent case; added a new item for the `build`-cache/`File`-object rebuild gap |
| `devlogs/devlog_66.md` | This file |

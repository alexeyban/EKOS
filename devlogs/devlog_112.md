# Devlog 112 — Four real, previously-undiscovered bugs found self-analyzing EKOS's own repository

**Date:** 2026-08-26
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

User asked EKOS to generate comprehensive documentation of its own repository — "act as data
architect/tech writer, fix all errors/issues/gaps end-to-end." Running the full pipeline
(`build`/`recover`) against EKOS's own real, long-lived self-analysis ledger surfaced 3 real parse
failures (`RUST003`) that traced back to 4 distinct, previously-undiscovered bugs, each found live
by chasing the actual root cause rather than accepting the first plausible explanation. All 4
fixed, tested, and live-verified against a fully fresh rebuild of EKOS's own repository (687 files,
0 parse warnings).

## Bug 1: artifact ids computed from pre-redaction content, data stored is post-redaction

`ObservationArtifact::new()` (inside each observer's own `scan()`) computes a content-addressed id
from the *raw* data it's given — but `build.rs`'s central redaction pass (RFC 0043) mutates that
same `data` *after* the id is already fixed. `PackArtifactStore::write()` is skip-if-exists, never
an overwrite. Net effect: whatever the redaction engine produced the *first* time a file's raw
content was ever observed gets locked in under that id forever — any later fix to the redaction
engine can never retroactively re-redact it, since the same unchanged raw content always re-derives
the same pre-redaction id and `write` sees "already have this."

**Fix:** `build.rs` recomputes `artifact.id` from the *final*, already-redacted `content` right
before writing — a redaction-logic change that alters the output for otherwise-unchanged raw source
now naturally produces a new id, so a real, fresh artifact actually gets persisted instead of
silently resolving to a stale one. New test:
`a_later_redaction_pattern_addition_actually_re_redacts_unchanged_source` (adds a real `[security]`
custom pattern between two builds against the same unchanged file, asserts the artifact store's own
persisted content — not the ledger's `File` object, which is rebuilt fresh every run regardless and
can't expose this class of staleness — reflects the new pattern).

## Bug 2: every artifact collector except one passed every historical duplicate through

A direct, immediate consequence of fixing bug 1: once a real target can legitimately have more than
one artifact (an old stale one plus a fresh one), `recover.rs`'s `collect_rust_artifact_ids` (and 10
sibling functions — python, elixir, javascript, github, clickhouse, confluence, localdocs, pentaho,
git) blindly passed *every* artifact for the matching connector through, with no deduplication by
target at all. `collect_crypto_artifact_ids` alone already had this fix (found by someone earlier,
never generalized — the same "fixed once, still broken in every duplicate copy" pattern this
session has hit repeatedly with other bugs). A stale, permanently-broken artifact got reprocessed —
and re-warned-about — on every single future `recover` run, forever, no matter how many times the
underlying content bug got fixed.

**Fix:** new shared `collect_artifact_ids_for_connector(store, connector_name)`, all 11 collectors
reduced to a one-line call into it. Deduplicates by target using `ArtifactMeta.created_at` (a real
RFC3339 timestamp every artifact already carries) — not "whichever `store.list()` happens to insert
last," which has no relationship to recency at all (`PackArtifactStore::list()`'s order comes from
segment/offset position) and was empirically confirmed picking the *stale* version on a real run.
`collect_git_artifact_ids` (a different return shape — commits plus one repo artifact) is reduced to
call the shared helper too, with `target == "repo"` pulled out separately. 2 new tests, one for the
shared dedup-by-recency behavior, one confirming the git repo/commit split still works with real
duplicate commit shas.

## Bugs 3-5: three independent bugs in the same regex, found by tracing each real parse failure to its actual cause

`redaction.rs`'s own `generic-assigned-secret` pattern (`api_key|secret|password|...` = value) had
three separate, real defects, each found by refusing to stop at the first plausible-looking
explanation:

- **Bug 3 — asymmetric quote consumption.** `['"]?value['"]?` matched each quote *independently*.
  A value with no leading quote (`redact("api_key=1.2.3.4-not-an-identifier", ...)` — this file's
  own `redacts_ip_like_value_that_is_not_a_dotted_identifier` test) could still have the *trailing*
  `['"]?` consume a real, syntactically-necessary closing quote sitting right after it. The
  replacement never restored what it ate, silently deleting the file's own closing `"` and
  swallowing everything after it into one unterminated string literal. Fixed: an explicit
  alternation — `"value"` (both quotes, one unit) or `'value'` (both single) or bare `value` (none)
  — never one quote without its pair, since the `regex` crate has no backreference support to
  express `(['"]?)...\1` directly (non-backtracking DFA engine).
- **Bug 4 — no word-boundary guard.** The label alternation matched as a bare *substring* inside a
  longer real identifier — `api_secret: "consumer-secret".to_string()` matched starting mid-
  identifier at `secret` (the `api_` prefix was never part of the match), leaving
  `api_[REDACTED:...].to_string()` — `syn` parses `api_[...]` as an array-index expression on
  identifier `api_`, not a struct-literal field, and fails. A first attempt (plain `\b` on both
  sides) overcorrected: `\b` never fires between two word characters, so it stopped `api_secret`/
  `access_token_secret` from matching *at all* — real compound field names ending in `secret`/
  `password` are exactly as real a target as the already-explicit `api_key`/`access_key` compounds
  in the same list. Fixed properly: `(?:[A-Za-z0-9]+[_-])*` consumes zero or more real leading
  `word_`/`word-` segments as part of the match itself, so a real multi-segment identifier like
  `access_token_secret` matches its whole self, never a fragment.
- **Bug 5 — replacing the whole match, not just the value.** Even a fully "correct" match (whole-
  word label, symmetric quotes, valid value) still deleted the real field name and its separator —
  `api_key: "consumer-key".to_string()` became a bare `[REDACTED:...]` where Rust's struct-literal
  grammar required `field_name: value`. The original design ("we don't need to preserve
  `password:` text, over-redacting is safer than under-redacting") was a real, deliberate decision,
  now empirically proven wrong specifically for source-code contexts: over-redaction that deletes
  required syntax isn't safer, it's a worse failure than under-redaction. Fixed: only the captured
  value's own span gets replaced now — the real field/env-var name, separator, and any real quote
  character on either side stay untouched verbatim, so the result is always a drop-in replacement
  for the secret-shaped text alone, never the surrounding structure.

4 new tests total across bugs 3-5, plus all 11 pre-existing `redaction.rs` tests still pass
unmodified (the whole-match-vs-value-only change is compatible with every existing assertion, which
only ever checked "the real secret is gone" and "a placeholder is present," never that the label
also vanished).

## A one-time historical-data remediation, not a further code bug

After bugs 1-5 were fixed, 2 files (`plugins/clickhouse/src/lib.rs`, `crates/clickhouse-query/
src/client.rs`) still reported `RUST003` on a rebuild that only cleared the ledger. Traced
precisely: today's redaction correctly leaves these files' real `password: password.into()`/similar
lines untouched (`looks_like_code_reference` already protected the dotted-reference shape) — but
because a *no-op* redaction means post-redaction content equals pre-redaction content, the freshly
recomputed id (bug 1's fix) coincidentally equals whatever a purely-raw-content id would have been
— which is exactly what the *original*, pre-bug-1-fix code used to compute, back when redaction
*wasn't* a no-op for these files and corrupted them. The id matches; `write()` sees "already have
this" and never overwrites the permanently-mismatched legacy (id, corrupted-data) pair. This is a
real content-addressing invariant violation baked into the old data, not a code bug going forward —
new writes can never again produce a mismatched (id, data) pair, and the id will always change
whenever the *actual persisted content* changes. Confirmed with the user and remediated with a full
`.ekos/` reset (the whole cache, not just the ledger — the corrupted entries live in
`.ekos/artifacts`, which a ledger-only clear never touches).

## Live verification

Full-repo scan (`syn::parse_file` against every one of EKOS's own 256+ real `.rs` files, using the
exact same `ekos_common::redaction::redact` function production code calls): 4 files legitimately
changed by redaction, 0 broken — before the fixes, this same scan found 3 real breaks.

Full, from-scratch rebuild of EKOS's own `.ekos/` (a full reset, not just the ledger — confirmed
with the user given the larger scope): `init` → `build` (687 files, real GitHub connector against
`alexeyban/EKOS` enabled) → `recover` (**0 warnings** — all three previously-reported `RUST003`
files now parse clean) → `resolve --force` (2 remaining conflicts, both expected/benign for a real
multi-crate, multi-language workspace this size — `main` appearing as both `RustSymbol` and
`PythonSymbol` across many real crates/scripts, and one real `ObserveError` type/module name
coincidence) → `compile` (4783 objects, 7496 relationships) → `commit` (real AI-Assisted Overviews
via local Ollama).

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups, 8 new tests across the 4 bugs).
`tests/integration` 3/3.

## Knowledge Captured

- **A live pipeline's own long-lived, real self-analysis history is a genuinely different — and
  harder — verification target than a disposable scratch/external project.** Every fix this session
  before this one was verified against either `pdf-reader` or a throwaway scratch scope; this was
  the first time a fix needed verifying against EKOS's *own*, `Aug 9`-onward, real accumulated
  ledger — and it's precisely that accumulated history (real artifacts written under an old,
  already-fixed-elsewhere version of the redaction engine) that exposed bugs 1-2, which a fresh
  scratch project could never have surfaced (nothing there has old enough history to go stale).
- **"The regex now protects the one case I found" is not the same claim as "the regex is now
  correct."** Bugs 3, 4, and 5 are all in the *same* pattern, found one at a time, each only visible
  once the previous one was fixed and a *new* real file surfaced the next failure. A full-repo scan
  against real content (not just the one fixture that motivated each individual fix) was what
  actually confirmed "done," not passing the specific test that prompted each round.
- **Content-addressing's core invariant — same id implies same real content — can be silently
  violated by exactly the kind of bug this session keeps finding: two different code paths (id
  computed one place, data mutated another) that were never actually kept in sync.** Once violated,
  the mismatch is permanent for that specific (id, data) pair; no future code fix can self-heal it,
  only a deliberate reset of the store can. Worth treating "does this id get computed from the exact
  same bytes we're about to persist" as a standing question whenever a content-addressed store's
  write path has more than one mutation step between "compute the id" and "write the data."

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/build.rs` | Artifact id recomputed from post-redaction content, not raw; 1 new test |
| `ekos/crates/cli/src/commands/recover.rs` | New shared `collect_artifact_ids_for_connector` (dedup by target, latest by `created_at`); all 11 collectors reduced to call it; `collect_git_artifact_ids` updated for the new shape; 2 new tests |
| `ekos/crates/common/src/redaction.rs` | Three fixes to `generic-assigned-secret`: symmetric-quote-only alternation, real word-boundary-aware compound-identifier matching, value-only replacement (label/separator/quotes preserved); 4 new tests |
| `.ekos/` (EKOS's own self-analysis ledger) | Full reset and fresh rebuild against all fixes — 687 files, 0 parse warnings |

# Devlog 90 — Two real bugs found while reading generated output for a presentation

**Date:** 2026-08-23
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Asked to add real examples of generated output to the Architecture Document Generation deck.
Picking a real Elixir module's entity page to showcase surfaced a genuine, serious identity-
resolution bug — a real password-hashing module falsely merged with 1,236 unrelated real modules
at confidence 1.00. Chasing the root cause surfaced a second, larger bug: `docs-generated/` was
never excluded from the analytics project's own `[observe] paths`, so every `ekos build` was
re-ingesting EKOS's own previously generated documentation as if it were real project source — a
self-referential contamination loop that had been quietly inflating every object/relationship
count reported earlier this session (RFC 0076 Finding 6 was misdiagnosed as the sole cause of the
ledger's growth; this loop was the dominant one). Both fixed, both verified with a full clean
ledger rebuild.

## Bug 1 — `ElixirModule`/`ElixirSymbol`/`JsModule`/`JsSymbol` missing from the identity resolver's exclusion list

`DefaultResolver`'s blocking step has a documented, recurring failure class (CLAUDE.md's own
crate-map names it explicitly): any `Custom(_)` object kind that's self-identified by a structural
key must be excluded from fuzzy name-similarity blocking, or `structural_score`'s same-kind 1.0
fallback (no `columns` property to differentiate on) pushes unrelated objects sharing any name
prefix/suffix over the merge threshold. `Section`, `TransformNode`, `RustSymbol`/`RustModule`,
`PythonSymbol`/`PythonModule`, `Crate`, `Claim`, `ArchitectureGap` had all already hit this and
been added to the exclusion list (`crates/identity/src/lib.rs`). RFC 0081 (Elixir) and RFC 0085
(JS/TS) both introduced new `Custom(_)` kinds this session and neither was added — a real gap in
work already reported as "done" earlier in this same conversation.

Found by reading `docs-generated/entities/elixirmodule/pl/plausible-auth-password.md` — a real
password-hashing module — directly, not by running a test suite. It showed an 18-times-duplicated
`SameAs` relationship at confidence=1.00 to `PlausibleWeb.Plugins.API.Schemas.Funnel.CreateRequest`,
a completely unrelated real module. Investigating the live CKM directly (not just the stale
committed ledger) found the real scale: that one canonical `ElixirModule` object had 1,236 real
`SameAs` edges to unrelated real modules.

Fixed by adding all four kinds to the exclusion list, with the same reasoning comment pattern
every prior addition already uses. Two new regression tests use the exact real names from the bug
(`Plausible.Auth.Password`, `PlausibleWeb.Plugins.API.Schemas.Funnel.CreateRequest`), plus one
covering the other three kinds proactively.

## Bug 2 — `docs-generated/` re-ingested as real source on every build

Re-verifying the fix against a fresh `compile` kept showing the exact same bad relationship,
unchanged, no matter how many times pass-manifests were invalidated and recompiled — a real,
confusing signal that something deeper was wrong. Grepping the fresh CKM for the erroneous evidence
string found it embedded inside a real `Custom("Section")` object's `excerpt` property, sourced
from `bleweb-texthelpers.md: section 2` — a real markdown file under `docs-generated/entities/`
that had itself quoted the (buggy) `SameAs` relationship text verbatim, because `docs-generated/`
was never added to `ekos.toml`'s `ignore-patterns`. Every `ekos build` after every `ekos docs
generate` this session had been feeding the project's own generated output back into
`LocalDocsObserver` as if it were real project documentation — explaining why "Local documents
analysed" climbed from 237 to 6,364 across this session's phases, and why the ledger's object count
kept growing (127,676 objects at its worst) even on runs where no analyzer code changed.

Fixed with one line: `"docs-generated"` added to `ignore-patterns`. Verified by a full clean
rebuild (`.ekos/` moved aside, `init` → `build` → `recover` → `compile` → `commit` →
`docs generate`, all from scratch): 2,414 real files observed (not 6,128), 139 real local
documents (not 6,364), 8,787 real CKM objects (not 127,676) — the true, uncontaminated real size
of this project's compiled knowledge.

## Also hit, again: the stale-binary trap

`cargo fmt` reformatted `crates/identity/src/lib.rs` after the fix was written; the release binary
built before that reformat was silently one commit behind. Confirmed via `stat` mtime comparison
(the same diagnostic already used twice earlier this session for the exact same class of issue) —
the "fixed" binary was still producing the 1,236-edge bug on a fresh compile. `touch` + rebuild
resolved it; re-verified directly against the CKM (0 bad edges, not just "tests pass") before
trusting the fix.

## Live verification

Direct CKM inspection (not just re-reading generated docs, which can't prove a negative on their
own): `PlausibleWeb.Plugins.API.Schemas.Funnel.CreateRequest`'s `SameAs` edge count went from 1,236
to 0. Total `SameAs` relationships in the whole ledger dropped from an inflated count to 148, all
of which are pre-existing, already-documented residual fuzzy-match categories (`Document`/
`Document`, `Technology`/`Technology`, `Pipeline`/`Pipeline` — RFC 0060's own known territory), not
new bugs. `Plausible.Auth.Password`'s own entity page is now exactly 3 real `Contains` relationships
and nothing else. Real Backend/Frontend/Database numbers (System Decomposition) were unaffected by
either bug and match every earlier phase's own verification: 1232 Backend files, 324 Frontend
files, 57 SQL tables.

## Knowledge Captured

- **A generated-documentation output directory must always be excluded from that same project's
  own `[observe] paths`**, not just as a convention but as a real correctness requirement — a
  self-referential ingestion loop doesn't just waste time re-scanning, it can feed a pipeline's own
  past mistakes back into itself as fresh "evidence." Worth checking for this on every new
  `ekos.toml` this project's own tooling creates for a target repo, not just this one.
- **A confidence-1.00 identity match is not automatically trustworthy** — this bug's matches all
  scored the maximum, because `structural_score`'s same-kind fallback of 1.0 is a *ceiling on
  ignorance*, not evidence of a real match. A perfect score with an empty properties comparison is
  itself a signal to look closer, not a green light.
- **Reading one real generated page directly found a bug an entire test suite, six RFCs, and a
  full workspace gate all missed** — every unit test for RFC 0081/0085 used small, clean, isolated
  fixtures; none exercised cross-kind interaction with the identity resolver at real project scale.
  Live verification against messy real data remains this project's single highest-value practice,
  reconfirmed a third time this session.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/identity/src/lib.rs` | `ElixirModule`/`ElixirSymbol`/`JsModule`/`JsSymbol` added to the blanket kind-exclusion list; 3 new tests |
| `CLAUDE.md` | Crate-map note updated with the two new kinds and the "check this list on every new `Custom(_)` kind" reminder |
| `/home/legion/PycharmProjects/analytics/ekos.toml` | `docs-generated` added to `ignore-patterns` |
| `devlogs/devlog_90.md` | This file |

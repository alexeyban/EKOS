# Devlog 37 — RFC 0036/0037 Phase 2: real Pentaho smoke tests, three real bugs found and fixed

**Date:** 2026-08-07
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Ran Phase 2 of both RFC 0036 (Pentaho → dbt export) and RFC 0037 (curated documentation set)
against the same real cloned Pentaho ETL repo used throughout RFC 0035's testing — the first time
either feature was exercised against a large, real, messy compiled workspace (198 objects: 98
Transformation IR nodes, 21 files, 74 PDF-derived sections, 3 documents, 1 person, 1 table) rather
than small hand-built fixtures. Both runs completed without panics, but real output surfaced three
genuine bugs — two cosmetic-but-honesty-relevant wording issues, one a real violation of RFC
0037's own stated design goal ("never one giant unreadable diagram"). All three fixed with
regression tests before this devlog was written, matching the project's established "real data
finds what unit tests can't" pattern from RFC 0035/0036 Phase 1.

---

## RFC 0036 Phase 2 — real Pentaho dbt export

### What was run

`ekos dbt generate` against the real Pentaho repo's already-committed ledger: 98 real dbt models
rendered, 23 real source tables identified. No panics, no missing files, `ref()` chains resolved
correctly end to end including through real `Unmapped` steps.

### Bug found: confusing comment on the no-keys join fallback

A real Pentaho `StreamLookup` step compiled with an empty `keys` list. The `Join` renderer's
no-keys fallback (`on true -- no join keys compiled`) still had the join renderer's blanket
`-- TODO: verify column qualification, source dialect: Pentaho` comment appended after it,
producing `on true -- no join keys compiled -- TODO: verify column qualification, source dialect:
Pentaho` — nonsensical, since there's no key expression to verify a qualification for. Fixed by
moving the verify-comment inside the `keys.is_empty()` branch so it only attaches to real key
text; a real join with real keys was spot-checked afterward
(`on s.dim_product_id = p.dim_product_id -- TODO: verify column qualification...`) and renders
correctly. Regression test extended in `ekos-dbt-gen`.

---

## RFC 0037 Phase 2 — real Pentaho curated documentation

### What was run

`ekos docs generate --layout curated` against the same real ledger: all four files (`README.md`,
`Architecture.md`, `API.md`, `SequenceDiagrams.md`) written successfully, 198 objects considered.

### Bug found: the diagram-size problem the RFC explicitly set out to avoid, not fully solved

RFC 0037's design explicitly excluded `Custom("FeedsInto")` edges from `Architecture.md`'s
dependency graph specifically to avoid an unreadable diagram (86 `TransformNode`s in this same
repo was the stated real number). That exclusion wasn't sufficient: `Contains` edges — from real
PDF pages/slide sections, produced by `local_docs_analyzer` — numbered 75 in this one relationship
kind alone, rendering a 76-node Mermaid graph and pushing `Architecture.md` to 189 lines. The
design had special-cased the one kind known in advance to be large (`FeedsInto`) but not the
general case: *any* relationship kind can have too many real edges to render usefully.

Fixed with a per-kind size cap (20 edges, chosen as a round number well above what any of the
small structural kinds in real test data produced): a kind over the cap renders one honest
sentence — "_75 `Contains` relationships compiled — diagram omitted, too large to render
usefully. See `ekos docs generate --layout objects` for per-object detail._" — instead of the
graph. `Architecture.md` dropped from 189 to 34 lines on the real repo. Regression test added
covering exactly this shape (50 `Contains` edges from one document to 50 sections).

### Bug found: "single step" placeholder wrong for multi-node pipelines with no edges

`.kjb` job-orchestration files are always `Unmapped` by design (RFC 0027 — job entries are never
modeled as data transformations). A real `DimensionsJob.kjb` compiled to 8 `TransformNode` objects
in one origin group, correctly with zero `FeedsInto` edges between them (nothing wires job entries
together). `SequenceDiagrams.md`'s no-edges placeholder unconditionally said "_(single step — no
`FeedsInto` edges compiled for this pipeline)_" regardless of how many participants existed —
wrong for 8 participants. Fixed to report the real count with correct singular/plural wording
("_(8 steps — no `FeedsInto` edges compiled for this pipeline)_"). Regression test added covering
the multi-node, zero-edge case explicitly (the existing single-node test alone couldn't have
caught this).

### What rendered correctly on the first real run

`README.md`'s component counts and real contributor (`Joseph Higaki (1 commits)`, from
`git_analyzer`'s real authorship data); `Architecture.md`'s ER diagram section (correctly showing
"_No table foreign-key relationships compiled._" — this repo has exactly 1 `Table` object, no
`ForeignKey` edges, which is honestly true, not a bug); `SequenceDiagrams.md`'s 14 real pipeline
sections (one per `.ktr`/`.kjb`/SQL-script origin) with correctly-chained `FeedsInto` messages
where edges did exist; `API.md`'s empty-state placeholder (correctly triggered — this repo has no
source files with harvested `symbols`, only ETL job files and documents).

---

## Knowledge Captured

- **"Exclude the one relationship kind known to be large" is not the same as "cap diagram size."**
  RFC 0037's original design reasoned from the one number it already knew (86 `TransformNode`s /
  `FeedsInto` edges from RFC 0035's own prior test) rather than the general principle (*any* kind
  can have too many edges in a real, messy workspace). The fix generalizes correctly: a size cap
  applied per relationship kind, not a kind-specific exclusion list. Future diagram-rendering code
  in this codebase should default to a general size guard, not enumerate known-large cases.
- **"Single X" wording bugs hide behind small hand-built test fixtures.** Both `docs-gen` unit
  tests that existed before Phase 2 used exactly one node for the "no edges" case, so "single
  step" always happened to be correct in-suite. A workspace-scale real test (8 participants, one
  `.kjb` job) is what caught the plural-form bug — worth remembering when writing "empty state"
  tests generally: cover more than the n=1 case if the message contains a count-dependent word.
- **Real Pentaho `.kjb` job entries producing multiple `Unmapped` nodes with zero internal edges
  is expected, real behavior**, not a gap — RFC 0027 already designed job-orchestration entries to
  never be wired together. Any future renderer over `TransformNode` data should expect this shape
  (many nodes, one origin, zero edges) as a normal case, not treat it as anomalous.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/dbt-gen/src/lib.rs` | Fix: verify-comment only attaches to real join key text, not the no-keys fallback; regression test extended |
| `ekos/crates/docs-gen/src/lib.rs` | Fix: per-relationship-kind size cap on `Architecture.md`'s dependency graph (20 edges); fix: `SequenceDiagrams.md`'s no-edges placeholder reports the real step count with correct singular/plural wording; 2 new regression tests |
| `ekos/docs/rfcs/0036-pentaho-to-dbt-export.md` | Phase 2 marked DONE with real findings |
| `ekos/docs/rfcs/0037-curated-documentation-set.md` | Phase 2 marked DONE with real findings |

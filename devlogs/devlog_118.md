# Devlog 118 — RFC 0101: closing a gap by reading the skill file that motivated it

**Date:** 2026-08-26
**PRs:** RFC 0101
**Branch:** main (direct)

---

## Summary

RFC E of this session's Runtime/Retrieval gap-closure plan (the sixth and final item — A/`devlog_113`
EKL, B/`devlog_114` read-only `FactLedger`, C/`devlog_115` streaming, D/`devlog_116` multi-turn
history, F/`devlog_117` `ai_overview` search, done out of order because it turned out cheaper than
planned). `docs/GAP_ANALYSIS.md` had flagged this as blocked on a real, unresolved design question:
no `memory/` path convention could be found anywhere in the `ekos/` source tree. It turned out the
search just hadn't looked in the right place — the convention is real, documented, and in daily use,
in `.claude/skills/memory` and this repo's own real estate-root `ekos.toml`.

---

## RFC 0101 — Structural search boost for `memory/`-path content

### Problem / motivation

RFC 0014 (2026-07-17) explicitly deferred this: *"bm25 name-weighting plus the keyword-slug
convention already privileges notes; revisit with real usage."* Real usage has existed the whole
time — the memory skill's own workflow — but nobody had gone back and read the skill definition (or
the real, in-production `/home/legion/PycharmProjects/ekos.toml`) to find out what it actually
established as the convention.

### What was built

| Component | Change |
|---|---|
| `KirObject::is_under_memory_path` | Detects both real `[observe] paths` shapes this codebase supports (multi-project via `"project" == "memory"`, single-path via a literal `"memory/"` prefix on `"path"`) |
| `SearchIndex` (tantivy, `FactLedger`) | New `memory_path` field; an unconditional 5× boost `Should` clause alongside existing per-term `Must` clauses |
| `FactLedger::index_object` | Passes the real detection result to `SearchIndex::upsert` |

### Implementation details worth remembering

- **The "unresolved design gap" from the original plan was a research gap, not a design gap.** The
  actual convention (`$WORKSPACE_ROOT/memory`, `<scope>--<type>--<keywords>.md` filenames, real
  markers like `--lesson--`/`--decision--`) is fully specified in `.claude/skills/memory/SKILL.md`
  and demonstrated by this repo's own real, currently-used `/home/legion/PycharmProjects/ekos.toml`
  (`paths = ["memory", "analytics", ..., "EKOS", ...]`). No design decision was actually needed —
  just reading the skill file and the real config that already uses it, which the original
  gap-analysis pass never did (it searched `ekos/`'s own source tree, not the repo's `.claude/`
  directory or files outside the workspace root).
- **Two real, both-currently-valid `[observe] paths` shapes needed handling, not one.** A
  multi-project workspace (this repo's own real estate config) puts `memory`'s own name in RFC
  0079's `"project"` property and never puts a `memory/` prefix in `"path"` at all; a hypothetical
  single-path workspace with an internal `memory/` subdirectory has no `"project"` property and
  needs the prefix read from `"path"` directly. Missing either shape would have made the whole
  feature silently inert for one of the two real configurations it needs to support — checked and
  tested for both, not just the one this repo happens to use today.
- **The boost had to be structured as an unconditional `Should` clause outside the per-term `Must`
  array, not folded into the per-term boost list next to name/kind/content.** Those three are
  evaluated per query *term*; "is this whole document under memory/" is a document-level property,
  independent of which term matched. Getting this wrong (e.g. adding it inside the per-term loop)
  would have either applied the boost once per matched term (double- or triple-counting it for a
  multi-word query) or, worse, risked changing which documents match at all rather than just their
  ranking — verified explicitly with a test that a memory-path document *not* matching the query
  terms still correctly produces zero results, not a false positive.

### Decisions (alternatives considered, why this choice)

- **Fixed, hardcoded `"memory"` convention, not an `ekos.toml`-configurable glob** (the pattern RFC
  0083's `[[architecture.system-decomposition.overrides]]` established elsewhere in this codebase).
  Rejected for v1: there's exactly one real, established convention this RFC serves, `ekos-kir` (the
  crate `is_under_memory_path` lives in) has no dependency path back to `ekos.toml`'s config types to
  read a pattern from without a real architecture change, and every sibling function right next to
  it (`indexed_content()`'s own `excerpt`/`ai_overview`/etc. property-key reads) is equally
  hardcoded, not configurable. Matching the existing architecture beat generalizing ahead of a real
  second use case.
- **`FactLedger` only, not the SQLite backend.** The SQLite backend's FTS5 ranking has no per-document
  boost-query primitive the way tantivy does — adding the equivalent there needs its own real schema
  migration, disproportionate scope for a backend RFC 0016 already scoped down to "keeps serving
  pre-existing, never-migrated workspaces only." Matches RFC 0097's own precedent for the same
  backend-scoping call.

---

## Knowledge Captured

- **A "no such convention exists" finding from a full-repo *source-code* search doesn't mean the
  convention doesn't exist — it might live in a skill definition, a real config file, or a planning
  document outside the code tree entirely.** This gap sat "blocked" in the tracked backlog for over
  a month (RFC 0014 → this session) specifically because the earlier survey's search scope was too
  narrow, not because the underlying question was genuinely unanswerable. Worth checking
  `.claude/skills/`, real `ekos.toml` files outside the immediate project, and `archive_plans/`
  before concluding a "vague backlog item" genuinely has no concrete answer yet.
- **This repo's own real, in-production estate `ekos.toml`
  (`/home/legion/PycharmProjects/ekos.toml`) is itself a valuable source of ground truth for how
  EKOS's own conventions are actually used in practice** — more reliable than reasoning abstractly
  about what a convention "should" look like. Worth checking directly the next time a real-world
  usage pattern needs grounding, the same way this session already leaned on this repo's own
  self-analysis ledger for other RFCs' live verification.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/lib.rs` | New `KirObject::is_under_memory_path`; 4 new tests |
| `ekos/crates/ledger/src/search.rs` | New `memory_path` field; unconditional boost `Should` clause in `query()`; `upsert()` gains `is_memory_path` |
| `ekos/crates/ledger/src/fact_ledger.rs` | `index_object` passes the real detection result through; 2 new tests |
| `ekos/docs/rfcs/0101-memory-path-search-boost.md` | New RFC |

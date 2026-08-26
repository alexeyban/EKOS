# RFC 0101 — Structural search boost for `memory/`-path content (RFC E)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC E of this session's Runtime/Retrieval gap-closure plan. `docs/GAP_ANALYSIS.md`'s survey found
this named as backlog (RFC 0014's own Non-Goals: *"Boosting `memory/` paths structurally — bm25
name-weighting plus the keyword-slug convention already privileges notes; revisit with real
usage"*), but with no concrete scope — a full-repo search at the time found zero references to a
`memory/` convention anywhere in the codebase, and the plan flagged this as a real, unresolved
design gap blocking implementation.

It wasn't unresolved because the convention doesn't exist — it's real, and has been in daily use
since RFC 0014 shipped (2026-07-17): `.claude/skills/memory` and `next_steps.md`'s "Personal Memory
OS" vision both establish `$WORKSPACE_ROOT/memory` as a real directory of `<scope>--<type>--
<keywords>.md` notes (`global--lesson--fabric-capacity-pause.md`, etc.), and this repo's own real,
in-production `/home/legion/PycharmProjects/ekos.toml` observes it as a literal `[observe] paths`
entry alongside every other project. The gap was that nothing had gone and read that skill file and
that real config to ground the design in — the survey's "full-repo search" only checked the `ekos/`
source tree, not the skill definitions or the actual estate config that uses them.

## Design

### `KirObject::is_under_memory_path` — two real config shapes, one check

A file's `memory/`-ness has to be detected correctly under both real `[observe] paths` shapes this
codebase supports, not just one:

- **Multi-project** (this repo's own real estate `ekos.toml`: `paths = ["memory", "EKOS",
  "analytics", ...]`) — RFC 0079's `"project"` property holds exactly `"memory"` (the observe-path
  entry's own relative name), and `"path"` is relative to *that* entry, so it never contains a
  `memory/` prefix on its own.
- **Single-path** (`paths = ["."]` with an internal `memory/` subdirectory) — no `"project"`
  property at all (RFC 0079's own "absent for the single-path case" rule), so `"path"` carries the
  prefix directly.

`is_under_memory_path` reconstructs `[project]/[path]` (using `path` unprefixed when `project` is
absent) and checks the result's *first path segment* — a real segment check, not a substring one, so
`memory-old/x` or `not-memory/x` (real near-miss names worth guarding against explicitly) don't
false-positive.

The convention is a fixed, hardcoded property-key/value check (`"memory"`), matching this codebase's
own established style for the sibling functions immediately next to it — `indexed_content()` reads
`excerpt`/`symbols`/`ocr_text`/`ai_overview`/`ai_usage` by fixed key too, none of it
`ekos.toml`-configurable. `ekos-kir` (where this lives) also has no dependency path back to
`ekos-compiler-core::EkosConfig` to read a config value from even if this were meant to be
configurable — matching the existing architecture, not fighting it.

### `SearchIndex`: an unconditional boost `Should` clause, not a per-term one

A new `memory_path` tantivy field (`STRING`, indexed-only) is set to a fixed present/absent token at
upsert time, from `KirObject::is_under_memory_path()`. At query time, `SearchIndex::query` adds one
extra `Should` clause (`BoostQuery` on a `TermQuery` against that field, weight `5.0` — meaningfully
above the plain content-field weight of `1.0`, matching RFC 0014's own motivating example of a
common-term search drowning the one memory note that should rank first, but below the real
exact-name-match weight of `10.0`, so a memory note's content boost still never outranks something
the query literally named) alongside the existing per-term `Must` clauses.

This is the one real design choice worth naming explicitly: the boost clause sits **outside** the
per-term `Must` array as an unconditional `Should`, which in a boolean query with existing `Must`
clauses only ever adds extra score to documents that *already* satisfy every `Must` clause on their
own — it can never independently qualify a document that fails to match the real query terms. Live
verification confirms both halves of this: a real ranking reorder for documents that do match, and
an empty result set (not a false positive) for a query a memory-path document doesn't match at all.

### Scope: `FactLedger` (tantivy) only, not the SQLite backend

The SQLite backend's FTS5-based ranking (`index_object_fts_v1`/`v2` in `crates/ledger/src/lib.rs`) is
a structurally different mechanism (`bm25()` SQL weight tuples, no per-document boost-query
primitive) — adding the equivalent boost there would need its own FTS5 schema migration (a new
column, a v2→v3 upgrade path), real additional scope for a backend RFC 0016 already deemed legacy
(kept serving only pre-existing, never-migrated workspaces; every new workspace defaults to
`FactLedger`). Deliberately not attempted here, matching RFC 0097's precedent of scoping a real
improvement to the current-default backend rather than doubling the work to also patch the one being
phased out.

## Non-goals

- **A configurable path/glob pattern** (matching RFC 0083's `[[architecture.system-decomposition
  .overrides]]` precedent). Considered and rejected for v1 — the one real, established convention
  this RFC exists to serve is a fixed directory name; a config knob for a single hardcoded value
  nobody has asked to customize is speculative scope, and `ekos-kir`'s own architecture doesn't
  currently have a path back to `ekos.toml`'s config types to read one from anyway.
- **The equivalent SQLite-backend boost.** Named above — real, but disproportionate scope for a
  backend already being phased out by RFC 0016's own default-switch policy.

## Verification

4 new `ekos-kir` unit tests (`is_under_memory_path` correct for both real config shapes; false for
near-miss names like `memory-old/`; false for an ordinary project object). 2 new `ekos-ledger`
regression tests against the real `FactLedger` backend (not just the pure detection function): a
memory-path object ranks above an otherwise-identical ordinary-project object matching the same
query term; the boost clause never widens the result set for a query a memory-path object doesn't
actually match. Full workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D
warnings`, `test --workspace`, 101/101 `ekos-ledger` test groups), `tests/integration` 3/3.

Live-verified against a real scratch workspace mirroring this repo's own real estate `ekos.toml`
shape (`[observe] paths = ["memory", "myproject"]`, both containing a file with the word
"quadratic"): `ekos query find "quadratic"` ranks all three `global--lesson--quadratic-blowup.md`-
derived objects (File, Document, Section) above all three `notes.md`-derived objects, confirmed
through the real CLI, not a unit test in isolation.

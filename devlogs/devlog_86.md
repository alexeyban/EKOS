# Devlog 86 — Real System Decomposition view (RFC 0083), Phase 3 of the docs quality plan

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Third phase of the source-decomposition plan — the deliverable the user actually asked for:
"which components does it have and how do they relate." Phases 1/2 gave real Backend (Elixir) and
Frontend (npm) data; this phase ties that together with the already-real `Table` data into one
top-level `## System Decomposition` view, C4 Container-level, one step inside `## System Context`.

## RFC 0083

New `layer_classification.rs`: a small convention-based `classify_path` (backend/frontend
language extensions, `package.json` as an always-frontend signal, unrecognized extensions
honestly left unclassified rather than guessed). Per the plan's own explicit requirement, a real
`ekos.toml` escape hatch — `[[architecture.system-decomposition.overrides]]`, same
first-glob-match-wins shape `[recover.sql.dialect-rules]` already established for the identical
SQL-dialect problem.

`system_decomposition_graph` groups compiled objects into up to four real nodes (Backend,
Frontend, SQL Database, ClickHouse Database — kept distinct since a real project can use both
databases at once), each labeled with its real count, reusing `render_graph_svg` (RFC 0073)
completely unmodified for both the Mermaid and standalone-SVG output — no new diagram primitive
needed, the C4 Container-level view is just a different set of nodes/edges fed into the same
renderer System Context already uses.

Cross-tier edges are real or absent, never guessed: a `DependsOn`/`ReadsFrom`/`WritesTo`
relationship between two different layers becomes a real edge; when none is compiled (true for
this project today — Backend→Database needs Phase 6's Ecto-repo-config parsing), the diagram says
so explicitly (`%% No real compiled relationship yet connects these layers to each other.`)
instead of drawing a line that isn't backed by evidence.

## Live verification

Pure `docs-gen` rendering over the already-committed real ledger from Phase 2 — no new analyzer,
so no `recover`/`compile`/`commit` re-run needed, just a rebuild and `docs generate`. Real result
against the real analytics project: three real layers — Backend (1232 files), Frontend (324
files), SQL Database (57 tables) — as a small, genuinely readable 568×80px SVG. Direct contrast
with the complaint that started this whole plan: System Context's own diagram for this same
project renders as an unreadable 8296×190px single row of 46 boxes (RFC 0073's still-open finding,
now explicitly scheduled as Phase 4).

## Knowledge Captured

- **A C4 Container-level view for a non-Rust project doesn't need a new diagram renderer** — the
  existing `render_graph_svg`/Mermaid pair (RFC 0073) is generic over any `(node_id, label)`/
  `(from, to)` pair; the only real work was deciding what the nodes and edges *should be* for a
  project with no `Crate` concept, not building new rendering machinery.
- **A layer-classification override table belongs in `ekos.toml`'s existing pattern, not a bespoke
  format** — `[recover.sql.dialect-rules]`'s first-glob-match-wins shape from RFC 0031 transferred
  directly to `[[architecture.system-decomposition.overrides]]` with almost no new design
  thinking required; worth checking for this same shape before inventing a new config pattern for
  the next "workspace-specific override" need.
- **A test asserting "exactly N files, nothing else" needs updating, not just extending, when new
  real conditional output is added** — `generate_curated_writes_exactly_the_four_named_files_and_
  nothing_else`'s single-`Table` fixture now correctly triggers a real `system-decomposition.svg`
  (a lone `Table` is real, honest `Database` layer data); the test's premise was still correct, its
  expected file list just needed to grow by one, matching the sibling System Context SVG tests'
  own established pattern.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0083-system-decomposition-view.md` | New RFC |
| `ekos/crates/docs-gen/src/layer_classification.rs` | New classifier module; 5 tests |
| `ekos/crates/docs-gen/src/lib.rs` | New section + SVG renderer; 21 test call sites updated |
| `ekos/crates/docs-gen/Cargo.toml` | New `glob` dependency |
| `ekos/crates/compiler-core/src/config.rs` | New `[architecture.system-decomposition]` config; 2 tests |
| `ekos/crates/cli/src/commands/docs.rs` | Config wiring, new SVG write; 3 new/updated tests |
| `TODO.md` | Phase 3 marked done |
| `devlogs/devlog_86.md` | This file |

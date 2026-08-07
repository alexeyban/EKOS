# RFC 0037 — Curated Documentation Set (Architecture/API/README/Sequence Diagrams)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

RFC 0035's `ekos docs generate` renders one Markdown/HTML page per compiled object plus an index
and an ER diagram — comprehensive, but not shaped like the documentation set a developer actually
expects for a project: `README.md`, `Architecture.md`, `API.md`, and sequence diagrams. This RFC
adds exactly that four-document pipeline (`Source Code → EKOS → {Architecture.md, API.md, README,
Sequence diagrams}`) as a second, curated output mode over the same compiled data — a different
*shape* of rendering, not a new extraction pass.

Two scoping questions were resolved with the user before design:

1. **Additive, not a replacement.** The curated set ships behind a new `--layout curated` flag;
   today's per-object multi-page output stays the unchanged default (`--layout objects`), so
   nothing already tested or already described in the `generated-documentation.html` deck breaks.
2. **Sequence diagrams render from Transformation IR `FeedsInto` chains, not a code call-graph.**
   Confirmed by grepping the whole `crates/`/`plugins/` tree: `RelationshipKind::Calls` is never
   constructed anywhere — every hit is inside a test fixture. `FeedsInto` (Pentaho/SQL pipeline
   step order, RFC 0027) is the only real *ordered* flow data that exists today, so it's rendered
   honestly as a "data-flow sequence," explicitly labeled as such, never presented as a code call
   sequence that doesn't exist.

Investigated directly against the current source before writing this RFC:

- `ekos/crates/docs-gen/src/lib.rs` (1249 lines) — every function this RFC builds alongside
  (`is_significant`, `ObjectPageModel`, `build_object_page_model`, `render_markdown_object_page`,
  `render_mermaid_graph`, `render_er_diagram`, `render_index_page`, `html_escape`) confirmed at
  its current location.
- `ekos/crates/cli/src/commands/docs.rs` (633 lines) — `generate()`'s exact 3-pass structure and
  every helper it calls confirmed.
- **No real call-graph data exists anywhere** — grepped the whole `crates/`/`plugins/` tree for
  `RelationshipKind::Calls`; every construction site is inside a `#[cfg(test)]` block.
- **No real API surface data exists either** — `ObjectKind::Api`/`ObjectKind::Service` are never
  constructed by any analyzer. The closest real data: `ObjectKind::File` objects
  (`crates/cli/src/commands/build.rs:185-207`) sometimes carry a `symbols` property — bare
  identifier names harvested by a substring scan for `fn `/`def `/`class `/`func `/`interface `
  prefixes (`plugins/file/src/lib.rs:145-177`, capped at 50/file) — names only, no parameters,
  return types, or HTTP verb/path data.
- **Real "Architecture" signal that does exist**: `dependency_analyzer.rs` (268 lines) — a fixed
  substring-match table of 5 external technologies producing `KirObject(Custom("Technology"))` +
  `RelationshipKind::DependsOn` edges from files; `ForeignKey` edges between `Table` objects;
  `CoupledWith` (git co-change, `git_analyzer.rs`); `References`/`Contains` from git/GitHub/
  file-tree recovery.
- **Real "README" signal that exists**: `ObjectKind::Person` objects from `git_analyzer.rs`
  (lines 111-119) with a `commit_count` property.
- No hand-written Architecture.md/API.md/sequence-diagram example exists anywhere in this repo to
  mirror — this RFC defines the target shape itself.

## Scope

A new `ekos docs generate --layout curated` mode producing exactly four Markdown files —
`README.md`, `Architecture.md`, `API.md`, `SequenceDiagrams.md` — from already-compiled ledger
objects/relationships. `--layout objects` (today's per-object behavior) stays the default.

## Non-goals

- Not a new extraction pass — same "read what's already compiled" rule as RFC 0035/0036. No new
  analyzer for real API signatures or a real call-graph is built here.
- Not claiming a code call-graph exists — `SequenceDiagrams.md` is explicit that it renders
  Transformation IR data-flow order, not function/service call order.
- Not HTML output for the curated layout in this phase (Open Question, deferred).

## What already exists and is reused

- `ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind}` — same types every prior
  `docs-gen` renderer reads.
- `render_er_diagram`, `render_mermaid_graph`'s escaping/labeling helpers, `render_index_page`'s
  kind-grouping idiom, `html_escape` — reused as-is, not duplicated.
- `open_store`, `write_page`, `resolve_output_dir` (`crates/cli/src/commands/docs.rs`,
  `crates/cli/src/commands/store.rs`) — same plumbing every post-commit CLI command uses.
- `dependency_analyzer`'s `Custom("Technology")` objects, `git_analyzer`'s `Person`/`commit_count`,
  `build.rs`'s `File`/`symbols` — real, already-compiled data this RFC reads, produces nothing new.

## Design

**`README.md`** (`render_readme(objects: &[KirObject]) -> RenderedPage`)
- Object-kind counts (reusing `render_index_page`'s grouping idiom).
- `## Contributors` — `Person` objects sorted by `commit_count` desc; honest "_No contributor data
  compiled._" placeholder when none exist.
- `## Documentation` — relative links to `Architecture.md`, `API.md`, `SequenceDiagrams.md`.

**`Architecture.md`** (`render_architecture(objects, relationships) -> RenderedPage`)
- `## Components` — grouped-by-kind counts.
- `## Technologies` — `Custom("Technology")` objects + which files `DependsOn` them; honest
  "_No technology dependencies compiled._" placeholder when none exist.
- `## Entity Relationships` — reuses `render_er_diagram` exactly when `Table` objects +
  `ForeignKey` edges exist, same condition `docs.rs`'s existing ER-diagram block uses.
- `## Dependency Graph` — one small Mermaid `graph TD` per relationship kind (`ForeignKey`,
  `DependsOn`, `CoupledWith`, `References`, `Contains`, etc.), explicitly **excluding**
  `Custom("FeedsInto")` — pipeline-internal step wiring belongs in `SequenceDiagrams.md`; a real
  Pentaho workspace has 86 `TransformNode`s, so inlining that here would be unreadable. This
  pragmatically resolves RFC 0035's still-open "diagram size" question for the curated layout:
  split by relationship *purpose*, not by trying to fit everything into one graph.

**`API.md`** (`render_api(objects: &[KirObject]) -> RenderedPage`)
- Lists `File` objects carrying a `symbols` property, grouped by file path, each symbol as a
  bullet — real data, not invented.
- A caveat line at the top: symbol names only (from a text scan for declaration-line prefixes),
  not a parsed API spec; real `Api`/`Service` objects, if ever compiled, would render here
  directly once an analyzer produces them (none does today).
- Honest "_No API surface data compiled._" placeholder when zero files carry `symbols`.

**`SequenceDiagrams.md`** (`render_sequence_diagrams(objects, relationships) -> RenderedPage`)
- Groups `Custom("TransformNode")` objects by origin (the part of `object.name` before the last
  `:index`, e.g. `fact_sales.ktr` from `fact_sales.ktr:10`).
- One Mermaid `sequenceDiagram` block per origin: a `participant` per node, one message per
  `FeedsInto` edge within that origin, labeled with the target node's `node_type`.
- A caveat line at the top of the file: this is a data-flow sequence between compiled pipeline
  steps, not a code call sequence — EKOS does not compile call-graph data today.
- Honest "_No transformation pipelines compiled._" placeholder when zero `TransformNode`s exist.

**CLI wiring** (`crates/cli/src/commands/docs.rs`, `crates/cli/src/bin/ekos.rs`)
- `pub enum Layout { Objects, Curated }` + `Layout::parse`, mirroring `Format::parse`'s shape.
- `--layout` flag on `DocsCommands::Generate`, default `"objects"`.
- `generate()` branches early: `Layout::Curated` calls a new `generate_curated(config, cwd,
  output)` that opens the store, reads `all_objects()`/`all_relationships()` once, calls the four
  new renderers, writes the four fixed-name files, prints a summary — no per-object loop, no
  `--prose`/format branching (Markdown-only this phase).

## Alternatives Considered

- **New sibling crate** — rejected; this is the same "render compiled KIR objects as text" job
  `ekos-docs-gen` already does, just a different document shape, unlike `ekos-dbt-gen` which
  renders a genuinely different output type (executable SQL with `ref()` semantics).
- **Replacing the default output shape** — rejected per the user's explicit confirmation; would
  break existing tests and the presentation deck's described behavior for no benefit.
- **Fabricating an API spec or call-graph from heuristics** — rejected; violates the project's
  evidence-first ethos harder than an honest empty-state placeholder does.

## Open Questions

- [ ] HTML output for the curated layout — deferred to a later phase; Markdown-only for now.
- [ ] Whether a future call-graph analyzer (populating real `Calls` relationships) should feed
      `SequenceDiagrams.md` instead of/alongside `FeedsInto` once one exists — not blocking this
      phase, since `FeedsInto` is real data today and a call-graph analyzer is unscoped work.
- [ ] Whether `API.md` should eventually read a real `Api`/`Service` object kind once some future
      analyzer produces one — the renderer's caveat text should update accordingly then; no
      analyzer produces one today so this doesn't block Phase 1.

## Testing

- Golden-file tests per renderer, each covering real-data rendering and the honest empty-state
  placeholder when the relevant source data doesn't exist.
- A `SequenceDiagrams.md` test covering multiple origins (two distinct pipelines must render as
  two separate `sequenceDiagram` blocks, not merged).
- An `Architecture.md` test confirming `Custom("FeedsInto")` edges are excluded from the
  `## Dependency Graph` section.
- CLI-level test: `--layout curated` writes exactly `README.md`, `Architecture.md`, `API.md`,
  `SequenceDiagrams.md` and nothing else; `--layout objects` (default) is unchanged.

## Acceptance Criteria

- [x] All Open Questions resolved or explicitly deferred with rationale.
- [x] `--layout curated` runs end-to-end against a fixture workspace, zero panics, writes exactly
      the four named files. Verified against a real compiled SQL schema (2 tables + a real
      `CREATE VIEW ... JOIN ... WHERE ... GROUP BY`, 6 real Transformation IR nodes): real
      `Architecture.md` ER diagram + dependency graph, real `SequenceDiagrams.md` Join→Join→
      Filter→Aggregate→Sink chain, and a correctly-triggered honest `API.md` empty-state
      placeholder (this fixture has no source files with harvested `symbols`).
- [x] `--layout objects` (default) behavior and all its existing tests are unaffected — all 6
      pre-existing `docs.rs` tests pass unmodified.
- [x] Golden-file tests pass for every new renderer, including empty-state placeholders (12 new
      tests in `ekos-docs-gen`, 3 new CLI-level tests in `commands/docs.rs`).
- [x] `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` clean.

## Implementation Plan

**Phase 1 — Four curated renderers + `--layout curated` (this pass).** All four renderers in
`ekos-docs-gen`, `Layout` enum + CLI wiring in `commands/docs.rs`/`bin/ekos.rs`, golden-file +
CLI-level tests.

**Phase 2 — Real-data smoke test. DONE (2026-08-07).** Ran `ekos docs generate --layout curated`
against the same real cloned Pentaho repo used for RFC 0035/0036's testing (198 compiled objects,
including 74 `Section` objects from real PDF pages/slides). Surfaced and fixed a real bug the
design's own stated goal ("never one giant unreadable diagram") didn't fully deliver on: excluding
`Custom("FeedsInto")` from `Architecture.md`'s `## Dependency Graph` wasn't enough — `Contains`
edges from PDF pages alone produced 75 edges in one relationship kind, still unreadable
(`Architecture.md` was 189 lines with a 76-edge graph). Fixed by adding a per-kind size cap (20
edges): a kind over the cap renders an honest one-line summary ("75 `Contains` relationships
compiled — diagram omitted, too large to render usefully") instead of the graph — `Architecture.md`
dropped to 34 lines. Regression test added
(`architecture_omits_a_diagram_for_a_relationship_kind_with_too_many_edges`).

Also found and fixed a wording bug in `SequenceDiagrams.md`: the no-edges placeholder said "single
step" even when a `.kjb` job (whose entries are always `Unmapped` by design, never wired together)
had 8 participants and zero edges. Fixed to report the real step count with correct
singular/plural wording; regression test added
(`sequence_diagrams_multi_step_pipeline_with_no_edges_uses_plural_wording`).

`README.md`, the real join/aggregate/filter renderers, and `API.md`'s empty-state placeholder all
rendered correctly on the first real run — no other gaps found.

**Phase 3 — HTML curated output.** Resolves the HTML Open Question, once Phase 1/2 prove the
Markdown shape is sound.

## Files Changed (Phase 1)

| File | Change |
|---|---|
| `ekos/crates/docs-gen/src/lib.rs` | `+render_readme`, `+render_architecture`, `+render_api`, `+render_sequence_diagrams` + tests |
| `ekos/crates/cli/src/commands/docs.rs` | `+Layout` enum, `+generate_curated`, `--layout` threading + tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `+--layout` arg on `DocsCommands::Generate` |

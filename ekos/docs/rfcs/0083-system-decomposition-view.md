# RFC 0083 — Real System Decomposition View (Backend/Frontend/Database)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Phase 3 of the "Deep Source Decomposition + Production-Grade Architecture Diagrams" plan — the
actual, most directly requested deliverable: "which components does it have and how do they
relate." Phases 1 (RFC 0081) and 2 (RFC 0082) gave real backend (Elixir) and real frontend (npm)
data; nothing yet tied that together with the already-real `Table` data into one top-level view a
human can read in one glance. `## System Context` (RFC 0073) exists one level up, but it's
Crate/Technology-shaped — meaningless for a project with no `Crate` at all.

## Design

**New module** `ekos/crates/docs-gen/src/layer_classification.rs`: `Layer` (`Backend`/
`Frontend`/`Database`) and `classify_path(path, overrides) -> Option<Layer>` — a small,
convention-based, path-only classifier. Backend/frontend language extensions (`.ex`/`.rs`/`.py`/…
vs `.js`/`.ts`/`.css`/…) plus `package.json` as an always-frontend signal; an unrecognized
extension (`.md`, `.toml`, `.json`, …) is honestly left unclassified rather than guessed into
either bucket. `Database` is never assigned by this function — it's real and unambiguous already
via `ObjectKind::Table` itself.

Per the plan's own explicit requirement ("never silently misclassify without an escape hatch"):
`[[architecture.system-decomposition.overrides]]` in `ekos.toml` — first-glob-match-wins, checked
before the convention, the same shape and precedence `[recover.sql.dialect-rules]`
(`resolve_dialect_name`, RFC 0031) already established for the equivalent SQL-dialect problem. New
`ArchitectureConfig`/`SystemDecompositionConfig`/`LayerOverrideConfig` in `compiler-core/src/
config.rs`, threaded through `docs.rs`'s `generate_curated` into `render_architecture`'s new third
parameter.

**New `## System Decomposition` section** in `render_architecture` (`crates/docs-gen/src/lib.rs`),
positioned directly after `## System Context` (one level more detailed, same C4-adjacent spot):
- `layer_membership` tags every compiled `File` object via `classify_path`, every `Table` object
  as `Database`.
- `system_decomposition_graph` groups into up to four real nodes — Backend, Frontend, SQL
  Database, ClickHouse Database (the two `Table` `source_system` (RFC 0056) values kept as
  distinct nodes rather than merged, since a real project can use both at once) — each labeled
  with its real compiled count, and edges wherever a real `DependsOn`/`ReadsFrom`/`WritesTo`
  relationship actually connects two different layers.
- Rendered both as Mermaid-in-Markdown and, via the existing `render_graph_svg` primitive (RFC
  0073) reused unmodified, as a standalone `system-decomposition.svg` — the same conditional-write
  pattern (`Option`, `None` when there's no real layer data at all) `render_system_context_svg`
  already established.
- No real cross-tier edge exists yet for a project without Phase 6's Ecto-repo-config parsing —
  rendered as an honest `%% No real compiled relationship yet connects these layers to each
  other.` comment rather than a guessed line, matching RFC 0068 §22's own "don't fabricate"
  principle already used throughout Data Architecture's Ownership/Lifecycle/Data Quality fields.

## Scope — what this does and doesn't cover

**Covers**: real Backend/Frontend/Database layer boxes with real per-layer counts, a real
`ekos.toml` override escape hatch, a small readable SVG (not an unreadable wide row).

**Does not cover** (explicitly deferred, not silently dropped): cross-tier edges beyond what's
already compiled — Backend→Database (Ecto repo config) and Frontend→Backend (route/fetch
matching) are both Phase 6's job, deliberately scoped there with an explicit confidence note (the
latter much lower-confidence than the former). No sub-layer detail (which files within Backend) —
that's what `## Crate & Workspace Topology`/`## Component View`/`API.md` already answer one level
down.

## Testing

- 5 new tests in `layer_classification.rs`: real backend/frontend extension recognition,
  `package.json` as an always-frontend signal, an ambiguous extension honestly left
  unclassified, an override winning over the convention, a malformed override glob skipped (not
  fatal).
- 2 new tests in `compiler-core/src/config.rs`: `[architecture.system-decomposition]` omitted
  entirely defaults to no overrides; a real `[[overrides]]` table parses correctly.
- 3 new/updated tests in `crates/cli/src/commands/docs.rs`: a real Backend `File` + `Table`
  fixture writes a real `system-decomposition.svg` linked from `Architecture.md`; a real
  `ekos.toml` override routes a `.rs` file to Frontend through the full CLI config→render
  pipeline (not just the unit-level `classify_path` test); the pre-existing "exactly four files"
  test updated to include the now-real fifth file for its one-`Table` fixture.
- 21 existing `render_architecture` call sites in `docs-gen`'s own test module updated for the new
  third parameter (`&[]` — none of those tests exercise layer overrides).
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end** against the real analytics project (no new analyzer/recover pass —
  this phase is pure `docs-gen` rendering over the already-committed real ledger from Phase 2, so
  no `recover`/`compile`/`commit` re-run was needed): `## System Decomposition` renders three real
  layers — Backend (1232 files), Frontend (324 files), SQL Database (57 tables) — as a small,
  readable 568×80px SVG, a direct contrast with System Context's unreadable 8296×190px single row
  (RFC 0073's still-open Finding, tracked as Phase 4). No cross-tier edge yet for this project,
  rendered as the honest "not yet compiled" comment rather than a guess — correct, matches Phase
  6's own deferred scope.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0083-system-decomposition-view.md` | This RFC |
| `ekos/crates/docs-gen/src/layer_classification.rs` | New: `Layer`, `LayerOverride`, `classify_path`; 5 tests |
| `ekos/crates/docs-gen/src/lib.rs` | `render_system_decomposition`/`_svg`, `render_architecture`'s new `layer_overrides` parameter and new section; 21 test call sites updated |
| `ekos/crates/docs-gen/Cargo.toml` | New `glob` dependency |
| `ekos/crates/compiler-core/src/config.rs` | `ArchitectureConfig`/`SystemDecompositionConfig`/`LayerOverrideConfig`; 2 tests |
| `ekos/crates/cli/src/commands/docs.rs` | Config→override wiring, new SVG write; 3 new/updated tests |
| `TODO.md` | Phase 3 of the decomposition plan marked done |
| `devlogs/devlog_86.md` | This increment's devlog |

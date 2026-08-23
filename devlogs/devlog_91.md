# Devlog 91 — RFC 0087: real doc-comment extraction (Rust/Python/Elixir/JS-TS)

**Date:** 2026-08-23
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Phase 1 of the "Real Descriptions, Purpose, and Links Throughout Generated Documentation" plan.
Every real code-decomposition analyzer (`rust_analyzer.rs`, `python_analyzer.rs`,
`elixir_analyzer.rs`, `javascript_analyzer.rs`) now extracts the human-written documentation that
already exists in the real source — `///` doc comments, Python docstrings, `@moduledoc`/`@doc`,
JSDoc — as a real `"description"` property on the `Module`/`Symbol` object, never fabricated.
`docs-gen`'s entity pages (Phase 2, same session) promote that property into the page's Definition
section, falling back to an honest "Not documented in source" when the property is absent. Found
this work already in progress at session start (all four analyzers and `docs-gen` modified,
RFC 0087 filed, no devlog yet) — this entry closes it out: full workspace gate re-verified, then
live-verified end-to-end against the real analytics project.

## PR #N — RFC 0087 implementation

### Problem / motivation

`docs-gen`'s entity pages render structural properties (`kind`, `visibility`, `arity`) but never
any human-written documentation, even when the real source has it — every page's "Definition"
section was structural-only. `architecture_reasoning.rs`'s `read_crate_doc_comment` was the one
precedent (reads a crate's `//!` preamble) but its output only ever fed an LLM prompt for `ekos
architecture investigate`, never persisted as a KIR property or read by `docs-gen`.

### What was built

| Analyzer | Extraction | Notes |
|---|---|---|
| `rust_analyzer.rs` | `extract_doc_comment` | Reads `syn`'s `#[doc = "..."]` attributes (what `///` desugars to) directly off the already-parsed AST — no new parsing. Consecutive doc lines join with a space. Wired into `fn`/`struct`/`enum`/`trait` items and `impl` methods (5 call sites); `add_symbol` gained a new parameter to carry it through. |
| `python_analyzer.rs` | `python_docstring` | Real PEP 257 convention — a function/class body's own first statement, a bare string-literal expression statement — via `string_constant`, a helper already in this file for PySpark chain-argument recognition. |
| `elixir_analyzer.rs` | `extract_doc_comments` | The one real design complication: `@moduledoc` sits *inside* the module (documenting the already-open enclosing module) while `@doc` *precedes* the function it documents — genuinely different structural positions. Returns two separate maps (`moduledoc` keyed by its own line, `doc` keyed by the line right after). Handles single-line and `"""`-heredoc forms via a real pre-scan pass. `@moduledoc false`/`@doc false` recognized and intentionally produce no entry. |
| `javascript_analyzer.rs` | `extract_jsdoc_by_offset` | Uses `oxc_parser`'s own `program.comments`, already classified by `CommentContent::Jsdoc`, with a real `attached_to` offset. A `doc_anchor: u32` (the *outermost* wrapping statement's span start — `export`/`export default`'s own span when present) is threaded from `extract_javascript_file`'s match arms through every handler, so a JSDoc comment preceding `export function Foo()` or `export const Foo = () => {}` attaches correctly rather than being lost to the wrapper. |

`docs-gen/src/lib.rs`: entity-page rendering promotes a real `"description"` property into the
page's Definition section (both Markdown and HTML renderers), with an honest "Not documented in
source" fallback when the property is absent — never fabricated, matching the property-absence
convention `crate_topology_analyzer.rs` already established for Cargo.toml descriptions.

### Implementation details worth remembering

- No new object kinds and no new parsing: every extraction reuses structure the analyzer's AST/
  line-scanner already builds (`syn`'s parsed attributes, the PySpark-chain string helper, the
  Elixir line-oriented scanner's existing block-depth tracking, `oxc_parser`'s own comment
  classification).
- The JS/TS `doc_anchor` design point is the one genuinely subtle piece: JSDoc precedes the
  *statement*, not the inner function/class node, so a comment on `export function Foo()` has to
  resolve against the `export` keyword's span start, not `Foo`'s own span, or it's silently
  dropped for every exported (i.e. most) real symbol.
- Elixir's two-map return (`moduledoc` vs `doc`) is required by position alone — collapsing them
  into one map keyed purely by line number would conflate "documents the enclosing module" with
  "documents the next function," which sit on genuinely different sides of their own attribute
  line.

### Decisions (alternatives considered, why this choice)

- **Property absence over an empty-string sentinel**: no `"description"` property at all when the
  source has none, rather than writing `""` or a placeholder — keeps "undocumented" indistinguishable
  from "not yet extracted" at the storage layer false, and keeps the honesty judgment (what string
  to show) entirely in the render layer, not duplicated into every analyzer.
- **File/module-level Rust (`//!`) and Python (top-of-file docstring) deliberately out of scope**:
  neither language has a real KIR object representing "this file as a module" the way Elixir's
  `defmodule` or Rust's `use`-target `RustModule` objects do — no real target exists to attach the
  text to. A real gap, left open rather than inventing a new object kind just to hold it.

## Testing

18 new tests across the four analyzers (all passing): 4 in `rust_analyzer.rs` (single-line `///`,
multi-line join, a method inside `impl`, no-doc-comment), 4 in `python_analyzer.rs` (function
docstring, class docstring, no docstring, a real non-string first statement not mistaken for one),
5 in `elixir_analyzer.rs` (single-line `@moduledoc`, heredoc `@moduledoc`, single-line `@doc`
attaching only to its own function, `@moduledoc false`/`@doc false` producing no description, no
description property at all when absent), 5 in `javascript_analyzer.rs` (a real JSDoc block, JSDoc
on an exported function, JSDoc on an arrow-function `const` — both confirming `doc_anchor` uses the
outer `export` span — a plain `//` line comment not mistaken for JSDoc, no-comment case). Plus
`docs-gen` gained its own render-layer test confirming the honest "not documented in source"
placeholder when the property is absent, and that the HTML page promotes description into its own
definition section.

Full workspace gate re-verified this session: `cargo build/test/clippy -D warnings/fmt --check`
from `ekos/`, all clean (232+ tests across the workspace, 0 failures).

## Live verification

Ran the full pipeline (`init` → `build` → `recover` → `compile` → `commit` → `docs generate`) from
a clean `.ekos/` against the real analytics project to verify end-to-end, not just unit-level:

- **Negative case**: `Plausible.Auth.Password` (`lib/plausible/auth/password.ex`) — genuinely no
  `@moduledoc` in source — renders "_Not documented in source._" Also reconfirms devlog_90's two
  bug fixes still hold on a fresh rebuild: exactly 3 real `Contains` relationships, no `SameAs`
  contamination.
- **Positive case**: `Plausible.SentryFilter` (`lib/sentry_filter.ex`) has a real
  `@moduledoc """Sentry callbacks for filtering and grouping events"""` — the entity page's
  Definition section renders that exact real text, extracted correctly from real source.
- Pipeline numbers matched devlog_90's clean-rebuild baseline: 2,471 files observed, 139 real local
  documents, 1,231 Elixir files / 1,260 modules / 4,812 symbols, no re-growth from the
  `docs-generated/` contamination loop.

**Process note — a live-verification side effect caught and fixed before it became a new bug**:
`docs generate --output doc` (the exact form in this project's own `CLAUDE.md` command list) wrote
a new `doc/` folder inside the analytics checkout. That project's `ekos.toml` `ignore-patterns`
excludes `docs-generated` (per devlog_90's fix) but not `doc` — leaving that folder in place would
have reintroduced the identical self-ingestion contamination bug under a different directory name
on the next real `ekos build`. Caught before any real build re-ran; the folder was moved out of the
analytics tree (not left in place) and `.ekos/` was restored to its pre-verification state, since
this session's actual scope is EKOS itself, not maintaining that project's workspace.

## Knowledge Captured

- **A generated-docs output directory name is not fixed across sessions** — `--output doc` and
  `--output docs-generated` are both real, both used in this project's own history, and only one
  of them was ever added to a downstream project's `ignore-patterns`. Any live-verification run
  against a real external project should either reuse that project's already-ignored output
  directory name, or add the new one to `ignore-patterns` before running `build` again — otherwise
  the exact contamination class devlog_90 fixed can recur silently under a new name.
- **JSDoc/doc-comment attachment must resolve against the outer statement span, not the inner
  declaration node**, whenever a language allows a modifier keyword (`export`, `pub`, decorators)
  to wrap the documented declaration — the comment precedes the statement as written in source, not
  the AST's inner node.
- Confirms an existing lesson rather than a new one: live verification against the real analytics
  project (not just clean unit fixtures) is still this project's highest-value practice — this
  session's process note above is a small-scale repeat of exactly what devlog_90 found at bug scale.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0087-real-doc-comment-extraction.md` | RFC (found already filed at session start) |
| `ekos/crates/recovery/src/rust_analyzer.rs` | `extract_doc_comment`; `add_symbol` new param; 4 tests |
| `ekos/crates/recovery/src/python_analyzer.rs` | `python_docstring`; `add_symbol` new param; 4 tests |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `extract_doc_comments`, `extract_quoted`; 5 tests |
| `ekos/crates/recovery/src/javascript_analyzer.rs` | `extract_jsdoc_by_offset`; `doc_anchor` threaded through every handler; 5 tests |
| `ekos/crates/docs-gen/src/lib.rs` | Definition-section promotion of `"description"`; honest fallback; new render tests |
| `devlogs/devlog_91.md` | This file |

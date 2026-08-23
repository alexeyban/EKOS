# RFC 0087 — Real Doc-Comment Extraction (Rust/Python/Elixir/JS-TS)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-23

---

## Motivation

Phase 1 of the "Real Descriptions, Purpose, and Links Throughout Generated Documentation" plan.
Every real code-decomposition analyzer this project has (`rust_analyzer.rs`, `python_analyzer.rs`,
`elixir_analyzer.rs`, `javascript_analyzer.rs`) extracts structural properties (`kind`,
`visibility`, `arity`) but never the human-written documentation that already exists in the real
source — `///` doc comments, Python docstrings, `@moduledoc`/`@doc`, JSDoc. Confirmed by direct
research before designing anything: no analyzer captures any of this today;
`architecture_reasoning.rs`'s `read_crate_doc_comment` is the one real precedent (reads a crate's
`//!` preamble) but its output only ever feeds an LLM prompt for `ekos architecture investigate` —
never persisted as a KIR property, never read by `docs-gen`.

## Design

Each analyzer writes a real `"description"` property (reusing the one established convention for
human-readable text on a `KirObject` — `crate_topology_analyzer.rs`'s Cargo.toml descriptions) onto
the real `Module`/`Symbol` object, only when the source actually has one. No property at all when
absent — never fabricated; the entity-page rendering layer (Phase 2) is responsible for the honest
"not documented in source" fallback, not this extraction step.

- **Rust** (`rust_analyzer.rs`): `extract_doc_comment` reads `syn`'s own `#[doc = "..."]`
  attributes (what `///` desugars to) directly off the already-parsed AST — no new parsing, a real
  read of structure `syn::parse_file` already builds. Consecutive doc lines join with a space.
  Wired into `fn`/`struct`/`enum`/`trait` items and `impl` methods (5 call sites).
- **Python** (`python_analyzer.rs`): `python_docstring` reads the real PEP 257 convention — a
  function/class body's own first statement, a bare string-literal expression statement — via
  `string_constant`, a helper already in this file for PySpark chain-argument recognition.
- **Elixir** (`elixir_analyzer.rs`): `extract_doc_comments` — the one real design complication.
  `@moduledoc` and `@doc` sit in genuinely different structural positions: `@moduledoc` is the
  first real statement *inside* the module (after `defmodule X do`, documenting the already-open
  enclosing module), while `@doc` *precedes* the specific `def`/`defp` it documents. Returns two
  separate maps (`moduledoc` keyed by the attribute's own line, `doc` keyed by the line right
  after it) rather than one. Handles both single-line `"..."` and `"""`-heredoc forms (a real
  pre-scan pass, since the main line-oriented loop has no lookahead) — `@moduledoc`/`@doc false`
  recognized and intentionally produces no entry. `@moduledoc` mutates the module object in place
  once encountered a few lines after its own creation (`module_obj_index`) since real Elixir
  syntax puts it after, not on, the `defmodule` line.
- **JS/TS** (`javascript_analyzer.rs`): `extract_jsdoc_by_offset` uses `oxc_parser`'s own
  `program.comments`, already classified by `CommentContent::Jsdoc` (a real `/** ... */` block,
  not any comment) with a real `attached_to` offset — the exact token position the comment
  precedes. A `doc_anchor: u32` (the *outermost* wrapping statement's span start — `export`/
  `export default`'s own span when present, not the inner `Function`/`Class` node's) is threaded
  from `extract_javascript_file`'s match arms down through every handler, so a JSDoc comment
  preceding `export function Foo()` or `export const Foo = () => {}` attaches correctly rather
  than being lost to the wrapper.

## Scope — what this does and doesn't cover

**Covers**: real, already-written documentation surfaced as a real `"description"` property,
across all four real decomposition analyzers.

**Does not cover** (explicitly deferred, not silently dropped): file/module-level Rust (`//!`) and
Python (top-of-file docstring) capture — neither language has a real KIR object representing "this
file as a module" the way Elixir's `defmodule` or Rust's `use`-target `RustModule` objects do, so
there's no real target to attach the text to; a real gap, not attempted here rather than invented a
new object kind just to hold it. Sigil-prefixed Elixir docs (`~S"""`) — the same "not sigil/
heredoc-aware" limitation this file's own top-level doc comment already states for block-depth
tracking. A module referenced via `alias`/`import` before its own real `defmodule`/`@moduledoc` is
processed (cross-file iteration order) can miss its real description — the same accepted risk RFC
0081 already documents for cross-file dedup ordering, not a new one.

## Testing

- 5 new tests in `elixir_analyzer.rs`: single-line `@moduledoc`, heredoc `@moduledoc` (joins real
  lines), single-line `@doc` attaching only to its own function, `@moduledoc false`/`@doc false`
  producing no description (not the literal word), and no-doc-comment producing no property at
  all.
- 4 new tests in `rust_analyzer.rs`: single-line `///`, multi-line `///` (joins), a method inside
  `impl`, and no-doc-comment.
- 4 new tests in `python_analyzer.rs`: function docstring, class docstring, no docstring, and a
  real non-string first statement not mistaken for one.
- 5 new tests in `javascript_analyzer.rs`: a real JSDoc block, a JSDoc block on an exported
  function (confirms `doc_anchor` correctly uses the outer `export` span, not the inner function's
  own), a JSDoc block on an arrow-function `const` (same outer-span correctness), a plain `//`
  line comment *not* mistaken for JSDoc, and no-comment producing no property.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0087-real-doc-comment-extraction.md` | This RFC |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `extract_doc_comments`, `extract_quoted`; 5 tests |
| `ekos/crates/recovery/src/rust_analyzer.rs` | `extract_doc_comment`; `add_symbol` new param; 4 tests |
| `ekos/crates/recovery/src/python_analyzer.rs` | `python_docstring`; `add_symbol` new param; 4 tests |
| `ekos/crates/recovery/src/javascript_analyzer.rs` | `extract_jsdoc_by_offset`; `doc_anchor` threaded through every handler; 5 tests |

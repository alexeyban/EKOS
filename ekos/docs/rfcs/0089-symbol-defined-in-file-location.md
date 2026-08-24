# RFC 0089 — Real "Defined in" File Location on Symbol/Module Entity Pages

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-24

---

## Motivation

Found live: a user reviewing a real generated entity page (`Plausible.IP.Tools.Registry`'s
`combine_guards` symbol, `analytics/lib/ip`) asked why the page didn't say what file the symbol was
defined in. `ObjectPageModel`'s `"Based on"` relationship group already renders the object's
immediate real `Contains` parent — for a `RustSymbol`/`PythonSymbol`/`ElixirSymbol`/`JsSymbol` that
parent is its owning module, not the file two hops up (`File --Contains--> Module --Contains-->
Symbol`). The file is real, already-compiled data (every module has exactly one `Contains` edge
from the file that declared it), just never surfaced past one hop.

## Design

Two additions, both pure/deterministic, zero LLM, matching RFC 0035's "no new extraction, only
rendering already-compiled data" scope:

1. **`ekos_docs_gen::build_contains_parent_map(&[KirRelationship]) -> HashMap<KirId, KirId>`** —
   real `to -> from` for every real `Contains` edge, built once per `docs generate` run.
2. **`ekos_docs_gen::resolve_defining_file(object_id, &parent_of, &objects_by_id) -> Option<KirId>`**
   — walks the parent chain from `object_id` looking for a real `ObjectKind::File`. Returns `None`
   when the *immediate* parent already is the file (a module's own case — already shown by its own
   `"Based on"` row, so this would just repeat it) or when the chain never reaches one at all
   (bounded by the map's own size against a malformed cycle). Only a real multi-hop resolution
   (symbol → module → file) produces `Some`.

`ObjectPageModel` gains two fields: `source_span: Option<(u32, u32)>` (promoted out of the generic
`properties` table exactly the way `description` already is — real `{start_line, end_line}` RFC
0088 compiles for Rust/Elixir symbols today) and `defined_in_file: Option<String>` — not produced by
`build_object_page_model` itself (that function only sees the one object's own touching
relationships, never the whole graph) but set by the caller afterward, the same "layered on top"
pattern `prose` already uses. Both `crates/cli/src/commands/docs.rs` call sites (`--layout objects`
and `--layout curated`'s entity-page loop) build the parent map once over the full ledger and set
`model.defined_in_file` per object.

Rendered as one line right under `## Definition`:

- Both known: `**Defined in:** \`tools.ex\` (lines 47–52)`
- File only (no `source_span` — e.g. `combine_guards`, whose real span-capture rule per RFC 0088
  leaves multi-clause functions whose first clauses are one-line `, do:` forms unspanned):
  `**Defined in:** \`tools.ex\``
- Neither: nothing rendered — never a fabricated placeholder.

## Scope — what this does and doesn't cover

**Covers**: a real one-line "Defined in" surfaced for any object whose real `Contains` chain
resolves a file more than one hop up — today that's every Rust/Python/Elixir/JS symbol.

**Does not cover**: a clickable link to the file's own page — `File` isn't an `is_entity_page_kind`
(no per-file page exists at all, consistent with every other relationship row that names a `File`
by plain text + id, never a link). Adding a `File` entity page is a larger, separate scope decision
not made here.

## Verification

5 new `ekos-docs-gen` unit tests (`resolve_defining_file` on a real 2-hop chain, a 1-hop
already-shown case, and a chain that never reaches a file; render tests for both
Markdown/HTML with and without `source_span`/`defined_in_file`). Live-verified against the real
`analytics/lib/ip` scope: `allowed?` now reads `**Defined in:** \`tools.ex\` (lines 47–52)`,
`combine_guards` reads `**Defined in:** \`tools/registry.ex\`` (no lines — correctly, it has no real
compiled `source_span`), and `Plausible.IP.Tools`'s own module page shows no redundant "Defined in"
line (its `"Based on"` row already names `tools.ex`). Full workspace gate
(`fmt`/`build`/`clippy -D warnings`/`test --workspace`) and `tests/integration` clean.

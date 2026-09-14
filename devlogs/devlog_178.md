# Devlog 178 — RFC 0141: what a symbol and a `Calls` edge actually carry

**Date:** 2026-09-14
**PRs:** (local, not yet pushed) `feat(recovery): RFC 0141 §1/§2/§3 — symbol signatures, Calls edge attributes, embedding basis`
**Branch:** main (local)

---

## Summary

RFC 0141 (§4 already shipped devlog_174) proposed three more entity/edge attributes, each tied to
a specific measured RFC 0138 failure: a `signature` property on function/method symbols, three new
properties on `Calls` edges (`call_count`/`call_site_line`/`caller_is_test`), and a symbol-specific
embedding basis text. All three are now implemented, unit-tested, and clean under
`cargo test --workspace` / `clippy -D warnings` / `fmt --check`. The RFC's own status moved from
Proposed to Accepted as part of this session, following the mandatory Design → Architecture Review
→ Interfaces → Tests → Implementation sequence — the design was additive (existing
`properties: HashMap` fields, no schema change) and already anticipated the one real determinism
hazard (§2's edge-id churn), so the review confirmed rather than reshaped it.

Two things turned out narrower than the RFC's own prose implied, both recorded honestly in the RFC
and TODO.md rather than smoothed over: `Calls` edges exist in exactly one analyzer (`rust_analyzer`
— Elixir/JS never build a call graph, contra the crate map's older phrasing), and `caller_is_test`
only fires for a bare top-level `#[test] fn` or a `tests/`-directory file, not the idiomatic inline
`#[cfg(test)] mod tests { ... }` pattern, because `rust_analyzer` has never walked into nested
`mod` bodies as a symbol source at all — a pre-existing limitation this RFC does not fix.

---

## PR — RFC 0141 §1/§2/§3

### Problem / motivation

The RFC ties each item to a specific eval-suite failure: `code-002` ranks the type `LlmProvider`
above the function `build_llm_provider` that returns one, because the discriminating term lives in
the return type and nothing recorded it; `ekos impact`'s `Calls` edges are a bare id pair, so 40
dependents of which 35 are tests reads identically to 40 production call sites; and three semantic
scenarios fail because the question never names the object it wants (`ObservationArtifact`), which
only a doc-comment-shaped basis text can bridge.

### What was built

| Item | Where | What |
|---|---|---|
| §1 signature | `rust_analyzer.rs`, `python_analyzer.rs`, `elixir_analyzer.rs` | Real declaration text on `function`/`method` symbols |
| §2 Calls attributes | `rust_analyzer.rs` | `call_count`, `call_site_line` (min, deterministic), `caller_is_test` on every `Calls` edge |
| §3 embedding basis | `embed.rs::embedding_text` | Symbol objects embed from `name + signature + description`, not `kind`/`ai_overview`/`excerpt` |

### Implementation details worth remembering

**§1 is real source-text slicing, not re-synthesis, in all three languages** — matching this
codebase's existing discipline (`source_evidence.rs`'s fragments) rather than a `quote!`-rendered
reprint:

- **Rust**: `syn::Signature` already implements `ToTokens`, so `.span()` (via the blanket
  `Spanned` impl) gives the exact joined span from `fn` through the return type/where-clause —
  excluding attributes and the doc comment, which live on the outer `ItemFn`/`ImplItem::Fn`, not
  on `Signature` itself. The end boundary is `block.brace_token.span.open()` — `DelimSpan::open()`
  gives the `{`'s own span. Slicing `source` between those two points (via the existing
  `source_evidence::slice_lines`, line-granularity, then trimming at the last `{`) gives the real
  text, indentation included. `quote` was never added as a dependency — confirmed unnecessary
  once the span-slicing approach worked.
- **Python**: `rustpython_parser`'s `Identifier` (the AST's `name` field) carries no span of its
  own, unlike `syn`'s idents. Anchored instead on a forward text search for the literal `def`
  keyword, starting just past the last decorator's own span (`StmtFunctionDef::range()` includes
  decorators, which are not part of a signature) — a false match is not realistically reachable
  since only whitespace/newlines can appear between a decorator and the keyword in valid Python.
  Slices to the first body statement's start, trimmed at the last `:` before it (handles a lambda
  default argument's own `:` correctly, since the *last* colon before the body is always the real
  one). **Found along the way**: `async def` is a wholly separate AST variant
  (`Stmt::AsyncFunctionDef`), which `walk_top_level_statement` has never matched — an async
  top-level function gets no `PythonSymbol` at all, today or before this change. Not fixed here
  (would be new analysis, out of this RFC's scope) — a test asserting the opposite was written,
  failed for exactly this reason, and was deleted rather than the analyzer being extended to paper
  over it.
- **Elixir**: no real AST exists for this hand-written line scanner (documented as such in the
  file's own header). The `def`/`defp` line it already reads for `def_kind`/`parse_def_line` is
  reused directly, with the block-opening `do` (or the compact one-line `, do:` form) stripped
  from the end. A guard clause spanning to a `do` on a later line is kept as-is, real but
  incomplete text — the same tradeoff `source_span`'s own multi-line guard handling already
  accepts for this analyzer.
- **`javascript_analyzer` was deliberately not touched.** It has a real AST (`oxc_parser`) and
  could have gotten the same treatment, but the RFC's own §1 prose names only Rust/Python/Elixir —
  extending further would have been scope this task never asked for.

**§2's real surprise was that only one analyzer needed it.** The crate map in `CLAUDE.md`
describes `elixir_analyzer`/`javascript_analyzer` as having "real AST + `Calls` recovery" — a
`grep -rln "RelationshipKind::Calls" crates/recovery/src/` before writing any code turned up
`rust_analyzer.rs` alone. Both other files' own module-doc comments already say so explicitly:
Elixir's states "not interprocedural call tracing" as a deliberate scope decision, and
JavaScript's states "Not a call graph, not a JSX component-tree walk." Confirming this by grep
first (rather than assuming the crate map was current) avoided writing dead aggregation code for
two analyzers that never produce the relationship kind it would apply to.

`CallVisitor`'s edge collection moved from `HashSet<(KirId, KirId)>` to
`HashMap<(KirId, KirId), CallSiteAgg { count: u32, first_line: Option<u32> }>`. The RFC's own text
is explicit that `call_site_line` must be the minimum line, computed rather than "whichever call
the visitor reached first" — `syn::visit::Visit`'s traversal order is real but incidental, and
`HashMap`/`HashSet` iteration afterwards is not order-preserving regardless. `record()` takes an
explicit `.min()` on every call, so the final value is order-independent by construction, matching
the RFC 0135 Part C determinism discipline this same file already applies to relationship ids
(`KirRelationship::deterministic`'s `discriminator` never includes anything that shifts when
unrelated code moves).

`caller_is_test` combines a path check (`path.contains("tests/")`) with an attribute check
(`#[test]` or `#[cfg(test)]`, via `Attribute::parse_nested_meta` — ignores `cfg(not(test))`
correctly, since that predicate's own path is `not`, not `test`). Both signals are exactly what
the RFC named ("derivable from the caller's own path/attributes... no new analysis"). What the RFC
didn't call out, found while implementing: `rust_analyzer` has never walked into `Item::Mod`
bodies at all (top-level `for item in &file.items` only) — meaning the idiomatic
`#[cfg(test)] mod tests { #[test] fn ... }` pattern (used throughout this very codebase) produces
*no* `RustSymbol` for its test functions, today or before this change. `caller_is_test` is
therefore real wherever it's set, but fires far less often than the RFC's "40 dependents of which
35 are tests" framing suggested. Recursing into nested modules would be new analysis (a real,
separate, larger change) and was left alone rather than folded into this RFC's scope.

**§3 targeted the wrong-named-right-shaped function.** The RFC's own text says "not the current
`indexed_content` (`excerpt + symbols + ocr_text + ai_overview + ai_usage`)" — that exact shape is
`KirObject::indexed_content()` in `kir/src/lib.rs`, which serves the BM25 lexical index and was
left untouched (correct: a symbol never carries `excerpt`/`symbols`/`ocr_text` regardless of this
change). The actual embedding input is `embed.rs`'s private `embedding_text()`, a structurally
similar but separately-implemented function (`name + kind` then `ai_overview` *or* `excerpt`) that
feeds `EmbeddingProvider::embed` directly. Any object carrying a `symbol_kind` property now takes
a different, earlier branch: `name` + `signature` (if present) + `description` (if present),
joined by newlines, with no `ai_overview`/`excerpt` fallback — those either don't exist on a
symbol object or are a separate opt-in enrichment this basis doesn't need to wait on.

### Decisions (alternatives considered, why this choice)

- **Rust signature: span-slicing vs. `quote!`-rendering.** `quote!(#sig).to_string()` would have
  worked without any new byte-offset arithmetic, but proc-macro2's `Display`/`ToTokens` output
  inserts spacing inconsistent with real Rust formatting (`Arc < dyn LlmProvider >`), which is
  fine for BM25 tokenization but reads badly wherever a signature is rendered to a human (docs-gen,
  MCP evidence citations). Span-slicing costs a little more arithmetic (line/column → byte offset,
  reusing `slice_lines`) but produces the literal source text, matching this codebase's existing
  "real text, never re-synthesized" discipline for `source_evidence`'s own fragments. No new
  dependency (`quote` was never added).
- **Python signature anchor: forward search vs. backward search from the name.** `Identifier` has
  no span, so the alternative was searching *backward* from wherever the name's substring first
  matches after the AST-given range start — fragile (a decorator or a string literal earlier in the
  file could contain the same identifier text) and more code. Searching *forward* from just past
  the last decorator for the literal `def` keyword is simpler and has no realistic false-match path
  in valid Python syntax.
- **`caller_is_test` scope: fix the nested-`mod` gap now, or record it as a known limit?** Fixing
  it (recursing into `Item::Mod` bodies to recover inline `#[cfg(test)] mod tests` functions) would
  meaningfully change the analyzer's own scope statement in its module doc comment and is a real,
  separately-motivated piece of work — likely worth its own RFC given `rust_analyzer`'s doc
  comment currently describes its own scope deliberately, not by omission. Recorded as an honest
  gap in both the RFC and TODO.md instead of silently expanding this task.

---

## Knowledge Captured

- **`syn::Signature` and `syn::token::Brace`'s `DelimSpan` are enough to slice a real function
  signature out of source text with no new dependency.** `Signature: ToTokens` gives a real joined
  span via the blanket `Spanned` impl; `Brace::span: DelimSpan` exposes `.open()`/`.close()` for
  the delimiter's own position. Together they bound "declaration start" to "body start" precisely,
  without `quote`.
- **`rustpython_parser`'s `Identifier` (the `name` field on `StmtFunctionDef`) carries no span.**
  Unlike `syn::Ident`, if you need the byte position of a Python symbol's own name, you cannot get
  it from the AST directly — anchor on a neighboring node's span instead (here, a forward search
  from the end of the decorator list).
- **`async def` is a distinct top-level AST variant (`Stmt::AsyncFunctionDef`) in
  `rustpython_parser`, not a flag on `Stmt::FunctionDef`.** `python_analyzer.rs`'s
  `walk_top_level_statement` has never matched it — every async top-level function in an observed
  Python codebase produces no `PythonSymbol` at all, a pre-existing gap this session found but did
  not fix.
- **Before extending a per-language analyzer, grep for the actual relationship/property it's
  supposed to already produce — don't trust the crate map's prose.** `CLAUDE.md`'s own crate table
  describes Elixir/JS as having "Calls recovery"; `grep -rln "RelationshipKind::Calls"` shows only
  `rust_analyzer.rs` ever constructs one, and both other files' own doc comments already say so.
  Would have been easy to write dead code for two analyzers whose relationship kind never fires.
- **A `HashMap`/`HashSet`'s iteration order must never leak into a persisted value.** `call_count`
  is a straightforward aggregate, but `call_site_line` (the *first* call site) has to be computed
  as an explicit running `.min()`, never "whichever call `Visit` reached first" — the same
  determinism discipline RFC 0135 Part C already applies to relationship ids, now applied to an
  edge property too.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/rust_analyzer.rs` | `signature` on function/method symbols; `call_count`/`call_site_line`/`caller_is_test` on `Calls` edges; `CallVisitor` edge set moved `HashSet` → `HashMap`; pass version `v3` → `v4`; 10 new tests |
| `ekos/crates/recovery/src/python_analyzer.rs` | `signature` on function symbols (`python_signature` helper); pass version `v3` → `v4`; 3 new tests |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `signature` on function symbols (`elixir_signature` helper); pass version `v3` → `v4`; 3 new tests |
| `ekos/crates/recovery/src/embed.rs` | `embedding_text` gains a symbol-specific basis (`name`+`signature`+`description`); 3 new tests |
| `ekos/docs/rfcs/0141-entity-and-edge-attributes.md` | Status Proposed → Accepted; §1/§2/§3 each gain a "Shipped" note describing what was actually built and where scope narrowed from the original proposal |
| `TODO.md` | RFC 0141 entry marked done with the same detail, including the still-open real-workspace rebuild + eval re-run |

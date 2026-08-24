# Devlog 98 — real `source_span` capture for Python symbols (RFC 0088 fast-follow)

**Date:** 2026-08-24
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Analyzed a new real project this session for the first time — `pdf-reader/backend/app` (a real
FastAPI/PyMuPDF/Tesseract app, 15 files) — and generated curated docs. The user then asked why every
`PythonSymbol` page's `## Definition` was empty and asked for an AI-generated definition there too.
Two real, separate causes: most of these symbols (FastAPI route handlers, service functions) simply
have no docstring in the real source — honestly correct, not a bug. But asking for the LLM-generated
overview surfaced a real gap RFC 0088 always had for Python specifically: `llm_description.rs` only
describes a `Symbol` object when it has a real compiled `source_span` (needed to slice the real
source text sent to the LLM), and `python_analyzer.rs` — unlike `rust_analyzer.rs`/
`elixir_analyzer.rs` — never captured one. Every `PythonSymbol` in any workspace would be honestly
skipped by `scope = "symbols"`/`"all"`, silently, regardless of the config.

## The fix

`python_analyzer.rs` gains `line_number(source, offset) -> u32` (1-indexed, counts `\n` bytes before
the byte offset — `rustpython_parser`'s `Ranged::range()` gives byte offsets, not line/column, so
this is the real conversion Rust's `syn::LineColumn`-based `item_span` didn't need) and
`item_span<T: Ranged>(item, source) -> (u32, u32)`. Wired into both `add_symbol` call sites
(`FunctionDef`/`ClassDef`) — `add_symbol` gained a `span: Option<(u32, u32)>` parameter, writing the
same `source_span` property shape (`{"start_line", "end_line"}`) Rust/Elixir already write.

4 new tests: single-line function, multi-line body, a class (methods aren't individually walked —
see `python_analyzer.rs`'s own documented scope — but the class itself still gets a real span), and
a symbol defined after other real top-level code (confirms line counting isn't just "starts at 1").
All 21 `python_analyzer` tests pass. Full workspace gate (`fmt`/`build`/`clippy -D
warnings`/`test --workspace`) and `tests/integration` clean.

## What was correctly *not* a bug

Most of `pdf-reader`'s route-handler/service functions (`delete_document`, `list_documents`,
`compute_file_hash`, ...) genuinely have no docstring — RFC 0087's real-doc-comment-only rule means
`## Definition` correctly rendered "Not documented in source" for them, same as it would for Rust or
Elixir. This fix doesn't change that — a docstring is still never fabricated. What it changes is
that `## AI-Assisted Overview` (RFC 0088, opt-in, LLM-grounded — a real reading of the function's
own source, not a claim about human-written documentation) can now actually run for these symbols
once `[llm-description] scope` includes `"symbols"`/`"all"`, where before every one was silently,
honestly skipped with no way to know why short of reading `llm_description.rs` itself.

## Knowledge Captured

- **RFC 0088's `source_span` requirement was launched for Rust/Elixir only, and nothing surfaced
  the gap for Python until a real user pointed at a real empty page and asked for the AI overview
  specifically** — same shape as `devlog_87`'s own real-doc-comment-extraction rollout (Rust/
  Python/Elixir/JS all in one RFC) vs. RFC 0088's `source_span` (Rust/Elixir only, Python/JS
  deferred without it being flagged prominently as a gap in the RFC's own scope section). Worth
  checking, the next time a "symbol-level X" feature ships for only some of the four structural
  analyzers, whether that's a deliberate scope cut or a silent gap — the cost of missing it is a
  real language's real users getting silently worse output with no diagnostic telling them why.
- Every structural analyzer (`rust_analyzer.rs`, `python_analyzer.rs` now, `elixir_analyzer.rs`)
  independently reimplements its own `item_span`/span-capture helper, in each language's own real
  span representation (`syn::LineColumn` for Rust, byte offset via `rustpython_parser::Ranged` for
  Python, a hand-tracked block-depth stack for Elixir) — there's no shared abstraction across them,
  and there doesn't need to be one: each language's real AST/parser gives spans in a genuinely
  different native shape, and forcing a shared interface would add indirection without removing any
  real per-language logic.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/python_analyzer.rs` | `line_number`/`item_span`; `add_symbol` writes a real `source_span`; 4 new tests |
| `ekos/crates/recovery/src/llm_description.rs` | Module doc comment updated to reflect Python's real coverage |
| `devlogs/devlog_98.md` | This file |

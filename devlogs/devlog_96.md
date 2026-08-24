# Devlog 96 — real `@doc`/`def` doc-comment extraction bug found via a scoped `lib/ip` re-verification

**Date:** 2026-08-24
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

After committing and pushing the 7-plugin bare-file `[observe] paths` fix (`devlog_95`/`fed1e0b`),
verified it end-to-end with a small, fast, real scope instead of the full ~2-3hr backend run: the
user asked for documentation generated only for `analytics/lib/ip` (2 real Elixir modules,
`Plausible.IP.Tools` + `Plausible.IP.Tools.Registry`, 18 symbols, plus the real top-level
`README.md` as a bare-file `[observe] paths` entry — exactly the bug class just fixed). The bare-file
fix verified clean: `README.md` resolved correctly for both `File` and `Document` kinds, and
`Architecture.md`'s `Purpose` field read the real, correct project purpose.

But the user then asked why almost every `ElixirSymbol` page in that output had no `## Definition`
even for public functions with real, clearly-written `@doc` comments. Chased to a real,
previously-undiscovered bug in `elixir_analyzer.rs`'s doc-comment extraction, unrelated to the
bare-file bug: `extract_doc_comments` only attaches a `@doc` to the *very next source line* — any
`@spec` (or blank line) between `@doc` and the `def`/`defp` it documents breaks the match entirely.
This is the standard, near-universal real Elixir convention (`@doc` above `@spec` above `def`) —
every real public function in `lib/ip/tools.ex` used this exact shape, and every one of them
silently lost its doc comment before this fix. 17 tests for this doc-comment logic existed before
this session; none used this shape.

## The bug

`extract_doc_comments` (elixir_analyzer.rs) keys its `doc: HashMap<usize, String>` map by the line
index immediately following where the `@doc` text closes (`result.doc.insert(i, text)`), then the
main parse loop looks up `doc_comments.doc.get(&line_idx)` at the exact line a `def`/`defp` is
found. This only matches when `def` is literally the next real line after `@doc`. Real code from
`analytics/lib/ip/tools.ex`:

```elixir
@doc """
Returns the ranges used in `reserved?/1`, for testing purposes.
"""
@spec ranges() :: [%{cidr: String.t(), name: String.t(), reserved: boolean()}]
def ranges do
```

`doc` gets keyed at the `@spec` line (17), but `def` is at line 18 — no match, so `ranges` (a real,
publicly documented function) rendered `_Not documented in source._`. Same failure, worse, for
`allowed?`, which has a blank line *and* a `@spec` between `@doc` and `def`.

## Fix

After a `@doc` block closes, `extract_doc_comments` now skips forward past blank lines and
single-line `@spec ...`/`@spec(...)` lines before keying the `doc` map — so the key lands on the
real declaration line, not whatever attribute sits between `@doc` and `def`. A multi-line `@spec`
(a wrapped type signature) is not unwound — an accepted, documented limitation, same tradeoff this
module's own doc comment already states for `~S"""` sigils and unmatched `end`/`do` inside strings.

Two new tests reproducing both real shapes found live (`@doc` → `@spec` → `def`, and `@doc` →
blank → `@spec` → `def`). All 30 `elixir_analyzer` tests pass, all 261 `ekos-recovery` tests pass,
full workspace gate (`fmt`/`build`/`clippy -D warnings`) clean.

## What was correctly *not* a bug

Two other symbols on the same page stayed undocumented after the fix, both honestly:

- `combine_guards` (private, 3 clauses, none of which carry a real `@doc` anywhere in the source)
  — correctly "Not documented in source."
- `reserved?` (public, but its real `@doc` precedes a `for %{...} <- @clauses do def reserved?(...)
  ... end` macro-generated comprehension, not a plain `def` directly) — the doc-comment lookup
  requires the very next real declaration line after skipping blanks/`@spec`, and a `for`-wrapped
  macro-generated def is a real, harder case this analyzer's stated scope (structural extraction,
  "not a full parser") doesn't attempt to resolve. Left as an accepted limitation, not silently
  guessed at.

The user also asked why symbol pages don't show a path to the file they're defined in.
`ObjectPageModel`'s `"Based on"` relationship group renders only the object's immediate real
`Contains` parent — for an `ElixirSymbol` that's its owning `ElixirModule`, not the file two hops
up (`File --Contains--> Module --Contains--> Symbol`). This is deliberate, existing, and consistent
across every language this analyzer suite covers (Rust/Python/Elixir symbols all render their
owning module, not their file, one hop up) — not a bug, just a design choice not yet extended to a
direct file link. Not changed this session; flagged to the user as a real, answerable design
question rather than assumed to need fixing.

## Live verification

Fresh `lib/ip`-scoped pipeline (`build`/`recover`/`resolve`/`compile`/`commit --yes`/`docs generate`)
re-run end to end with the rebuilt binary. Confirmed on the real generated page:
`entities/elixirsymbol/al/allowed.md`'s `## Definition` now reads *"Determines if IP is allowed,
i.e. valid and not reserved/private."* (previously "_Not documented in source._"), same for
`ranges`. `combine_guards`/`reserved?` correctly remain undocumented for the reasons above.

## Knowledge Captured

- **A test suite that only exercises `@doc` immediately followed by `def` will never catch the
  single most common real shape (`@doc` → `@spec` → `def`)** — Elixir/Credo's own style guide puts
  `@spec` directly above `def`, `@doc` directly above `@spec`; this is not an edge case, it's the
  convention. Any future doc-comment-adjacent parsing logic for any language needs a test using the
  language's own idiomatic attribute-ordering convention, not just the minimal shape.
- Sixth real-bug-found-live instance this session (after `devlog_90`/`93`/`94`/`95` ×2): a small,
  fast, human-scoped re-run (`lib/ip`, seconds not hours) caught a real bug in code that had 17
  passing unit tests and had already shipped. The scoped-verification workflow this session
  converged on — small real slice, human review of the actual generated page, not just green CI —
  keeps finding real gaps the unit tests didn't, cheaper than the full ~2-3hr run every time.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/elixir_analyzer.rs` | `extract_doc_comments` now skips blank lines and single-line `@spec` lines between `@doc` and the `def`/`defp` it documents; 2 new tests |
| `devlogs/devlog_96.md` | This file |

# Devlog 97 — RFC 0089: real "Defined in" file location on symbol/module entity pages

**Date:** 2026-08-24
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Following `devlog_96`'s `@doc`/`@spec` fix, the user asked again why
`combine_guards-d5032963.md` had neither a `## Definition` nor a path to the file it's defined in.
The Definition half was already answered (a real, genuinely undocumented private function —
correct, not a bug). The file-location half was a real, answerable gap: `ObjectPageModel`'s
`"Based on"` relationship group only ever shows an object's *immediate* real `Contains` parent — for
a symbol that's its owning module, not the file two hops up. Filed RFC 0089 and implemented it: a
real "Defined in" line, resolved from already-compiled data, zero LLM.

## What shipped

- `ekos_docs_gen::build_contains_parent_map`/`resolve_defining_file` — walk the real `Contains`
  parent chain looking for a `File` more than one hop up; `None` when the immediate parent already
  is the file (the module case — already shown elsewhere on the page) or the chain never reaches
  one.
- `ObjectPageModel` gains `source_span: Option<(u32, u32)>` (promoted out of the generic properties
  table, same treatment `description` already gets) and `defined_in_file: Option<String>` (set by
  the caller after the fact, same pattern `prose` uses — `build_object_page_model` only sees one
  object's own relationships, not the whole graph).
- Both `docs.rs` call sites (`--layout objects`, `--layout curated`'s entity pages) build the parent
  map once per run and set `model.defined_in_file` per object.
- Renders as one line under `## Definition`: `**Defined in:** \`tools.ex\` (lines 47–52)`, file-only
  when no `source_span` exists, or nothing at all when neither resolves — never fabricated.

## Live verification

Re-ran `docs generate --layout curated` against the same `analytics/lib/ip` scoped ledger used for
`devlog_96`'s verification (no need to re-run the LLM pass — this is pure rendering over data
already compiled). Confirmed on the real generated pages:

- `allowed?`: `**Defined in:** \`tools.ex\` (lines 47–52)` — both file and real compiled span.
- `combine_guards`: `**Defined in:** \`tools/registry.ex\`` — file only, no line range, correctly:
  it has no compiled `source_span` (multi-clause private function whose first two clauses are
  one-line `, do:` forms, per the existing elixir_analyzer span-capture rule from RFC 0088).
- `Plausible.IP.Tools` (the module page itself): no "Defined in" line at all — its own `"Based on"`
  row already names `tools.ex`, so nothing would be added by repeating it.

5 new `ekos-docs-gen` tests, full workspace gate (`fmt`/`build`/`clippy -D warnings`/
`test --workspace`, 95 `ekos` crate tests including the two `docs.rs` call sites) and
`tests/integration` all clean.

## Knowledge Captured

- **A relationship-grouping convention that only ever shows one hop ("Based on" = immediate
  `Contains` parent) will read as a missing feature to a real reader even when it's a deliberate,
  consistent design** — every language's symbol page did this identically (not a bug specific to
  Elixir), but nobody had asked "where's the file" out loud until a real person reading a real page
  did. Worth treating "why doesn't the page show X" as a legitimate feature request first, not just
  re-explaining the existing design, once the same question is asked a second time.
- Reinforces this session's now-repeated pattern (`devlog_90`/`93`/`94`/`95`×2/`96`): a small, real,
  human-reviewed scope keeps finding real gaps that a green test suite and a full ~2-3hr run alone
  don't surface — this is the second real gap found from the exact same small `lib/ip` scope in one
  sitting, at effectively zero marginal cost since the ledger was already compiled.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0089-symbol-defined-in-file-location.md` | New RFC |
| `ekos/crates/docs-gen/src/lib.rs` | `build_contains_parent_map`/`resolve_defining_file`; `ObjectPageModel::source_span`/`defined_in_file`; Markdown + HTML rendering; 5 new tests |
| `ekos/crates/cli/src/commands/docs.rs` | Both `docs generate` layouts resolve and set `defined_in_file` per entity |
| `devlogs/devlog_97.md` | This file |

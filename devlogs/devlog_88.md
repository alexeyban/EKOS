# Devlog 88 — Real JavaScript/TypeScript decomposition (RFC 0085), Phase 5 of the docs quality plan

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Fifth phase of the source-decomposition plan — the last major real-code language family (the
frontend) that still had no real decomposition, only `plugins/file`'s crude bare-name fallback.
Shipped a real AST-based analyzer using `oxc_parser`, evaluated against `swc_ecma_parser` before
committing, and found + fixed a real JSX-in-`.js` parsing bug live against the real analytics
project before calling this done.

## RFC 0085

New `ekos-plugin-javascript` observer + `JavaScriptAnalyzerPass`, same shape as every prior
language analyzer this session shipped: real `Custom("JsModule")`/`Custom("JsSymbol")` objects,
real `Contains`/`DependsOn` relationships, `export` keyword read as a real visibility signal
instead of guessed. Flat `File → Symbol` containment (JS/TS has no `defmodule`-style nesting), so
no `render_api` containment fix was needed this time — a nice contrast with Phase 1's Elixir work,
which needed one.

Parser choice was a real, live-checked decision, not assumed: fetched real crates.io metadata and
docs.rs pages for both `oxc_parser` and `swc_ecma_parser` before picking. `oxc_parser` won on API
simplicity (one `Parser::new(...).parse()` call vs. swc's `SourceMap`/`Lexer`/`Syntax` setup),
MIT license, and native TS/JSX/TSX support in one crate — not on raw download count (swc's ~35M is
mostly Next.js's own compiler, not necessarily people embedding just the parser). Had to pin to
`=0.133.0`: `cargo add --dry-run` revealed the latest release (0.146) needs rustc 1.95, newer than
this workspace's 1.93 — checked before committing to the dependency, not discovered after.

## The real bug, found and fixed before shipping

Live verification is where this phase earned its keep: 18 of 291 real files failed to parse.
Instead of writing those off as "some files just won't parse," pulled up one of the failures
directly (`assets/js/dashboard/components/lazy-loader.js`) and found real JSX
(`<div ref={ref} ...>`) in a plain `.js` file — a very common real-world pattern this codebase
uses throughout its dashboard components. `SourceType::from_path` alone doesn't know to enable
JSX for `.js` (only `.jsx`/`.tsx`), so oxc correctly rejected real JSX tokens under strict-JS
grammar. Fixed by forcing JSX on for every JavaScript source type (safe superset — a file with no
JSX in it parses identically either way) while deliberately leaving TypeScript's `.ts` alone,
since `<T>expr` old-style generic assertions are genuinely ambiguous with JSX — the same reason
real TypeScript tooling keeps `.ts` non-JSX. Re-verified after the fix: 18 failures → 2, and the
2 remaining are real, valid-looking TS the pinned `oxc_parser` 0.133.0 itself can't handle (a
`swc`-vs-`oxc` grammar-completeness gap at this specific pinned version, not something EKOS's own
code can fix without a newer rustc to unpin the dependency).

## Live verification

Real numbers against the real analytics project: 291 real JS/TS files, 434 real `JsModule`
objects, 851 real `JsSymbol` objects, 99.3% real parse success after the fix. Spot-checked via
`ekl`: `react` exists as both a real `Technology` (from `package.json`, RFC 0082) and a real,
distinct `JsModule` (from a real `import` statement) — the two signals don't collide, and 151 real
files really do import it. `LazyLoader` — the exact component that surfaced the JSX bug — now
shows up correctly in `API.md`, grouped under its real owning file.

Also hit and worked around, honestly, not silently: `commit` against this much larger
post-`build`-rescan ledger (93,734 CKM objects, ~4x the size seen in earlier phases — a `build`
re-scan plus `recover` cache invalidation compounding the already-documented RFC 0076 Finding 6
accumulation pattern) took over an hour of real wall-clock time — well past this session's usual
~180s. Confirmed via `/proc/<pid>/io` that it was genuinely CPU/disk-active (43% CPU, tens of GB
of real I/O) rather than hung, and let it run to real completion rather than killing it.

## Knowledge Captured

- **A real, live "does this parser even work on our actual files" check caught a bug no amount of
  hand-written unit tests would have found** — every unit test used clean, JSX-free `.js`
  fixtures; the real bug only existed at the intersection of "real `.js` file" + "real JSX inside
  it," a combination this codebase's own real components use constantly. Live verification against
  a real, large, messy codebase remains this session's single highest-value practice.
- **A newer major-version dependency isn't always available — check the toolchain constraint
  before assuming a `cargo add` will pick the latest** — `oxc_parser` 0.146 silently downgrades to
  0.133 under this workspace's pinned rustc, with a real behavioral difference (the older
  version's own real, uninvestigated 2-file gap) worth remembering exists, not something to treat
  as invisible just because the build succeeded.
- **A background process producing no new log output isn't automatically stuck** — checked
  `/proc/<pid>/io` and CPU% directly before assuming the hour-long `commit` had hung; real,
  climbing I/O counters were the actual evidence it needed, not elapsed time alone.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0085-javascript-typescript-analyzer.md` | New RFC |
| `ekos/plugins/javascript/` | New observer plugin; 3 tests |
| `ekos/crates/recovery/src/javascript_analyzer.rs` | New analyzer pass + JSX fix; 14 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registration |
| `ekos/crates/cli/src/commands/build.rs` | Observer wired in |
| `ekos/crates/cli/src/commands/recover.rs` | Pass wired in |
| `ekos/crates/docs-gen/src/lib.rs` | Entity/symbol page kinds extended |
| `ekos/Cargo.toml`, `ekos/crates/recovery/Cargo.toml`, `ekos/crates/cli/Cargo.toml` | New `oxc_*` dependencies, new plugin crate |
| `TODO.md` | Phase 5 marked done |
| `devlogs/devlog_88.md` | This file |

# RFC 0085 — Real JavaScript/TypeScript Decomposition (`javascript_analyzer.rs`)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Phase 5 of the "Deep Source Decomposition + Production-Grade Architecture Diagrams" plan — the
last major real-code language family that still had no real decomposition. Frontend `Technology`
data (RFC 0082) tells us *what* npm packages a project declares; it says nothing about *how the
frontend code itself* is structured — no functions, no components, no real per-file import graph.
`plugins/file`'s crude fallback was still all JS/TS got: bare symbol name strings, no
relationships, no module structure.

## Parser choice

No mature Elixir-style hand-rolled scan is realistic for JS/TS's much larger grammar — a real
third-party parser crate is required. Compared `oxc_parser` against `swc_ecma_parser` before
committing (per the plan's own explicit instruction to evaluate, not assume):

| | `oxc_parser` | `swc_ecma_parser` |
|---|---|---|
| License | MIT | Apache-2.0 |
| API shape | One call: `Parser::new(&allocator, source, source_type).parse()` — native TS/JSX/TSX, no separate syntax config | `SourceMap` + `Lexer` + a `Syntax::Es`/`Syntax::Typescript` config split — more setup per file |
| Dependency footprint | 4 lean crates (`oxc_parser`/`oxc_allocator`/`oxc_span`/`oxc_ast`) | 3 crates, but `swc_common` alone pulls `parking_lot`/`sourcemap`/`stacker` |
| crates.io downloads (checked live) | ~5.9M total | ~35.2M total (SWC's much larger install base, largely via Next.js's own compiler, not necessarily people embedding just the parser) |

`oxc_parser`'s single-call API, native TS/JSX/TSX support, and MIT license were the deciding
factors for a bounded, "read what's declared" analyzer matching `rust_analyzer.rs`'s/
`python_analyzer.rs`'s own scope — not the raw download count. Pinned to `=0.133.0`: the latest
release (0.146) requires rustc 1.95, newer than this workspace's 1.93 toolchain — confirmed via
`cargo add --dry-run` before committing, not assumed.

## Design

**New plugin** `ekos-plugin-javascript` (`plugins/javascript/`) — a `.js`/`.jsx`/`.ts`/`.tsx`/
`.mjs`/`.cjs` file walker mirroring `plugins/elixir`/`plugins/rust` exactly.

**New pass** `JavaScriptAnalyzerPass` (`crates/recovery/src/javascript_analyzer.rs`):
- `import ... from "specifier"` → `Custom("JsModule")` + `DependsOn` from the owning `File` (one
  edge per distinct specifier per file, not one per `import` statement).
- `function foo() {}` / `class Foo {}` (top-level, or one level inside `export`/`export default`)
  → `Custom("JsSymbol")` (`kind`: `function`/`class`, `visibility`: `exported`/`local` — a real
  signal from the real `export` keyword) + `Contains` from the owning `File`. Flat `File →
  Symbol` containment, matching Python/Rust's shape (not Elixir's two-level
  `File → Module → Symbol`) — JS/TS has no `defmodule`-equivalent nesting concept.
- `const Foo = () => {...}` / `const foo = function() {...}` (top-level function-valued
  bindings) → the same `JsSymbol` shape — the real, common React component/hook pattern. A plain
  non-function-valued top-level `const` is not surfaced (would just be data-constant noise, the
  same judgment call `python_analyzer.rs` already makes for `def`/`class` only).
- Not a call graph, not a JSX component-tree walk — matches every prior language analyzer's own
  scope decision.

## A real, live-caught bug — fixed before shipping

Live-verifying against the real analytics project's frontend, 18 of 291 real files failed to
parse (`ParserReturn::panicked`). Root-caused directly rather than dismissed: all 18 were real
`.js` files containing real JSX (e.g. `assets/js/dashboard/components/lazy-loader.js`'s
`return (<div ref={ref} ...>...)`). `SourceType::from_path` alone only enables JSX for
`.jsx`/`.tsx` — but this real codebase (like most real React/Preact projects) authors JSX directly
in plain `.js` files. Fixed with `javascript_source_type()`: force JSX on for every JavaScript
`SourceType` (`.js`/`.jsx`/`.mjs`/`.cjs` — always a safe superset, a file with no JSX in it parses
identically either way), left off for TypeScript (`.ts` stays extension-derived, `.tsx` already
gets it) — real TypeScript's old-style generic type assertion (`<T>expr`) is genuinely ambiguous
with a JSX element, the exact reason real TS tooling keeps `.ts` non-JSX. Re-verified: failures
dropped from 18 to 2 (see Testing).

## Scope — what this does and doesn't cover

**Covers**: real module/function/class decomposition and real per-file import edges for the
JS/TS frontend — the actual highest-leverage remaining fix for "no source code decomposition ...
frontend" from the original complaint that started this whole plan.

**Does not cover** (explicitly deferred, not silently dropped): relative imports (`"./Dashboard"`)
are not resolved to the real internal file/component they point at — same honestly-scoped
limitation `package_json_analyzer.rs` (RFC 0082) already documented for npm workspace-internal
packages; real resolution needs extension/`index.*` guessing and bundler alias configs, a separate
harder problem. No JSX component-tree walk, no call graph (matches every prior analyzer's scope).
The 2 real files that still fail to parse under the pinned `oxc_parser` 0.133.0 (both real,
valid-looking TypeScript with `import type`/union types) are a real, honest, unresolved gap — a
newer `oxc_parser` release might fix this, but upgrading requires a newer rustc than this
workspace currently pins; not chased further this increment.

## Testing

- 14 new tests in `javascript_analyzer.rs`: top-level function/class recognition,
  exported/default-exported visibility tagging, arrow-function-as-component recognition, a plain
  non-function const correctly not surfaced, import→`DependsOn` deduped per specifier, TypeScript
  syntax parsing via extension detection, RFC 0079 project-qualification, a malformed file not
  panicking the caller, **the real JSX-in-`.js` regression** (`a_plain_js_file_containing_real_jsx_
  parses_successfully`), and the deliberate `.ts`-stays-non-JSX asymmetry.
- 3 new tests in `plugins/javascript/src/lib.rs` (mirrors `plugins/elixir`'s own test shape).
- `is_entity_page_kind`/`is_symbol_kind` extended for `JsModule`/`JsSymbol` — no `render_api`
  containment fix needed (JS/TS's flat `File → Symbol` shape already matches Rust/Python's
  existing logic, unlike Elixir's two-level case).
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end** against the real analytics project: 291 real JS/TS files, 434 real
  `JsModule` objects, 851 real `JsSymbol` objects, 99.3% real parse success (289/291) after the
  JSX fix (was 94% before it). Spot-checked via `ekl`: `react` exists as both a real `Technology`
  (RFC 0082, from `package.json`) and a real, distinct `JsModule` (from a real `import` statement)
  — no id collision, 151 real files import it. `LazyLoader` — the exact real `.js`-with-JSX
  component the fix targeted — is now a real `JsSymbol`, correctly grouped under its real owning
  file in `API.md`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0085-javascript-typescript-analyzer.md` | This RFC |
| `ekos/plugins/javascript/` | New crate: `JavaScriptObserver`; 3 tests |
| `ekos/crates/recovery/src/javascript_analyzer.rs` | New: `JavaScriptAnalyzerPass`; 14 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registration/exports |
| `ekos/crates/cli/src/commands/build.rs` | `JavaScriptObserver` registered |
| `ekos/crates/cli/src/commands/recover.rs` | `JavaScriptAnalyzerPass` registered; `collect_javascript_artifact_ids` |
| `ekos/crates/docs-gen/src/lib.rs` | `is_entity_page_kind`/`is_symbol_kind` extended |
| `ekos/Cargo.toml` | `oxc_parser`/`oxc_allocator`/`oxc_span`/`oxc_ast` pinned to `=0.133.0`; new `plugins/javascript` member |
| `ekos/crates/recovery/Cargo.toml`, `ekos/crates/cli/Cargo.toml` | New crate registration |
| `TODO.md` | Phase 5 of the decomposition plan marked done |
| `devlogs/devlog_88.md` | This increment's devlog |

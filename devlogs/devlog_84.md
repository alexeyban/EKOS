# Devlog 84 — Real Elixir decomposition (RFC 0081), Phase 1 of the docs quality plan

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

First phase of a plan approved after generating architecture docs for the real Plausible Analytics
project and finding they didn't read as professional documentation — no backend/frontend/database
decomposition, mostly flat lists. Root cause traced precisely: EKOS's only two real AST-based
analyzers are for Rust and Python; Elixir (100% of Plausible's application logic) got nothing but a
crude bare-name text scan. Shipped a real Elixir analyzer, closing that gap.

## RFC 0081

New `ekos-plugin-elixir` observer (mirrors `plugins/rust` exactly) + new `ElixirAnalyzerPass`, a
hand-written structural scanner (no mature Elixir-grammar Rust crate exists) recognizing
`defmodule`/`def`/`defp`/`alias`/`import`/`use`/`require`, producing real `Custom("ElixirModule")`/
`Custom("ElixirSymbol")` objects with real `Contains`/`DependsOn` relationships — the same shape
`rust_analyzer.rs`/`python_analyzer.rs` already established, scoped the same way
`python_analyzer.rs` was (module/dependency structure, not a call graph).

The real design work was block-depth tracking: Elixir's `do`/`end` blocks can span multiple lines
(guard clauses) or collapse onto one (`Enum.each(x, fn y -> y end)`), and getting this wrong
desyncs which module a later `def` gets attributed to. Built a generic depth stack keyed only on
the actual `do`/`fn`/`end` tokens present, not the keywords that precede them — verified directly
with two adversarial test fixtures (a guard spanning to a later line, an inline `fn...end`), both
passing on the first real implementation.

Found and fixed a real integration gap while wiring the new data into `API.md`: the existing
`render_api` only ever resolved a symbol's container via a direct `File → Symbol` `Contains` edge
— correct for Rust/Python, but Elixir's real shape is one level deeper
(`File → Module → Symbol`), so every real Elixir symbol was silently falling into the
"containing file not compiled" bucket until this was fixed.

**Deliberately deferred, not silently cut**: Phoenix-convention role tagging
(controller/LiveView/context by directory path) was designed, then cut from this increment after
noticing a real correctness risk in the natural implementation (a bare dependency-target module
object could permanently out-live its later, richer self-declaration under the existing
first-occurrence-wins dedup). Left for the System Decomposition phase, where a render-time
derivation (matching RFC 0075's Data Domains pattern) is the more promising design anyway.

## Live verification

Real numbers against the real analytics project: 1231 Elixir files, ~1260 real modules, ~4800 real
functions. Spot-checked two known real files directly against the compiled ledger:
`Plausible.Auth.Password` → exactly its 3 real functions; `PlausibleWeb.AuthController` → exactly 9
real dependency edges, matching the real source read directly, not just plausible-looking counts.

Found one real, honest, not-a-bug finding along the way: `ekos resolve`'s preview command
hard-failed on a genuine cross-kind name coincidence (`error` — both a real module and, separately,
several real `error/N` helper functions across unrelated modules, unsurprising in ~4800 real
functions). `ekos compile` — the actual pipeline step — was unaffected; the real ledger stayed
correct across two independent verification cycles (`Table` count held at 57 both times).

## Knowledge Captured

- **A stale release binary caused a real false negative mid-session** — rebuilt, tested against the
  real project, saw the *old* unlinked API.md output, and briefly suspected the fix hadn't worked.
  Source mtime was newer than the binary's (a `cargo fmt` after the build). Same class of issue
  this session already knew about for pass-level caching; worth remembering it applies to the
  compiled binary itself too, not just `.ekos/artifacts/pass-manifests`.
- **A code change to an analyzer needs the same `pass-manifests` cache invalidation a data change
  does** — re-ran `recover` against the real project after fixing a property on `ElixirSymbol`
  objects and got the *old* data back, because Phase 13's pass cache keys on artifact ids
  (unchanged — the source files didn't change), not on the analyzer's own code. Had to move
  `pass-manifests` aside, matching this session's own established workaround.
- **Two-level `Contains` (`File → Module → Symbol`) is a real, legitimate shape some languages need
  that Rust/Python's flat `File → Symbol` didn't anticipate** — worth checking explicitly for any
  future language analyzer, not assuming every language's containment hierarchy is one level deep.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0081-elixir-analyzer.md` | New RFC |
| `ekos/plugins/elixir/` | New observer plugin; 3 tests |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | New analyzer pass; 11 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registration |
| `ekos/crates/cli/src/commands/build.rs` | Observer wired in |
| `ekos/crates/cli/src/commands/recover.rs` | Pass wired in |
| `ekos/crates/docs-gen/src/lib.rs` | Entity pages + `API.md` two-level containment fix; 1 new test |
| `ekos/Cargo.toml`, `ekos/crates/cli/Cargo.toml` | Workspace registration |
| `TODO.md` | Phase 1 marked done |
| `devlogs/devlog_84.md` | This file |

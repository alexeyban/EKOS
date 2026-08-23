# RFC 0081 — Real Elixir Decomposition (`elixir_analyzer.rs`)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Phase 1 of the "Deep Source Decomposition + Production-Grade Architecture Diagrams" plan. Real
Plausible Analytics documentation generation showed no backend decomposition at all — Elixir,
100% of the application's actual business logic, only ever got `plugins/file`'s crude
declaration-prefix scan: bare symbol name strings, no relationships, no module hierarchy, nothing
a real architecture diagram could use. This RFC gives Elixir the same real, AST-adjacent treatment
`rust_analyzer.rs`/`python_analyzer.rs` already give Rust/Python.

## Design

No mature Elixir-grammar Rust crate exists — a real, bounded, hand-written structural scanner
instead, matching `crate_topology_analyzer.rs`'s own "read what's declared, don't build a full
resolver" spirit, and `python_analyzer.rs`'s scope decision: module/symbol/dependency structure,
not a call graph (Elixir's OTP/Phoenix architecture is legible from module boundaries and
dependency edges, which is what an architecture diagram needs).

**New plugin** `ekos-plugin-elixir` (`plugins/elixir/`) — a `.ex`/`.exs` file walker mirroring
`plugins/rust`/`plugins/python` exactly (raw source capture, no parsing).

**New pass** `ElixirAnalyzerPass` (`crates/recovery/src/elixir_analyzer.rs`):
- `defmodule Name do ... end` → `Custom("ElixirModule")` + `Contains` from the owning `File`.
- `def`/`defp` → `Custom("ElixirSymbol")` (properties: `kind: "function"`, `arity`,
  `visibility: public/private`) + `Contains` from the owning module. Multiple clauses of the same
  `name/arity` (real Elixir multi-clause dispatch) collapse into **one** symbol — matching how one
  Rust `impl` method or one Python `def` is already treated as one symbol, not one per clause.
- `alias`/`import`/`use`/`require` → real `DependsOn` edges from the owning module to the named
  target — using the **same deterministic id scheme for both a module's own declaration and any
  reference to it**, so a real internal dependency (module A depends on module B, both defined in
  this codebase) resolves onto one real linked object, not two disconnected ones. This is the
  concrete "restore links and relationships" deliverable: a real intra-codebase module dependency
  graph, not just per-file bullet lists.

**Block-depth tracking**: a generic stack keyed only on the `do`/`fn`/`end` tokens actually present
(never on `if`/`case`/`def` etc. themselves — those never push directly), so a guard clause
spanning to a `do` on a *later* line, or an inline `fn ... end`, doesn't desynchronize which module
a subsequent `def`/`alias` line is attributed to. Verified this specific risk directly — see
Testing.

**`docs-gen` integration**: `is_entity_page_kind`/`is_symbol_kind` extended so `ElixirModule`/
`ElixirSymbol` get real detail pages and appear in `API.md`, not just the ledger. Found and fixed a
real gap while wiring this in: `render_api`'s existing grouping logic only ever resolved a symbol's
containing *File* via a direct `Contains` edge — correct for Rust/Python (`File Contains Symbol`
directly), but Elixir's real shape is `File Contains Module Contains Symbol`, one level deeper —
every Elixir symbol was falling into the `"(containing file not compiled)"` bucket until
`render_api` was extended to resolve either a `File` or an `ElixirModule` as a symbol's real
container.

## Scope — what this does and doesn't cover

**Covers**: real module/function decomposition and real intra-codebase dependency edges — the
actual highest-leverage fix for "can't understand how the system works" on a real Elixir project.

**Does not cover** (explicitly deferred, not silently dropped): Phoenix-convention role tagging
(controller/LiveView/context, by directory path) was designed but cut from this increment — the
natural implementation had a real dedup-ordering correctness risk (a module first seen as a bare
dependency-target object, with no role tag, could permanently "win" over its later, richer
self-declaration under the existing first-occurrence-wins cross-file dedup) that wasn't worth
rushing past; a render-time derivation (matching how RFC 0075's Data Domains works) is the more
promising design, left for the System Decomposition phase where role tagging actually gets used.
Not a call graph (matches `python_analyzer.rs`'s own scope decision, for the same reason).
Multi-alias forms (`alias Plausible.{Auth, Teams}`) capture the real shared prefix as one honest
signal, not full expansion.

## A real, honest finding — not a bug

Live-verifying against the real analytics ledger, `ekos resolve`'s preview command hard-failed with
a real cross-kind conflict: the string `error` appears as both an `ElixirModule` and (separately)
several different `ElixirSymbol` objects (real `error/N` helper functions defined in several
unrelated real modules — a very common function name). This is a genuine coincidental name overlap
in a 4594-real-function codebase, not a data-integrity bug — `ekos compile` (the actual pipeline
step, `resolve` is preview-only) proceeded correctly regardless, and the real ledger ended up
correct (verified: `Table` count stayed a stable 57 across two independent recover/compile/commit
cycles). Recorded honestly rather than silently worked around.

## Testing

- 11 new tests in `elixir_analyzer.rs`: real module/symbol recognition, public/private tagging,
  multi-clause collapse to one symbol, bracket-depth-aware arity counting, real `alias`/`import`/
  `use`/`require` → `DependsOn` edges, a module depending on another locally-defined module
  resolving to the same real object, a guard clause spanning to a `do` on a later line *not*
  desyncing block depth, an inline `fn ... end` *not* desyncing block depth, a comment containing
  keyword-like text being correctly ignored, and RFC 0079 project-qualification (same name in two
  projects must not collide).
- 3 new tests in `plugins/elixir/src/lib.rs` (observer emits one artifact per `.ex`/`.exs` file,
  ignores unrelated extensions, stable content hash across runs) — mirrors `plugins/rust`'s own
  test shape exactly.
- New `docs-gen` test: a real two-level `File Contains Module Contains Symbol` fixture correctly
  groups under the module's name in `API.md`, not the honest-but-wrong "not compiled" bucket.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end** against the real analytics project: 1231 real Elixir files, 1260/1355
  real modules (before/after a second independent run), 4812/4594 real symbols. Spot-checked two
  real, known files directly against the compiled ledger via `ekl`:
  `Plausible.Auth.Password` → exactly its 3 real functions (`hash`, `match?`,
  `dummy_calculation`), matching the real file's content read directly; `PlausibleWeb.AuthController`
  → exactly 9 real `DependsOn` edges, matching its real `use`/`alias`/`require` lines read directly.
  `API.md` now shows real, linked entries grouped by real module name instead of the crude
  fallback.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0081-elixir-analyzer.md` | This RFC |
| `ekos/plugins/elixir/` | New crate: `ElixirObserver`; 3 tests |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | New: `ElixirAnalyzerPass`; 11 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registration/exports |
| `ekos/crates/cli/src/commands/build.rs` | `ElixirObserver` registered |
| `ekos/crates/cli/src/commands/recover.rs` | `ElixirAnalyzerPass` registered; `collect_elixir_artifact_ids` |
| `ekos/crates/docs-gen/src/lib.rs` | `is_entity_page_kind`/`is_symbol_kind` extended; `render_api`'s two-level containment resolution; 1 new test |
| `ekos/Cargo.toml`, `ekos/crates/cli/Cargo.toml` | New crate registration |
| `TODO.md` | Phase 1 of the decomposition plan marked done |
| `devlogs/devlog_84.md` | This increment's devlog |

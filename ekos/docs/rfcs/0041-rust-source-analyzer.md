# RFC 0041 — Rust Source Analyzer (real symbols + import graph + real Calls edges)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-08

---

## Motivation

Following RFC 0038's Phase 2 (Python/PySpark analyzer, RFC 0040), the user asked for the same
treatment for Rust source: real AST parsing, same architectural discipline, same real-data-driven
scoping. Rust source has no equivalent of PySpark's DataFrame method chains, so there is nothing
honest to lower into the Transformation IR (RFC 0027) the way Source/Filter/Join/Aggregate/Sink
worked for PySpark — forcing generic Rust iterator/builder chains into that shape would be exactly
the kind of guess this project has consistently avoided. This was confirmed directly with the user
rather than assumed, alongside two other options (force-fitting iterator chains into the
Transformation IR, or a minimal symbols-only scope) — the user chose the option below.

Instead, the headline capability is a **real function-call graph**: `RelationshipKind::Calls`
(`ekos/crates/kir/src/lib.rs:131`) has existed in the KIR since early in the project but has never
been populated by any analyzer — Pentaho, SQL, and Python all only ever produce `DependsOn`/
`Contains`/`FeedsInto`. Rust function-call expressions are cheap to recognize correctly via real
AST parsing, and this project's own `ekos/` Cargo workspace (~50 crates) is an immediately
available, large, real test corpus — no external repo dependency required, unlike Python's
`azure-databricks-project` fixture.

### Parser choice: `syn`, verified not assumed

`cargo add syn --features full,extra-traits --dry-run` in a scratch project resolves cleanly:
`syn v3.0.3`, pure Rust, no C-dependency — verified directly, matching the discipline already
applied twice this project (`rustpython-parser` over `tree-sitter-python` for Python; `pg_query`'s
rejection in RFC 0031). `syn::parse_file` parses a whole `.rs` file into `syn::File { items, .. }`.
The `visit` feature adds `syn::visit::Visit`, used to walk expressions inside a recognized
function/method body to find `Expr::Call`/`Expr::MethodCall` nodes without hand-writing a full
recursive walker.

## Scope

- `ekos/plugins/rust/` (`RustObserver`) — mirrors `plugins/python/src/lib.rs`: own `WalkDir` walk,
  filters to `.rs`, stores raw source verbatim under `data["source"]`, `connector_name: "rust"`.
- `ekos/crates/recovery/src/rust_analyzer.rs` (`RustAnalyzerPass`) — mirrors `python_analyzer.rs`.
  Per file, parsed via `syn::parse_file`:
  - `Item::Use` → `KirObject(Custom("RustModule"))` + `DependsOn` from the file's `KirId`.
  - `Item::Fn`, and associated `fn`s inside `Item::Impl` blocks, plus `Item::Struct`/`Item::Enum`/
    `Item::Trait` → `KirObject(Custom("RustSymbol"))` + `Contains` from the file.
  - Real intra-file `Calls` edges: walk each recognized function/method body for
    `Expr::Call`/`Expr::MethodCall`, resolve the callee name against symbols recognized in the
    *same file only*. Unresolved calls (external crates, std library) are simply not recorded.
  - No Transformation IR involvement — confirmed non-goal for this analyzer.

## Non-goals

- Lowering anything into the Transformation IR (RFC 0027) — no honest Rust-source analog exists.
- Cross-file / cross-crate call resolution — deferred, same posture as Python's interprocedural
  chain-tracing non-goal.
- Macro-expanded code — `syn` parses syntax only, no macro expansion.
- Trait-dispatch resolution — only literal same-file symbol name matches are recorded.

## What already exists and is reused

- `plugins/python/src/lib.rs` — structural template for `RustObserver`.
- `crates/recovery/src/python_analyzer.rs` — structural template for `RustAnalyzerPass`.
- `crates/cli/src/commands/recover.rs`'s `collect_python_artifact_ids` — template for
  `collect_rust_artifact_ids`.
- `build.rs`'s `ObjectKind::File` KirId scheme — reused so relationships attach to the real,
  already-existing `File` KIR object.
- `RelationshipKind::Calls` (`crates/kir/src/lib.rs:131`) — already defined, never populated
  until now.

## Design

**`RustObserver`** (`plugins/rust`) — mirrors `PythonObserver` exactly: own `WalkDir` walk,
filters to `.rs`, stores raw text verbatim under `data["source"]`, `connector_name: "rust"`.

**`RustAnalyzerPass`** (`crates/recovery/src/rust_analyzer.rs`):
- `Item::Use` flattened (`Path`/`Group`/`Rename`/`Glob` variants of `UseTree`) into one or more
  real module-path strings → `RustModule` objects, deduped per run via `HashSet<KirId>`.
- Function/method/struct/enum/trait defs → `RustSymbol` objects with `properties["kind"]`.
  Methods qualified as `Type::method` in their KirId derivation to disambiguate same-named methods
  across different `impl` blocks in one file.
- `Expr::Call(ExprCall { func: Expr::Path(p), .. })` where `p` resolves (by simple name) to a
  symbol recognized in the same file → `Calls` edge from the calling symbol to the callee symbol.
- `Expr::MethodCall(ExprMethodCall { method, .. })` where `method` resolves (by simple name) to a
  method recognized in the same file → `Calls` edge.

## Alternatives Considered

- **Force-fit Rust iterator/builder chains into the Transformation IR** — rejected by the user:
  no honest data-movement semantics exist for generic Rust chains the way PySpark's DataFrame API
  has for ETL.
- **Symbols + imports only, no call graph** — rejected by the user: the call graph is the
  differentiated, novel capability (first real `Calls` data in the project).
- **`tree-sitter-rust`** — not evaluated in depth; `syn` is pure Rust, already the de facto
  standard for Rust source parsing, and resolves cleanly against this workspace's toolchain.

## Open Questions

- [ ] Cross-file/crate call resolution — deferred, candidate future phase if real usage shows
      intra-file-only recognition is insufficient.
- [ ] Whether `use` flattening should stay per-imported-item or coarsen to per-crate — decided
      during implementation by checking which is more useful against this repo's own `use` blocks.

## Real-data testing — found a real bug in shared identity-resolution code

Ran the real pipeline (`ekos build && ekos recover && ekos resolve && ekos compile && ekos commit`)
against **this EKOS workspace itself** (`ekos/`, ~50 crates) from a scratch workspace pointing
`[observe] paths` at the repo root. `recover` alone produced real, non-trivial numbers: **118 Rust
files analyzed, 1270 symbols recovered, 715 `Calls` edges recovered.**

Inspecting the committed ledger via `ekos ekl` surfaced a real bug, not in the new analyzer, but in
the shared `DefaultResolver` (`crates/identity/src/lib.rs`) that every recovery pass feeds into at
`compile` time: many `RustSymbol` objects sharing a long name suffix — this repo genuinely has
`ConfluenceAnalyzerPass`, `PentahoAnalyzerPass`, `PythonAnalyzerPass`, `GitAnalyzerPass`, etc., each
defined in a different file — scored above `DefaultResolver`'s 0.85 Jaro-Winkler merge threshold
against each other (`structural_score`'s same-kind 1.0 fallback, since `RustSymbol` objects carry no
`columns` property to differentiate on, added a flat +0.3 on top of already-high name similarity).
Several genuinely distinct structs were silently merged into one canonical object and dropped from
the ledger — first run: 460 objects/1902 relationships after `compile`, several real
`<X>AnalyzerPass` structs missing entirely from the ledger, confirmed via `ekl` (`FIND Object WHERE
kind = 'RustSymbol' AND name CONTAINS 'AnalyzerPass'` returned only 4 of ~11 expected). This is the
exact same failure shape already fixed once for `Custom("Section")` (RFC 0024, devlog 27) and once
for `Custom("TransformNode")` (RFC 0027/0028) — both already carry a blanket kind-exclusion from
`DefaultResolver`'s blocking loop with the same rationale: objects already deterministically
identified by (file, index/path) can never legitimately be the same real-world entity, so name
similarity should never be allowed to merge them.

**Fix**: added `RustSymbol`/`RustModule` to that same blanket exclusion in
`crates/identity/src/lib.rs`, and — since `PythonSymbol`/`PythonModule` (RFC 0038/0040) share the
identical `KirObject` shape and are exposed to the identical failure mode, just never noticed
because RFC 0040's real-data testing only checked `TransformNode` counts, which were already
excluded — included them too. Re-running the full pipeline after the fix: `resolve`'s merge
proposals dropped from 190 to 2 (pairs compared 21,292 → 146), and `compile` produced **1812
objects / 3749 relationships** (up from 460/1902) — the previously-missing structs are back.
Verified one real `Calls` edge end-to-end via `ekl`: `PentahoAnalyzerPass::run` → `parse_kettle_xml`,
confirmed against the actual call site at `pentaho_analyzer.rs:133`.

One narrow, expected residual: `resolve` still reports one real cross-kind conflict
(`'observeerror' appears as multiple kinds: RustSymbol, RustModule`) — a `RustModule` import target
and a `RustSymbol` enum definition normalizing to the same string. This is `resolve`'s cross-kind
*conflict detector* (a separate code path from the blocking/merge loop above) correctly flagging an
ambiguous case for manual review rather than silently guessing, exactly as RFC 0007 designed it to.

## Testing

Unit tests built from real snippets in this repo's own source: imports → `DependsOn`, fn/struct/
enum/trait defs → `RustSymbol` objects, a same-file function call → `Calls` edge, a call to an
external/std function → no edge recorded, a same-file method call → `Calls` edge with correct
qualified name, an ambiguous same-name method across two types → not recorded, a `Self::` call
resolved via the enclosing `impl` type, a call inside a macro invocation → not recorded.

## Acceptance Criteria

- [x] `RustObserver` discovers and stores real `.rs` file content verbatim — 3 unit tests pass.
- [x] `RustAnalyzerPass` recognizes imports and fn/struct/enum/trait/impl-method defs, producing
      correct `DependsOn`/`Contains` relationships — 9 unit tests pass.
- [x] Real intra-file `Calls` edges recovered correctly (true positives verified against real call
      sites), external/std calls correctly not recorded — unit tests + real run against `ekos/`
      (715 `Calls` edges recovered from 118 files; spot-checked `PentahoAnalyzerPass::run` →
      `parse_kettle_xml` against the real source).
- [x] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` pass.
- [x] Found and fixed a real pre-existing bug in `DefaultResolver` (shared by every analyzer, not
      new to this RFC) via real-data testing — see above.

## Files Changed

| File | Change |
|---|---|
| `ekos/plugins/rust/Cargo.toml`, `src/lib.rs` | new — `RustObserver` |
| `ekos/crates/recovery/src/rust_analyzer.rs` | new — `RustAnalyzerPass` |
| `ekos/crates/recovery/src/lib.rs` | `+pub mod rust_analyzer;` + re-export |
| `ekos/crates/recovery/Cargo.toml`, `ekos/Cargo.toml` | `+syn`, `+plugins/rust` member |
| `ekos/crates/cli/src/commands/build.rs` | `+RustObserver` in the observer list |
| `ekos/crates/cli/src/commands/recover.rs` | `+collect_rust_artifact_ids` + pass registration |
| `ekos/crates/cli/Cargo.toml` | `+ekos-plugin-rust` path dependency |
| `ekos/crates/identity/src/lib.rs` | bug fix — exclude `RustSymbol`/`RustModule`/`PythonSymbol`/`PythonModule` from `DefaultResolver`'s merge blocking, same class of fix as Section/TransformNode |

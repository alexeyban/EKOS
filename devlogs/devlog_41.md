# Devlog 41 — RFC 0041: Rust source analyzer, and a real identity-resolution bug found dogfooding on EKOS itself

**Date:** 2026-08-08
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Following RFC 0038 Phase 2 (Python/PySpark analyzer, RFC 0040/devlog_40), the user asked for the
same treatment for Rust source. Rust has no honest equivalent of PySpark's DataFrame method chains,
so there's nothing to lower into the Transformation IR (RFC 0027) — confirmed with the user via
`AskUserQuestion` rather than assumed, and the chosen scope was instead a **real function-call
graph**: `RelationshipKind::Calls` has existed in the KIR since early in the project but had never
been populated by any analyzer. New `plugins/rust` (`RustObserver`, mirrors `plugins/python`) and
`crates/recovery/src/rust_analyzer.rs` (`RustAnalyzerPass`, real AST parsing via `syn`) recognize
`use` imports, fn/struct/enum/trait/impl-method definitions, and real intra-file `Calls` edges.
Real-data testing ran the full pipeline against **this EKOS workspace itself** (~50 crates, no
external repo dependency) and found a real bug — not in the new analyzer, but in the shared
`DefaultResolver` identity-resolution code every analyzer feeds into at `compile` time — where
distinct `RustSymbol` objects with a shared name suffix (very common in this codebase:
`ConfluenceAnalyzerPass`, `PentahoAnalyzerPass`, `GitAnalyzerPass`, ...) were silently merged and
dropped from the ledger. Fixed by extending an existing blanket-exclusion mechanism (already used
for `Section`/`TransformNode`) to cover `RustSymbol`/`RustModule` and, since the same bug applied
retroactively, `PythonSymbol`/`PythonModule` too.

---

## RFC 0041 — Rust Source Analyzer

### Problem / motivation

RFC 0038's roadmap never scoped a Rust analyzer — this was a new, direct user request layered
alongside the roadmap. The design question that mattered: Rust source has no equivalent of
PySpark's fluent DataFrame chains, so forcing generic Rust iterator/builder chains into the
Transformation IR's Source/Filter/Join/Aggregate/Sink shape would be exactly the kind of guess this
project has consistently avoided. Presented three options to the user via `AskUserQuestion`: (a)
real function-call graph recognition, (b) force-fit iterator chains into the Transformation IR, (c)
symbols/imports only, no relationships. User chose (a) — the differentiated, honest capability:
`RelationshipKind::Calls` (`crates/kir/src/lib.rs:131`) had existed in the KIR from early on but no
analyzer (Pentaho, SQL, Python) had ever populated it.

### What was built

| Component | Location |
|---|---|
| Observer plugin | `ekos/plugins/rust/` (new) — `RustObserver`, mirrors `plugins/python` exactly |
| Analyzer pass | `ekos/crates/recovery/src/rust_analyzer.rs` (new) — `RustAnalyzerPass` |
| RFC | `ekos/docs/rfcs/0041-rust-source-analyzer.md` (new) |

`RustAnalyzerPass` parses each file with `syn::parse_file`. `use` imports flatten (`Path`/`Name`/
`Rename`/`Group`/`Glob` variants of `UseTree`) into `KirObject(Custom("RustModule"))` + `DependsOn`
edges. `fn`/`struct`/`enum`/`trait` items, and associated `fn`s inside `impl` blocks, become
`KirObject(Custom("RustSymbol"))` + `Contains` edges, qualified as `Type::method` for methods to
disambiguate same-named methods across different `impl` blocks in one file.

`Calls` edges are recognized only for call expressions resolvable to a symbol defined in the *same
file* — the same honesty scoping Python's Phase 2 applied to interprocedural chain tracing:
- A bare function call (`foo(...)`) resolves against top-level `fn`s in the file.
- An associated-function call (`Type::method(...)` or `Self::method(...)`) resolves against `impl`
  methods in the same file — `Self::` is resolved via the enclosing `impl` block's own type name,
  not treated as a literal type name.
- A method call (`x.method(...)`) resolves only when the method name is unambiguous among every
  method recognized in the file — if two different types in the file define a same-named method,
  neither call site is guessed at.

Calls to anything outside the file (external crates, std library, trait default methods,
macro-expanded code — `syn` never expands macros) are simply not recorded — no placeholder needed,
since `Calls` is a plain relationship, not a `TransformNode` graph requiring every step accounted
for.

### Implementation details worth remembering

- **Parser choice verified, not assumed**: `cargo add syn --features full,extra-traits --dry-run`
  resolved `syn v3.0.3` cleanly against this workspace's pinned toolchain — pure Rust, no
  C-dependency, extending the same discipline already applied twice (`rustpython-parser` over
  `tree-sitter-python`, `pg_query`'s RFC 0031 rejection).
- **`syn::visit::Visit`** walks each recognized function/method body looking for `Expr::Call`/
  `Expr::MethodCall`. Symbol maps (`functions: HashMap<String, KirId>`, `methods_by_name`,
  `methods_exact: HashMap<(String, String), KirId>`) are built in a first pass over every item in
  the file *before* any body is walked, so a call to a symbol defined later in the file still
  resolves — order-independent, unlike a naive single-pass walk.
- **`self.method()` doesn't need receiver-type inference** — the visitor doesn't look at the
  receiver expression at all, just the method name; it resolves only when that name is unambiguous
  file-wide. This is a real, disclosed approximation (documented in a unit test:
  `ambiguous_method_name_across_two_types_is_not_recorded`), not full type-directed resolution.

### Real-data testing — found a real bug in shared identity-resolution code

Ran the full pipeline (`ekos build && ekos recover && ekos resolve && ekos compile && ekos commit`)
against **this EKOS workspace itself** — no external repo needed, unlike Python's
`azure-databricks-project` dependency. `recover` alone: **118 Rust files analyzed, 1270 symbols,
715 `Calls` edges** — real, non-trivial numbers on the first run.

Inspecting the committed ledger via `ekos ekl` surfaced something wrong: several real
`<X>AnalyzerPass` structs (`ConfluenceAnalyzerPass`, `PentahoAnalyzerPass`, `GitAnalyzerPass`, ...)
were entirely missing from `FIND Object WHERE kind = 'RustSymbol' AND name CONTAINS
'AnalyzerPass'` — only 4 of ~11 expected. Root cause, found by writing a scratch #[test] to run
`syn::parse_file` directly against the real source files (ruling out a parse failure — all nine
files parsed fine) and then reading `crates/identity/src/lib.rs`: `DefaultResolver`'s merge-proposal
loop blocks candidates by `(kind_str, first-3-normalized-chars)` and scores them by 70% Jaro-Winkler
name similarity + 30% structural similarity. `structural_score` falls back to a flat 1.0 for any
pair of objects sharing a kind but lacking a `columns` property to compare — which is every
`RustSymbol`. Structs sharing a long common suffix (`ConfluenceAnalyzerPass` vs
`PentahoAnalyzerPass` vs `GitAnalyzerPass`, all genuinely distinct types in different files) scored
above the 0.85 merge threshold and got silently collapsed into one canonical object, with the
others dropped from the ledger — first `compile` run: 460 objects / 1902 relationships,
`resolve` proposing 190 merges over 21,292 compared pairs.

This is the exact same failure shape the project had already hit and fixed twice before —
`Custom("Section")` (RFC 0024, devlog 27: 8,624 raw objects collapsed to 120) and
`Custom("TransformNode")` (RFC 0027/0028, a live demo smoke-test) — both already carry a blanket
kind-exclusion from `DefaultResolver`'s blocking loop, with the same rationale each time: objects
already deterministically identified by (file, index, or qualified name) can never legitimately be
the same real-world entity the way a cross-system `Table` match can, so name similarity should
never be allowed to merge them.

**Fix**: extended that exclusion (`crates/identity/src/lib.rs`) to cover `RustSymbol`/`RustModule`.
Also included `PythonSymbol`/`PythonModule` (RFC 0038/0040) in the same fix — they carry the
identical `KirObject` shape and are exposed to the identical failure mode, just never noticed
before because RFC 0040's real-data testing only checked `TransformNode` node counts (which were
already excluded), not raw `PythonSymbol` object survival. Re-running the full pipeline after the
fix: `resolve`'s merge proposals dropped from 190 to 2 (pairs compared 21,292 → 146 — most of the
prior comparison volume was exactly this spurious same-kind blocking), and `compile` produced
**1812 objects / 3749 relationships** (up from 460/1902) — every previously-missing struct is back,
confirmed via `ekl`.

Spot-checked one real `Calls` edge end-to-end: `ekl "FIND Relationship WHERE kind = 'Calls' FROM
'PentahoAnalyzerPass::run' RETURN to"` resolved to `parse_kettle_xml` — confirmed against the real
call site at `pentaho_analyzer.rs:133`.

One narrow, expected residual: `resolve` still reports one real cross-kind conflict
(`'observeerror' appears as multiple kinds: RustSymbol, RustModule`) — a `RustModule` import target
and a `RustSymbol` enum definition normalizing to the same string. This is a *different* code path
(`resolve`'s cross-kind conflict detector, which compares all objects regardless of kind and flags
same-normalized-name-different-kind pairs) correctly doing its job — flagging an ambiguous case for
manual review rather than silently guessing, exactly as RFC 0007 designed it to. Not a bug.

---

## Knowledge Captured

- **`DefaultResolver`'s same-kind structural-fallback-of-1.0 behavior (RFC 0007) is a recurring
  failure mode whenever a new analyzer's objects are named by convention rather than by genuinely
  distinguishing content.** This is now the third time this exact shape has been hit (Section →
  TransformNode → RustSymbol/RustModule/PythonSymbol/PythonModule). Any future analyzer whose KIR
  objects are named by a repeated pattern (a common file-path prefix, a common type suffix, a
  common template) should be checked against this failure mode *before* shipping, not discovered
  live — the fix is always the same one-line blanket kind-exclusion in
  `crates/identity/src/lib.rs`'s blocking loop.
- **Real-data testing against this repo's own Cargo workspace is a fully sufficient test corpus for
  future source-analyzer work** — no external dependency needed, unlike Python's Databricks
  fixture. Given this project already treats devlogs/RFCs as long-term memory, `ekos`'s own
  codebase is guaranteed to keep growing as a richer real-world test target over time.
- **A silent object loss during identity resolution is easy to miss if real-data verification only
  checks pass-level summary counts** (`recover`'s printed "files/symbols/edges" line) rather than
  the *post-compile* ledger contents. `recover`'s own stats were correct and unchanged before and
  after the identity-resolver fix (118/1270/715) — the loss only became visible by querying the
  ledger with `ekl` after the full `compile`/`commit` pipeline ran. Worth querying the ledger
  directly, not just trusting `recover`'s own pass-level counters, for any future analyzer's
  real-data verification.
- **`self.method()` call resolution doesn't need receiver-type inference to be useful** — resolving
  purely on method-name uniqueness within a file is a cheap, honest approximation that still
  produced 715 real edges on this repo, with the ambiguous case explicitly not guessed at.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0041-rust-source-analyzer.md` | new — full RFC, all acceptance criteria verified against real data |
| `ekos/plugins/rust/Cargo.toml`, `src/lib.rs` | new — `RustObserver`, 3 tests |
| `ekos/crates/recovery/src/rust_analyzer.rs` | new — `RustAnalyzerPass`, 9 tests |
| `ekos/crates/recovery/src/lib.rs` | `+pub mod rust_analyzer;` + re-export |
| `ekos/crates/recovery/Cargo.toml`, `ekos/Cargo.toml` | `+syn`, `+plugins/rust` workspace member |
| `ekos/crates/cli/src/commands/build.rs` | `+RustObserver` in the observer list |
| `ekos/crates/cli/src/commands/recover.rs` | `+collect_rust_artifact_ids` + pass registration + summary output |
| `ekos/crates/cli/Cargo.toml` | `+ekos-plugin-rust` path dependency |
| `ekos/crates/identity/src/lib.rs` | bug fix — exclude `RustSymbol`/`RustModule`/`PythonSymbol`/`PythonModule` from `DefaultResolver`'s merge blocking (same class of fix as Section/TransformNode), + 1 regression test |

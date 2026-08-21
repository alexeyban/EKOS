# RFC 0040 — Phase 2: Python/PySpark Analyzer (RFC 0038 Phase 2)

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-08

---

## Motivation

RFC 0038's roadmap scoped Phase 2 as "real AST parsing extracts imports/functions/classes, and
recognizes PySpark DataFrame call chains (`.read.table(...)`, `.join(...)`, `.groupBy(...).agg(...)`,
`.write.saveAsTable(...)`) lowered into the same Transformation IR Pentaho/SQL use." Before
designing the exact shape, this pass investigated three things directly: how source discovery
should plug into the existing architecture, what real PySpark code in a real repo actually looks
like, and which Python parser crate is viable in this workspace. All three materially shaped the
design.

### 1. Source discovery: a new `Observer` plugin, not a `recover.rs`-local walk

Confirmed by reading `plugins/pentaho/src/lib.rs` and `crates/cli/src/commands/recover.rs` in
full: this codebase has two live precedents for how an analyzer gets its raw source text —
(a) a dedicated `WalkDir` loop written directly in `recover.rs` (used today for `.sql` and for the
generic pattern-matching `DependencyAnalyzerPass`, which already technically walks `.py` files but
only does substring dependency-name matching, not real parsing), or (b) a dedicated `Observer`
plugin crate (used for Pentaho/git/crypto/GitHub/Confluence/localdocs) that re-walks the tree
itself, filters to its own extensions, and stores raw source text verbatim in the artifact's
`data` JSON under a format-specific key (Pentaho uses `"xml"`); `recover.rs` then only filters
`artifact_store.list()` by `connector_name` and hands the resulting `ArtifactId`s to the pass.
Python gets pattern (b) — the same shape as Pentaho — because it needs real per-file structural
parsing. `plugins/file`'s existing generic walk is not reused: its artifacts never carry more than
a 600-char excerpt, never full source.

### 2. Real PySpark code shape — investigated against a real repo, not assumed

Read real files in `/home/legion/PycharmProjects/azure-databricks-project` (a real Databricks
Asset Bundle project). Finding that reshaped the design: **real inline DataFrame method chains
are concentrated in a shared library (`src/dp/*.py`), not in the notebooks themselves.** Every
file under `notebooks/` is a flat, numbered top-level script that reads parameters via
`dbutils.widgets.get(...)`, then delegates actual transformation work to imported `dp.*`
functions. Real chains like
```python
deleted = active_bronze.join(pk_df, on=primary_keys, how="left_anti")
```
and
```python
df.withColumn("_inserted_at", ts).withColumn("_updated_at", ts).withColumn(...)
```
live in `src/dp/transforms/bronze.py`, `src/dp/quality/reconciliation.py`,
`src/dp/semantic/graph.py` (which also has real `.groupBy(...).agg(...)`). Writes funnel through
a shared `write_delta()` helper. Most `notebooks/semantic/*.py` files read/write via raw
`spark.sql(f"""...""")` blocks with Python f-string `{var}` interpolation — not valid standalone
SQL text.

This means recognizing inline method chains *within a single function's body* finds real,
meaningful business logic in the shared library files, but little to nothing directly inside most
notebook files. **Tracing a pipeline end-to-end across function/file call boundaries
(interprocedural dataflow) is explicitly out of scope for this phase** — the same "accept
incomplete coverage, never guess" scoping applied consistently elsewhere in this project.

`# Databricks notebook source` / `# COMMAND ----------` / `# MAGIC %md` cell markers are confirmed
harmless to a real Python parser — ordinary `#` comments. `dbutils`/`spark`/`display` are used as
bare, unimported runtime-injected globals — a real parser must not treat their absence as an
error, which a pure syntactic AST walk (no semantic analysis) naturally handles.

### 3. Parser choice: `rustpython-parser`, not `tree-sitter-python`

Verified directly: `cargo add --dry-run` shows `rustpython-parser`/`rustpython-ast` 0.4.0 resolve
cleanly against this workspace's pinned toolchain. `ruff_python_parser`/`ruff_python_ast` require
rustc 1.95; this workspace's toolchain is 1.93 — incompatible without forcing
`--ignore-rust-version` (rejected). `tree-sitter-python` resolves, but `tree-sitter`'s Rust
bindings wrap a C library — the same "new C-dependency class" RFC 0031 already explicitly rejected
`pg_query` over. `rustpython-parser` is pure Rust and was verified directly against a synthetic
snippet containing the exact real notebook conventions found in step 2 — parses correctly.

## Scope

A new `ekos build`/`ekos recover` pipeline stage: `PythonObserver` (new `plugins/python`) discovers
real `.py` files; `PythonAnalyzerPass` (new `crates/recovery/src/python_analyzer.rs`) parses them
with `rustpython-parser` and lowers recognized constructs into the Transformation IR (RFC 0027)
and plain KIR objects/relationships.

## Non-goals

- `.ipynb` notebooks — RFC 0038 Phase 3, not this phase.
- Interprocedural chain tracing across function/file call boundaries.
- Parsing `spark.sql(...)` argument text as SQL.
- Full `.agg(...)` coverage beyond the real `F.<func>(<col>).alias(<name>)` shape found in the
  test repo.

_All four are tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "Analyzers"
(interprocedural chain tracing is the same underlying gap as RFC 0041's Rust equivalent — one
consolidated cross-language item)._

## What already exists and is reused

- `pentaho_analyzer.rs`'s shape (parse artifact → `TransformGraph` → `lower_to_kir` →
  `merge_graphs`) — copied structurally.
- `ekos_semantic::transform_ir` (RFC 0027) — the exact `TransformNode`/`TransformGraph`/
  `lower_to_kir` Pentaho/SQL already use, so `ekos_transformation_diff`/`ekos_transformation_explain`
  (RFC 0028) and `ekos dbt generate`/`ekos docs generate` (RFC 0035-0037) all work on
  Python-sourced graphs for free.
- `build.rs`'s `ObjectKind::File` KirId scheme (`Uuid::new_v5(&Uuid::NAMESPACE_URL, rel_path)`) —
  reused so import/symbol relationships attach to the real existing file object.

## Design

**`PythonObserver`** (`plugins/python`) — mirrors `PentahoObserver` exactly: own `WalkDir` walk,
filters to `.py`, stores raw text verbatim under `data["source"]`, `connector_name: "python"`.

**`PythonAnalyzerPass`** (`crates/recovery/src/python_analyzer.rs`) — mirrors
`PentahoAnalyzerPass`'s shape. Per file:
- `Stmt::Import`/`Stmt::ImportFrom` → `KirObject(Custom("PythonModule"))` + `DependsOn` from the
  file's KirId.
- `Stmt::FunctionDef`/`Stmt::ClassDef` → `KirObject(Custom("PythonSymbol"))` + `Contains` from the
  file.
- DataFrame method chains: a chain like `df.join(x).withColumn(y).filter(z)` parses AST-inside-out
  (nested `Expr::Call(func=Expr::Attribute(value=<inner>))`); `linearize_chain` recurses into the
  receiver first, producing a flat, source-ordered list of `.method(...)` calls, then
  `calls_to_nodes` maps recognized methods to `TransformNode`s: `spark.table(...)`/`.load(...)` →
  `Source`; `.join(...)` → `Join` (keys from `on=`, kind from `how=`, real recognized `how=`
  values only — unrecognized ones like `left_anti`/`left_semi` default to `Inner`, a documented
  approximation matching `pentaho_analyzer.rs`'s own `DatabaseJoin` precedent); `.groupBy(...).agg(...)`
  → `Aggregate` (only when paired — bare `.groupBy(...)` alone produces no node);
  `.filter(...)`/`.where(...)` → `Filter` (condition = exact source-text slice of the argument,
  via `rustpython-ast`'s `Ranged`/`range()`, mirroring `pentaho_analyzer.rs`'s `xml_slice`
  byte-slicing exactly); `.withColumn(...)` → `Calculate`; `.write...saveAsTable(...)`/`.save(...)`
  → `Sink`. Intermediate calls (`.format(...)`/`.mode(...)`/`.option(...)`) pass through without
  producing a node. `spark.sql(...)` always becomes `Unmapped` with the raw argument text — never
  parsed as SQL.
- Each recognized chain (found at module level or one level into a function body) becomes its own
  `TransformGraph`, nodes linked by sequential edges, `TransformOrigin.source_path =
  "{path}#{index}"` mirroring SQL's per-statement origin naming.

## Alternatives Considered

- **`tree-sitter-python`** — rejected: C-dependency class RFC 0031 already rejected for the same
  reason (`pg_query`).
- **Lightweight regex/heuristic extraction** — already rejected when RFC 0038 was written.
- **Interprocedural chain tracing now** — rejected for this phase; real engineering cost with no
  proven need yet.
- **Best-effort de-interpolation of `spark.sql(f"...")` strings** — rejected; risks a
  syntactically-valid but semantically-wrong SQL statement, worse than an honest `Unmapped`.

## Open Questions

- [ ] `.ipynb` notebook support — Phase 3, not this phase.
- [ ] Interprocedural chain tracing — deferred, see Alternatives Considered.
- [ ] Whether `dbutils.widgets.get(...)` calls should be captured as evidence/properties even
      without a full parameter/variable IR (deferred to RFC 0038 Phase 5/6) — not attempted here,
      to avoid a half-built parameter concept ahead of Phase 5's real design.

## Testing

13 unit tests covering: import → `DependsOn`, function/class def → symbol object, each
chain-method → correct `TransformNode` variant (built from real snippets captured against
`azure-databricks-project`), a real multi-step chain (`bronze.py`'s `add_metadata_columns` shape)
→ a real multi-node graph with correct edges, `.groupBy(...).agg(...)` pairing (`graph.py`'s real
shape), the `spark.sql(f"...")` → honest `Unmapped` case, a plain-statement-with-no-chain →
no-graph case, and a Databricks-notebook-comment-marker robustness test.

## Acceptance Criteria

- [x] `PythonObserver` discovers and stores real `.py` file content verbatim — 3 unit tests pass.
- [x] `PythonAnalyzerPass` recognizes all six chain-method mappings and produces correct
      `TransformNode`s, verified by unit test against real captured snippets — 13 unit tests pass.
- [x] A real multi-step chain from `azure-databricks-project` recovers as a real multi-node graph
      when run live. Verified: a real `ekos build && ekos recover` run against the real repo
      (via a scratch workspace with `[observe] paths` pointing at the repo's absolute path, so no
      `.ekos/` state was ever written inside the real project) produced
      `Python files analysed: 83`, `Transformation IR nodes (Python): 57 total, 60% mapped
      (non-Unmapped)`. Inspected via `ekos docs generate --layout curated`'s `SequenceDiagrams.md`:
      real 3-node `Calculate→Calculate→Calculate` chains from `bronze.py`'s
      `add_metadata_columns`/`detect_deleted_rows`, a real `Join` node from `graph.py`'s
      `edges.join(vertices, ...)`, real `Calculate` chains from `reconciliation.py` — matching
      this RFC's Context section prediction exactly (real chains in library files, mostly
      single-node/no-chain results in notebooks themselves).
- [x] `spark.sql(f"...")` never gets guessed at — always `Unmapped` with the raw text, verified by
      test and confirmed absent from any `Source`/`Filter`/etc. misclassification in the real run.
- [x] `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`
      all pass.

## Files Changed

| File | Change |
|---|---|
| `ekos/plugins/python/Cargo.toml`, `src/lib.rs` | new — `PythonObserver` |
| `ekos/crates/recovery/src/python_analyzer.rs` | new — `PythonAnalyzerPass`, 13 tests |
| `ekos/crates/recovery/src/lib.rs` | `+pub mod python_analyzer;` + re-export |
| `ekos/crates/recovery/Cargo.toml`, `ekos/Cargo.toml` | `+rustpython-parser`, `+rustpython-ast`, `+plugins/python` member |
| `ekos/crates/cli/src/commands/build.rs` | `+PythonObserver` in the observer list |
| `ekos/crates/cli/src/commands/recover.rs` | `+collect_python_artifact_ids` + pass registration |
| `ekos/crates/cli/Cargo.toml` | `+ekos-plugin-python` path dependency |

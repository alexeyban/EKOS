# Devlog 40 — RFC 0038/0040 Phase 2: Python/PySpark analyzer, verified against a real Databricks repo

**Date:** 2026-08-08
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Implemented RFC 0038's Phase 2: a new Python/PySpark analyzer that recognizes real DataFrame
transformation chains and lowers them into the same Transformation IR (RFC 0027) Pentaho and SQL
already use. New `plugins/python` (`PythonObserver`, mirrors `plugins/pentaho`'s shape exactly)
and `crates/recovery/src/python_analyzer.rs` (`PythonAnalyzerPass`, real AST parsing via
`rustpython-parser`, not a regex/heuristic scan). Before writing any code, this session
investigated three things directly against real sources rather than assuming them: how source
discovery should plug into the existing architecture (a dedicated `Observer` plugin, matching
Pentaho's precedent), what real PySpark code in a real repo actually looks like (business logic
concentrates in a shared library, not the notebooks themselves — a finding that materially
narrowed the honest scope), and which Python parser crate is viable in this workspace
(`rustpython-parser`, pure Rust — `tree-sitter-python` was rejected for pulling in the same
C-dependency class RFC 0031 already rejected `pg_query` over). Verified end-to-end against a real
local Databricks Asset Bundle repo (`azure-databricks-project`): 83 real Python files analyzed, 57
real Transformation IR nodes recovered at 60% mapped coverage, including a real multi-step
`Calculate` chain and a real `Join` node matching the exact code this RFC's own design section
predicted would recover.

---

## Problem / motivation

RFC 0038 scoped Phase 2 at a high level: real AST parsing, PySpark chains lowered into the
Transformation IR. Three concrete design questions needed real answers before writing code, not
assumptions:

1. **Where does the raw Python source come from?** Read `plugins/pentaho/src/lib.rs` and
   `crates/cli/src/commands/recover.rs` in full. Confirmed two live precedents exist in this
   codebase for how an analyzer gets its input — a `recover.rs`-local `WalkDir` loop (used for
   `.sql`), or a dedicated `Observer` plugin that re-walks the tree and stores raw content
   verbatim in the artifact JSON (used for Pentaho and every other structural-parsing analyzer).
   Python needed the second pattern, since `plugins/file`'s existing generic walk only ever
   carries a 600-char excerpt, never full source.
2. **What does real PySpark code actually look like?** Read real files in
   `/home/legion/PycharmProjects/azure-databricks-project`. Found that real inline `.join()`/
   `.withColumn()`/`.groupBy().agg()` chains live almost entirely in a shared library
   (`src/dp/*.py`), not in the notebook files themselves — every notebook is a flat,
   widget-parameterized script that delegates to library functions. This directly shaped the
   decision to recognize chains *within one statement's expression*, honestly not attempting to
   trace a pipeline across function/file call boundaries.
3. **Which Python parser?** Verified via `cargo add --dry-run`: `rustpython-parser`/
   `rustpython-ast` resolve cleanly against this workspace's pinned toolchain;
   `ruff_python_parser` needs a newer rustc than this workspace has; `tree-sitter-python` would
   pull in a C-dependency class this project already rejected once (RFC 0031's `pg_query`
   rejection, same reasoning applied here).

---

## What was built

| Component | Location |
|---|---|
| Observer plugin | `ekos/plugins/python/` (new) — `PythonObserver`, mirrors `plugins/pentaho` |
| Analyzer pass | `ekos/crates/recovery/src/python_analyzer.rs` (new) — `PythonAnalyzerPass` |
| RFC | `ekos/docs/rfcs/0040-python-pyspark-analyzer.md` (new) |

`PythonAnalyzerPass` recognizes DataFrame method chains by unwrapping the AST-inside-out nested
`Call(func=Attribute(value=<inner>))` structure a fluent chain parses into (`linearize_chain`)
into a flat, source-ordered list of `.method(...)` calls, then maps recognized method names to
`TransformNode` variants: `spark.table(...)`/`.load(...)` → `Source`; `.join(...)` → `Join` (keys
from `on=`, kind from `how=`); `.groupBy(...).agg(...)` → `Aggregate` (only when the two calls are
adjacent — bare `.groupBy(...)` alone produces nothing); `.filter(...)`/`.where(...)` → `Filter`;
`.withColumn(...)` → `Calculate`; `.write...saveAsTable(...)`/`.save(...)` → `Sink`.
`spark.sql(...)` always becomes `Unmapped` with the raw argument text — never parsed as SQL, since
the argument is very often an f-string with `{var}` interpolation that isn't valid SQL syntax.
Imports become `KirObject(Custom("PythonModule"))` + `DependsOn`; function/class defs become
`KirObject(Custom("PythonSymbol"))` + `Contains` — both attach to the file's *existing* `KirId`
(same `Uuid::new_v5` scheme `build.rs` already uses for `ObjectKind::File` objects), not a
duplicate object.

## Implementation details worth remembering

- **`Filter`/`Calculate` conditions are exact source-text slices, not reconstructed from the
  AST.** `rustpython-ast`'s `Ranged`/`range()` gives real byte offsets into the original source,
  the same technique `pentaho_analyzer.rs`'s `xml_slice` already uses for raw XML — verified
  directly with a standalone test before relying on it (`F.col("x") == 1` sliced correctly from a
  real snippet).
- **`.groupBy(...).agg(...)` pairing requires lookahead in the linearized call list**, not
  independent per-call handling — `groupBy` alone contributes no node; it only becomes an
  `Aggregate` node when immediately followed by `.agg(...)`, consuming both calls at once. The
  `.agg(...)` argument recognition (`F.<func>(<col>).alias(<name>)`) was built from the real shape
  found in `src/dp/semantic/graph.py`, not a generic assumption.
- **`how=` values outside this IR's fixed `JoinKind` vocabulary (`left_anti`, `left_semi`, etc.,
  real PySpark join types) default to `Inner`** — a documented approximation, explicitly justified
  by precedent (`pentaho_analyzer.rs`'s own `DatabaseJoin` approximation), not silently wrong.
- **Python-module target objects are deduped within one pass run** (`HashSet<KirId>` keyed by the
  deterministic module KirId) since many files can import the same module — mirrors
  `dependency_analyzer.rs`'s "create/reuse" discipline for its Technology objects.

## Real-data testing

Ran the real pipeline (`ekos init && ekos build && ekos recover`) against
`azure-databricks-project` from a *separate* scratch workspace whose `ekos.toml` pointed
`[observe] paths` at the real repo's absolute path — deliberately avoiding writing any `.ekos/`
state inside the user's actual project directory. Real results: **83 Python files analyzed, 57
Transformation IR nodes at 60% mapped coverage** (up from the terminal's default `0%` for a file
type recovering nothing, confirming real structural recognition, not a no-op). Inspected the
actual recovered graphs via `ekos docs generate --layout curated`'s `SequenceDiagrams.md` (RFC
0037's existing tooling — zero new code needed to visualize Python-sourced pipelines, since they
share the exact same `TransformNode`/`FeedsInto` shape Pentaho/SQL already produce):

- `src/dp/transforms/bronze.py#0` — a real 4-node `Calculate→Calculate→Calculate` chain, matching
  `add_metadata_columns`'s real `.withColumn().withColumn().withColumn()` shape.
- `src/dp/semantic/graph.py#3` — a real 2-node `Join` chain, matching
  `edges.join(vertices, edges["src"] == vertices["id"])`.
- `src/dp/quality/reconciliation.py#0` — a real 3-node `Calculate` chain.
- Most `notebooks/*.py` origins recovered as single-node or no-graph results — the correct,
  predicted outcome (notebooks delegate to library functions this phase doesn't trace into), not a
  bug.

No real bugs found this time (unlike RFC 0035/0036/0037/0039's real-data testing) — the design's
own Context section already predicted the exact shape of what would and wouldn't recover, because
it was written *after* reading the real target repo, not before.

---

## Knowledge Captured

- **Investigating the real target data before designing a recognizer, rather than after, avoids
  the "real bug found by real testing" pattern this project has hit four times previously this
  session.** This phase's design doc explicitly predicted its own real-data test's outcome
  (chains in library files, not notebooks) and that prediction held exactly — the inverse of RFC
  0035-0039's pattern of designing first, then discovering the design's blind spots via real
  testing afterward. Worth deliberately doing "read the real target first" for future connector
  phases (Databricks/ADF, RFC 0038 Phase 4/5) too.
- **Real, well-organized PySpark codebases decompose business logic into small reusable library
  functions, not one big fluent chain per notebook.** Any future work wanting to recover a
  *complete* pipeline (not just its individual recognized fragments) needs real interprocedural
  call-graph tracing — a genuinely separate, larger problem than single-function-body pattern
  recognition, deliberately not attempted in this phase.
- **`rustpython-ast`'s `Ranged` trait gives exact source-text byte ranges for any AST node** — the
  same technique this project already uses for XML (`pentaho_analyzer.rs`'s `xml_slice`) now has
  a Python equivalent. Worth reusing for any future Python-AST-adjacent work.
- **A new source-format connector can be fully verified end-to-end using zero new visualization
  tooling** — RFC 0037's `ekos docs generate --layout curated` already renders any
  `Custom("TransformNode")` graph, regardless of which analyzer produced it, since the whole
  Transformation IR is format-agnostic by design (RFC 0027's original intent, now paying off a
  third time: SQL, Pentaho, now Python all render through the same pipeline).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0040-python-pyspark-analyzer.md` | new — full RFC, all acceptance criteria verified against real data |
| `ekos/plugins/python/Cargo.toml`, `src/lib.rs` | new — `PythonObserver`, 3 tests |
| `ekos/crates/recovery/src/python_analyzer.rs` | new — `PythonAnalyzerPass`, 13 tests |
| `ekos/crates/recovery/src/lib.rs` | `+pub mod python_analyzer;` + re-export |
| `ekos/crates/recovery/Cargo.toml`, `ekos/Cargo.toml` | `+rustpython-parser`, `+rustpython-ast`, `+plugins/python` member |
| `ekos/crates/cli/src/commands/build.rs` | `+PythonObserver` in the observer list |
| `ekos/crates/cli/src/commands/recover.rs` | `+collect_python_artifact_ids` + pass registration + summary output |
| `ekos/crates/cli/Cargo.toml` | `+ekos-plugin-python` path dependency |

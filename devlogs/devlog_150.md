# Devlog 150 — RFC 0127 Web Console, Phase 0 contracts (graph export, status --json, MCP tool)

**Date:** 2026-09-02
**PRs:** commits on branch `rfc/0127-web-console` → `main`
**Branch:** `rfc/0127-web-console` → `main` (fast-forward)

---

## Summary

RFC 0127 (Web Console) is a new umbrella RFC: EKOS's differentiating claim — *cross-system impact
analysis with a traceable evidence chain* — has no visual form today; the only way to see an
impact trace is to read `ekos_impact` JSON. The console itself (Python MCP client + FastAPI +
Vite/React) is deferred; this session landed only the **Rust, CI-gated Phase 0 contracts** it
needs:

- **R1** — `ekos graph export`: the first bulk graph-extraction path in EKOS. Every existing read
  is per-object (`ekos_neighborhood`, `ekos_impact`) or `LIMIT 50`-capped (`ekos_search`).
- **R2** — `--json` on `ekos status` / `ekos ledger status`: a machine-readable status feed.
- **R3** — `ekos_graph_export` MCP tool: a thin wrapper over R1's function.

The RFC was filed as **0127** (not 0128 as drafted): RFC 0118 and TODO.md had reserved *0127* in
prose for a future computed-staleness/drift RFC; that RFC now takes a later number and its
cross-references are re-pointed when it's authored.

---

## PR — RFC 0127 (umbrella)

### What was built

`ekos/docs/rfcs/0127-web-console.md` (668 lines, from `~/Downloads/0128-web-console.md`).
`Status: Accepted (2026-09-02) — Phase 0 R1/R2/R3`. Header renumbered 0128→0127; a "Numbering
note" paragraph added; §4.8 corrected (`all_objects_at`/`all_relationships_at` **do** exist on the
trait, RFC 0096 — `--as-of` graph export is deferred for scope, not a missing primitive).

---

## PR — `ekos graph export` (R1)

### What was built

| Component | Role |
|---|---|
| `ekos_runtime::graph_export` | `export_graph(&dyn KnowledgeStore, &GraphExportOptions) -> Result<GraphExport>` — one pure, read-only, deterministic function |
| `GraphExportOptions` | `level` (object/aggregate), kind + rel-kind include-lists, `exclude_rel_kinds`, `group_by` (kind / path-prefix), `max_nodes`/`max_edges`/`min_degree`, `include_properties` |
| `GraphExport` | short-key wire format: `nodes` (`id`/`n`/`k`/`d`/`p`), `edges` (`s`/`t`/`k`/`w` — indices into the node array + `kind_index`), `counts`, `truncated`, `filters` |
| `crates/cli/src/commands/graph.rs` | `ekos graph export` — arg parsing, read-only store open, `--format json|ndjson`, `--output` |

### Implementation details worth remembering

- **Truncation is reported, never silent.** Over `--max-nodes`, nodes are kept by degree
  descending (ties by `KirId`), and `truncated.{nodes,node_limit,selection}` says so. Devlog 14's
  6-million-`CoupledWith`-edge estate run is the cautionary tale — an export that silently returns
  its first 20 000 edges would be actively misleading.
- **Degree is post-filter.** Excluding a relationship kind changes node degree; the schema states
  this rather than leaving a reader to assume it (the console sizes nodes by `d`).
- **`min_degree` is a single pass**, not iterated to a fixpoint — documented, not a bug.
- **Aggregate level** collapses to super-nodes (`id_space: "synthetic"`, ids like `kind:File` /
  `path:crates/ledger`), never truncated; `Σ node.count == objects_after_filter` and
  `Σ edge.w == relationships_after_filter` (unit-tested both backends).

### Real-data check (RFC §4.9)

Against this repo's own `.ekos/` (fact-segment backend, 5533 objects / 8364 relationships):

| Run | returned | payload | wall-clock |
|---|---|---|---|
| `--level object` (default caps) | 5000 nodes (truncated), ~4000 edges | ~2 MB json | ~3 s |
| `--level aggregate --group-by kind` | 16 nodes, 15 edges | ~2 KB | ~3 s |

Determinism confirmed: two runs, `generated_at` stripped, byte-identical.

---

## PR — `ekos status --json` + `ekos_graph_export` (R2 + R3)

### What was built

| Component | Role |
|---|---|
| `ledger::build_status_json` | testable core of `status --json` — one `StatusJson` per backend, no stdout |
| `KnowledgeStore::evidence_count` | new trait method (no default). Real on `Ledger` (`COUNT … entry_type='evidence'`), `FactLedger` (`entities_with_attr("fragment")`), `PartitionedLedger` (sum over evidence partitions); `DistributedLedger` returns `Err` + a deferred-RPC comment |
| `Ledger::format_tag` | `"sqlite-v1"` / `"sqlite-v2"` — the private `Format` enum stays private |
| `ekos_graph_export` MCP tool | wraps `export_graph`; `query_log::classify_tool` → `Expensive` → cacheable |

### Implementation details worth remembering

- **`--json` is a pure alternate presentation.** It shares the exact opener the text path uses, so
  the two can never disagree about the backend, and the text body is edited nowhere — RFC 0116's
  `ekos status` == `ekos ledger status` byte-identity is preserved. A unit test asserts a `--json`
  run alongside has zero side effects on the text-path computation.
- **`integrity` is always `"unchecked"` in R2.** A real integrity pass (`verify_sealed_report` /
  `PRAGMA integrity_check`) is seconds-to-minutes; `status` must stay instant. `--verify` is a
  documented future add.
- **`last_write` points at `facts/segments/`, not the whole store root.** A fact/partitioned
  store rewrites its tantivy `search/` meta on every *read-only* open too — walking the whole root
  would make `last_write` mean "last opened". Narrowing to `segments/` keeps it a real
  last-*write*. Verified live: repo workspace reports `2026-08-26…` (a real past `commit`), not
  "now".
- **One deliberate divergence from RFC 0116:** `--json` reports `relationships` on every backend
  (the console dashboard needs it); the text path is unchanged.

### Decisions

- **Skipped the planned `store_root`/`store_fingerprint` lift from `mcp.rs` into `store.rs`.** It
  was a behaviour-neutral refactor to share a helper; `ledger.rs` gets its own small
  `newest_mtime` instead. Less churn in `mcp.rs` and its test suite, same result. If a third
  caller ever needs it, lift it then.
- **`DistributedLedger::evidence_count` returns `Err`, CLI does `.ok()` → JSON `null`.** A
  fan-out `evidence_count` RPC in the `QueryWorker` protocol (RFC 0113) is real deferred work, not
  worth blocking R2 on.

---

## Knowledge Captured

- **`RelationshipKind::Custom("CoupledWith")` round-trips to the built-in `RelationshipKind::CoupledWith`.**
  serde's `#[serde(untagged)] Custom(String)` is tried *last*, so any `Custom("<builtin-name>")`
  deserializes back to the real variant after a store round-trip. A test fixture that appended
  `Custom("CoupledWith")` and later filtered on `Custom("CoupledWith")` (an in-memory value that
  never round-tripped) silently matched nothing. Use the built-in variant in fixtures; reserve
  `Custom(...)` for genuinely-custom names.
- **`cargo clippy --workspace` (CI) does not lint test code** — no `--all-targets`. A
  `field_reassign_with_default` in a `#[cfg(test)]` block in `recover.rs` fails
  `cargo clippy --all-targets` locally but is invisible to CI. Match CI's exact flags before
  concluding clippy is clean.
- **clippy 1.98** flags `for ((a, b), _) in &map` (`for_kv_map` → use `.keys()`) and
  `.sort_by(|a,b| a.k.cmp(&b.k))` (`unnecessary_sort_by` → `.sort_by_key(|a| a.k)`) — both fired
  on new `graph_export.rs` code and pass silently on older toolchains.
- **`KirId` derives no `Ord`.** Every deterministic tie-break in `graph_export.rs` sorts on the
  inner `uuid::Uuid` (`id.0`), which does.
- **Tracing writes to stdout for every `ekos` subcommand** (`init_logging`), so
  `ekos graph export` / `ekos status --json` emit a `tantivy … file_watcher` INFO line above the
  JSON. Pre-existing, shared by `ekos ekl --json`; consumers filter it or use `--output`. Not
  fixed here — flagged for a future "quiet JSON commands" pass.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0127-web-console.md` | **new** — the umbrella RFC (renumbered 0128→0127) |
| `ekos/crates/runtime/src/graph_export.rs` | **new** — `export_graph` + wire types + 13 unit tests (both backends) |
| `ekos/crates/runtime/src/lib.rs` | `pub mod graph_export;` + re-exports |
| `ekos/crates/cli/src/commands/graph.rs` | **new** — `ekos graph export` command + parsers + 6 tests |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod graph;` |
| `ekos/crates/cli/src/bin/ekos.rs` | `Commands::Graph` + `GraphCommands::Export`; `--json` on both `Status` variants |
| `ekos/crates/cli/src/commands/ledger.rs` | `build_status_json` + `StatusJson`/`StorageJson` + `newest_mtime`; `status()` takes `json` |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_graph_export` tool def + `call_tool` arm + tools/list assertion + test |
| `ekos/crates/cli/src/commands/query_log.rs` | `classify_tool`: `ekos_graph_export` → `Expensive` |
| `ekos/crates/ledger/src/lib.rs` | trait `evidence_count`; `Ledger::{evidence_count, format_tag}`; `delegate_store!` forwarder |
| `ekos/crates/ledger/src/fact_ledger.rs` | `FactLedger::evidence_count` |
| `ekos/crates/ledger/src/partitioned/{mod.rs,knowledge_store.rs}` | `PartitionedLedger::evidence_count` |
| `ekos/crates/distributed/src/gateway.rs` | `DistributedLedger::evidence_count` → `Err` + deferred-RPC comment |
| `TODO.md` | RFC 0127 entry under the RFC 0118 series; 0127-reservation note re-pointed |
| `README.md` | `ekos graph export` + `status --json` mentions |
| `docs/generated/ekos-self-documentation.html` | graph export + `status --json` + `ekos_graph_export` sections |

# Devlog 21 — RFC 0019: dependency-fact extraction + symbol harvesting

**Date:** 2026-07-24
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

Closed the fact-generation half of the reasoning-vision gap that RFC 0018 (multi-hop impact
tracing) left open: the *engine* could already walk `DependsOn` edges transitively, but nothing
had ever emitted one. This session adds `DependencyAnalyzerPass`, a pattern-matching recovery pass
that detects known technology signatures (import statements, connection strings) across source
files and emits `DependsOn` edges to deduplicated synthetic Technology objects — zero new
dependencies, plain substring matching. It also extends `FileObserver` to harvest declaration-line
symbol names from full file content (not just the 600-char excerpt), making `ekos_search
"authenticate"` findable even when the matching `fn`/`def`/`class` sits deep in a large file. Both
ledger backends (`Ledger`'s SQLite FTS5, `FactLedger`'s tantivy index) now feed `excerpt` + `symbols`
into searchable content. This makes both of RFC 0018's original motivating example queries — "where
is X implemented" and "what breaks if I replace Postgres with Cosmos DB" — real end-to-end, not just
proven-on-the-engine.

---

## RFC 0019 — Dependency-Fact Extraction + Symbol Harvesting

### Problem / motivation

`RelationshipKind::DependsOn` and `Calls` had zero real construction sites anywhere outside test
fixtures — the taxonomy existed, nothing populated it. Separately, `ekos_search` could only match a
file's *name* or its first 600 characters (`EXCERPT_MAX_CHARS`, RFC 0014) — a declaration past that
window was invisible to search, with no code-level indexing finer than a file.

### What was built

| Component | File | Detail |
|---|---|---|
| `DependencyAnalyzerPass` | `recovery/src/dependency_analyzer.rs` | New `CompilerPass`: scans `(path, content)` pairs for a fixed table of literal patterns (`postgres://`, `psycopg2`, `org.postgresql`, `require('pg')`, …, covering PostgreSQL/MySQL/MongoDB/Redis/Kafka across Python/JS/Java idioms), emits `DependsOn` edges + deduplicated `Custom("Technology")` objects + evidence |
| `harvest_symbols` | `plugins/file/src/lib.rs` | Scans full file content (not the excerpt) for `fn `/`def `/`class `/`func `/`interface ` declaration lines, capped at 50 symbols; rides on the observation artifact as `data["symbols"]` alongside `excerpt` |
| `KirObject::indexed_content` | `kir/src/lib.rs` | New method: `excerpt` + space-joined `symbols` — the one shared definition of "what FTS indexes" for an object |
| FTS indexing | `ledger/src/lib.rs` (both v1/v2 paths), `ledger/src/fact_ledger.rs` | Both backends now feed `indexed_content()`-equivalent text (symbols alongside excerpt) into their search index |
| `recover.rs` wiring | `cli/src/commands/recover.rs` | New file walk (extensions `.py`/`.js`/`.ts`/`.java`/`.go`/`.rb`) collecting full content, one `DependencyAnalyzerPass` per `ekos recover` run (batches all matched files, like `GitAnalyzerPass`/`CryptoAnalyzerPass`, not per-file like `SqlAnalyzerPass`) |
| `docs/rfcs/0019-dependency-extraction-symbol-harvesting.md` | new | Full RFC: motivation, design, alternatives (explicitly rejects AST/tree-sitter for v1), tests, acceptance criteria |

### Implementation details worth remembering

- **Zero new crate dependencies** — both the dependency-pattern matcher and the symbol harvester are
  plain `str::contains`/prefix-matching, not `regex`. `regex` is already a *transitive* dependency
  (via tantivy) but was never added as a direct one; this RFC keeps it that way, matching the
  Ollama provider's earlier "reuse what's there" pattern.
- **Technology object dedup uses the same `Uuid::new_v5` scheme as everything else in this
  codebase** — `technology_kir_id(name)` is deterministic across files *and* across repeated
  `ekos recover` runs, so re-running recovery doesn't create duplicate Technology objects; the
  file-object id (`file_kir_id`) deliberately reuses `build.rs`'s exact scheme so the `DependsOn`
  edge lands on the same object id `ekos_search`/`ekos_impact` already resolve — this only works
  because both places independently derive the id the same way (`Uuid::new_v5(NAMESPACE_URL,
  rel_path.as_bytes())`); if that scheme ever changes in one place, it must change in the other too.
- **One pass, many files, not one pass per file** — deliberately different from `SqlAnalyzerPass`'s
  shape. Technology-object dedup needs a single pass's local `HashMap` to avoid emitting two
  different-but-equivalent Technology objects before `append_object`'s content-addressed
  idempotency has a chance to reconcile them across runs.
- **`harvest_symbols` scans full content; `text_excerpt` still caps at 600 chars** — this is the
  detail that makes symbol harvesting actually useful: a large file's opening 600 characters rarely
  contains its `fn authenticate_user`, but the full-content symbol scan still finds it. Only the
  harvested *names* (not the full body) get stored, keeping indexed content bounded the same way
  the excerpt always has.
- A pre-existing, unrelated `unused_imports` warning (`std::io::Write as _` in
  `plugins/file/src/lib.rs`'s test module) was cleaned up while in that file — it only surfaced
  under `--tests`, which the CI clippy invocation (`cargo clippy --workspace -- -D warnings`,
  no `--tests`) doesn't exercise, so it wasn't actually blocking CI, but was worth a one-line fix
  since it was sitting right next to this session's edits.

### Decisions (alternatives considered, why this choice)

- **Full AST/tree-sitter parsing** — rejected for v1 (same call as RFC 0018's traversal-engine
  scope decisions): confirmed absent from the tree, and a multi-language parser is disproportionate
  to what's needed to prove "the fact-generation gap can close without an engine change." Documented
  as future scope if the pattern table's false-negative rate proves too high in practice.
- **A general import-statement parser instead of a fixed pattern table** — rejected; a fixed,
  documented table is transparent and trivially extensible (new technology = new table row), and
  a parser still can't cover every language's import syntax without per-language grammars anyway.

---

## Knowledge Captured

- **The two ledger backends (`Ledger`'s SQLite FTS5 and `FactLedger`'s tantivy) each maintain their
  own copy of "what text feeds search"** — RFC 0014 only ever touched `excerpt` in both places
  independently; RFC 0019 had to touch both again for `symbols`. Any future property that should be
  searchable needs the same two-backend update, not just one. `KirObject::indexed_content()` now
  centralizes the *logic* for the SQLite side but `fact_ledger.rs`'s `index_object` works over raw
  `serde_json::Value` payloads, not `KirObject`, so its symbol-joining logic is a separate,
  parallel implementation — worth reconciling into one shared helper if a third property is ever
  added.
- **`file_kir_id`'s cross-file-consistency requirement is implicit, not enforced** — nothing stops
  `build.rs`'s file-object id derivation and `dependency_analyzer.rs`'s from silently diverging in a
  future edit (they're two independent `Uuid::new_v5(NAMESPACE_URL, rel_path)` call sites). If file
  object ids ever need namespacing (e.g. per-repo prefixes in a multi-repo estate), both sites must
  change together — a good candidate for extracting into one shared function if a third caller ever
  appears.
- **Pattern-based dependency extraction is a documented floor, not a ceiling** — it will miss
  aliased imports (`import psycopg2 as db`), re-exports, and anything behind a build-time constant
  concatenated into a connection string. This is an accepted, explicit v1 tradeoff (RFC 0019's
  Alternatives Considered) — the honest framing for the demo is "here's what obviously depends on
  X," not "here's proof nothing else does."

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0019-dependency-extraction-symbol-harvesting.md` | New RFC |
| `ekos/crates/recovery/src/dependency_analyzer.rs` | New `DependencyAnalyzerPass` + tests |
| `ekos/crates/recovery/src/lib.rs` | Export `dependency_analyzer` module |
| `ekos/crates/cli/src/commands/recover.rs` | New source-file walk + pass registration + summary line |
| `ekos/plugins/file/src/lib.rs` | `harvest_symbols` + `symbols` observation field + tests |
| `ekos/crates/cli/src/commands/build.rs` | Promotes `symbols` into the object's `"symbols"` property |
| `ekos/crates/kir/src/lib.rs` | `KirObject::indexed_content()` |
| `ekos/crates/ledger/src/lib.rs` | Both FTS index paths use `indexed_content()`; new symbol-search test |
| `ekos/crates/ledger/src/fact_ledger.rs` | `index_object` folds `symbols` into indexed content; new test |
| `demo/DEMO.md` | Act 8 notes updated: both example queries now verified real end-to-end |
| `todo_v2.md` | MC-002 updated with RFC 0019 closure note |

# Devlog 2 — Phase 6: Knowledge Recovery (SqlAnalyzer, GitAnalyzer, LLM layer)

**Date:** 2026-07-02  
**PRs:** —  
**Branch:** main

---

## Summary

Implemented Phase 6 — Knowledge Recovery. The compiler can now extract business entities,
foreign-key relationships, and git change patterns from SQL DDL and git history without
touching any production system. 65 tests pass, clippy clean.

---

## Phase 6 — Knowledge Recovery (`crates/recovery/`)

### Problem / motivation

Phases 1–5 gave us the infrastructure (artifact store, observation SDK, KIR). Phase 6 is where
the system becomes useful: structured knowledge is extracted from raw artifacts and written to
the ledger as KIR.

### What was built

| Component | Description |
|---|---|
| `LlmProvider` trait | `complete(LlmRequest) -> LlmResponse`; temperature=0 by contract |
| `MockLlmProvider` | In-process stub for tests; no network, returns fixed JSON |
| `AnthropicProvider` | HTTP client for Anthropic Messages API; always sends `temperature: 0` |
| `CachedLlmProvider<T>` | Wraps any provider; cache key = SHA-256(model ‖ prompt_version ‖ system ‖ user); Git object-store layout |
| `SqlAnalyzerPass` | Parses SQL DDL with `sqlparser`, adds LLM semantic enrichment |
| `parse_ddl_structural()` | Pure structural SQL → KirGraph (no LLM dep, tested standalone) |
| `GitAnalyzerPass` | Converts git commit ObservationArtifacts → KirEvents + CoupledWith relationships |
| `ekos recover` | CLI command: finds SQL files + git artifacts, runs passes, writes KnowledgeArtifacts |

### Implementation details worth remembering

- **Inline FK references**: The ecommerce fixture uses `col INT REFERENCES other(id)` syntax,
  not `CONSTRAINT fk FOREIGN KEY (col) REFERENCES other(id)`. These are `ColumnOption::ForeignKey`
  in the sqlparser AST, not `TableConstraint::ForeignKey`. Both must be handled.

- **LLM degradation path**: `SqlAnalyzerPass.run` has three outcomes for the LLM call:
  1. Cache hit → enrichment applied
  2. Cache miss, API available → call API, write cache, apply enrichment
  3. API unavailable or bad JSON → `Warning` diagnostic emitted, structural-only output returned.
  In all cases `run` returns `Ok(())` so the pass manager continues.

- **if-let chains (Rust 2024)**: `if A && let Some(x) = b && let Some(y) = c { ... }` is valid
  in edition 2024. Clippy enforces this when two or three nested `if`/`if let` can be combined.
  The outer `if` opens exactly one brace; removing inner `if`s means removing their closing braces.

- **Contributor KIR IDs**: `KirId(Uuid::new_v5(NAMESPACE_URL, "contributor:<name>"))` — stable
  across runs. The same author always maps to the same KirId even across multiple builds.

- **File coupling min threshold**: `GitAnalyzerPass::with_min_coupling(n)` — default 2.
  Two files that co-change in only 1 commit produce no `CoupledWith` relationship (noise filter).

### Decisions

- **`ekos recover` reads workspace files directly** rather than through observation artifacts.
  SQL file content is not stored in the artifact store (only path + size + sha256 is). The pass
  reads the file from disk. This is acceptable for v0.x; Phase 9+ should consider storing content.

- **`MockLlmProvider` is always compiled**, not cfg-gated. This lets integration tests in other
  crates use it without feature flags. It is zero-weight in release builds.

- **`AnthropicProvider` is always compiled** (not behind a feature flag). Users without an API
  key get the mock automatically via `recover.rs::build_llm_provider`.

---

## RFC Written

- **RFC 0008** (`docs/rfcs/0008-llm-policy.md`) — temperature=0, prompt versioning, cache key
  formula, API key fallback behaviour

---

## Knowledge Captured

- **sqlparser AST for inline FK**: `ColumnOptionDef { option: ColumnOption::ForeignKey { foreign_table, referred_columns, .. } }`. The `referred_columns` list may be empty when the target is an implied primary key (e.g., `REFERENCES categories(id)` with just `id` — some dialects omit the parens).

- **CachedLlmProvider persists across process restarts**: The on-disk cache means CI pipelines with
  a seeded cache can run `ekos recover` without API credentials. Seed the cache once per model/prompt
  version bump.

- **Reqwest with rustls-tls**: `reqwest = { features = ["json", "rustls-tls"], default-features = false }` avoids the OpenSSL system dependency, making the build reproducible on all Linux distros.

- **ekos recover is O(SQL files × LLM calls)**: Each SQL file is one LLM call. With caching,
  subsequent runs that don't change any SQL file cost zero API calls.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/recovery/src/lib.rs` | Full implementation; public API |
| `crates/recovery/src/llm.rs` | LlmProvider trait, LlmRequest, LlmResponse, MockLlmProvider |
| `crates/recovery/src/cache.rs` | CachedLlmProvider with disk cache + 2 tests |
| `crates/recovery/src/anthropic.rs` | AnthropicProvider HTTP client |
| `crates/recovery/src/sql_analyzer.rs` | SqlAnalyzerPass + parse_ddl_structural + 5 tests |
| `crates/recovery/src/git_analyzer.rs` | GitAnalyzerPass (events, contributors, coupling) + 3 tests |
| `crates/recovery/Cargo.toml` | Added sqlparser, reqwest, uuid, sha2, hex, chrono |
| `crates/compiler-core/src/scheduler.rs` | Added ExecutionReport::error_outcomes() |
| `crates/cli/src/commands/recover.rs` | New: ekos recover command |
| `crates/cli/src/commands/mod.rs` | pub mod recover |
| `crates/cli/src/bin/ekos.rs` | Added Recover subcommand |
| `crates/cli/Cargo.toml` | Added ekos-recovery |
| `Cargo.toml` (workspace) | Added sqlparser = "0.53", reqwest, ekos-recovery |
| `docs/rfcs/0008-llm-policy.md` | RFC 0008 — LLM policy |

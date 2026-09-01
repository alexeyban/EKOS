# Devlog 146 — RFC 0124: the REASON surface — `ekos ask` compiled, MCP `ekos_query`/`ekos_retrieve`, EKL `SEMANTIC`

**Date:** 2026-09-01
**PRs:** commit on branch `rfc/0118-compiled-knowledge-query-engine` → `main`
**Branch:** `rfc/0118-compiled-knowledge-query-engine` → `main`

---

## Summary

Phase 5 (final for the offline path) of RFC 0118. Phases 0–4 built the retrieval seam, query
understanding, the fact/graph QUERY surface, and the REASON planner + typed `EvidenceSet` — but
none of it was reachable from a user surface. RFC 0124 wires it:

- **`ekos ask` compiles the question by default** — REASON planner → `EvidenceSet` → "explain
  this evidence, cite each item." `--classic` keeps the pre-0123 `gather_context` path (implied
  by `--stream`); `--explain` prints the plan + evidence.
- **MCP** gains `ekos_query` (compiled fact + graph answer, no LLM) and `ekos_retrieve` (the
  plan + evidence set + query understanding, no LLM — "show your work"); `ekos_search` gains
  `limit`.
- **EKL** gains `SEMANTIC 'text' [LIMIT k]` — the retriever as a candidate-set strategy.
- **`ekos query find --explain`** prints the compiled plan for a search string.

Also fixed a **pre-existing** `tests/integration` build break (unrelated to this work) that came
in with commit `2896481` — `compile_worker_run` grew a 5th param (`force_resolve`) but its one
integration call site was never updated.

---

## PR — RFC 0124 (Phase 5 of RFC 0118)

### What was built

| Area | Change |
|---|---|
| `runtime` | `Serialize` on `QueryType`/`StructuralOp` (`retrieval.rs`) and `EntityRef`/`PlanNode`/`QueryPlan` (`reason.rs`) — so `--explain` and `ekos_retrieve` can emit the plan as JSON. `AiRuntime::reason_with_history` (history threaded into `LlmRequest.history` like `ask_with_history`; `reason` delegates to it with `&[]`). `reason::render_plan(&QueryPlan) -> String` — the shared `--explain` tree renderer. |
| `ekos ask` (`cli/commands/ask.rs`) | Rewrote `run(config, cwd, question, AskOpts)` where `AskOpts { json, stream, session, classic, explain }`. Default → `reason_with_history`. `--classic` → `ask_with_history` (unchanged path). `--stream` → implies `--classic` + a stderr note (REASON streaming unsupported — same RFC 0098 trailing-citation-block limitation, lower value for a compiled answer). `--explain` (REASON only): plan + evidence rendered after the answer, or nested under `plan`/`evidence` keys with `--json`. `--explain --classic` and `--stream --explain` are rejected with clear errors. |
| MCP (`cli/commands/mcp.rs`) | `ekos_search` `limit` param (default 20, clamped 1–100). New `ekos_query` → `plan_question` + `execute` → serialized `EvidenceSet`. New `ekos_retrieve` → `{ plan, evidence, understanding: { query_type, keywords, resolved_entities } }`. Both no-LLM, read-only, `Cheap` cost class (not cached — offline and fast). `tools/list` count += 2; module doc-comment + count test updated. |
| EKL (`ekl/parser.rs` + `interpreter.rs`) | `EklAst.semantic: Option<String>`. Parser: a `SEMANTIC` arm in the clause loop (`expect_string`, optional inline `LIMIT k`), + parse-time rejections for `SEMANTIC` combined with `FROM` / `AS OF` / `COUNT` / `FIND Relationship`. Interpreter: a branch in `candidate_rows` above the `(entity, from)` match — candidates are the ranked `runtime.retrieve()` hits hydrated to object rows, so `WHERE`/`RETURN`/`ORDER BY`/`LIMIT` apply unchanged. `WHERE` still precedes the clause list, as for every EKL query. |
| `ekos query find` (`cli/commands/query.rs`) | `--explain` flag → prints `render_plan(plan_question(query))` before results. |
| docs | README MCP-tools line; `docs/generated/ekos-self-documentation.html` MCP tool grid + EKL section + a new `ekos ask` paragraph. |

### Decisions (from the scoping questions this session)

- **Flip `ekos ask` to REASON now, keep `--classic`.** Not an opt-in flag first — every existing
  caller of the `ekos ask` *CLI* gets the compiled path immediately, with a one-word escape
  hatch. The `AiRuntime::ask*` methods are untouched, so the **demo server** (RFC 0045) and
  **`docs generate --prose`** (RFC 0035), which call `ask*` directly, stay on the classic path —
  moving them is a deliberate later step once `ekos ask` REASON has real mileage.
- **Full Phase 5 scope** — `SEMANTIC` and `ekos query find --explain` included, not deferred.
- **No `reason_stream`.** `--stream` implies `--classic`. A streamed compiled answer still can't
  have its `{"cited_evidence": …}` trailer stripped live, and streaming a *compiled* answer is
  low value. Trivial to add later if asked.
- **`SEMANTIC` rejections are parse-time, explicit, named** — never a silent degrade. Same
  discipline as RFC 0096's `AS OF` + `FROM` rejection.

### Implementation details worth remembering

- **`SEMANTIC` + `WHERE` ordering.** EKL parses the single optional `WHERE` clause *before* the
  free-order clause loop (`FROM`/`VIA`/`RETURN`/`LIMIT`/`AS OF`/`COUNT`/…). So `SEMANTIC` sits in
  that loop and `WHERE` must come first: `FIND Object WHERE kind = 'Symbol' SEMANTIC 'welcome email'`.
  The RFC's first draft had the two swapped — corrected.
- **`ekos_search` `limit` only bites past 50 on the FactLedger backend.** `Ledger::find_objects`
  (SQLite) hard-caps its FTS query at `LIMIT 50` regardless (both v1 and v2). The tool sets
  `RetrievalRequest.limit` and also `.take(limit)`s, so `limit ≤ 50` is exact everywhere;
  `limit > 50` is only honored on the fact engine.
- **`ekos_query` / `ekos_retrieve` call the free functions, not `AiRuntime`.** `AiRuntime::new`
  needs an `Arc<dyn LlmProvider>`; a no-LLM tool has none. `plan_question` / `execute` /
  `understand` all take `&Runtime` and are sync — MCP's dispatch is sync, so this is clean.
- **The pre-existing integration break.** `tests/integration` does not compile on `main` as of
  commit `2896481` (`fix: distributed storage/query engine`) — `cluster::compile_worker_run`
  gained `force_resolve: bool` but `integration.rs:179` still passed 4 args. Fixed here with
  `false` (matches the `ekos compile-worker` CLI default). CI's `build-and-test` job runs from
  `ekos/` only, so it never caught this; `tests/integration` is a separate workspace run by hand.

### Verification

New tests: `reason_with_history` threads turns / `reason` sends empty history (runtime,
RecordingMock); `explain_with_classic_is_rejected` (ask.rs); `ekos_query` returns an
`EvidenceSet` naming the FK-dependent table / `ekos_retrieve` shows plan + `Structural`
understanding + resolved `orders` / `ekos_search` honours `limit: 1` (mcp.rs); EKL parser
`SEMANTIC` table + the 4 rejection combos; EKL interpreter `SEMANTIC` seeds candidates and an
unmatched `SEMANTIC` is empty-not-error.

Full gate: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace` (111 groups, 0 failures), `tests/integration` (4/4).

---

## Knowledge Captured

- **`ekos ask` is now a compiler, not a retriever.** The default path: `understand` → rules
  `plan` → `execute` → `EvidenceSet` (flat atomic claims, each with `location` + `extracted_by`
  + `source` evidence id) → LLM prompt is a *numbered claim list*, not `ObjectState` JSON. The
  old path is `--classic` (and `--stream`). Anyone debugging an `ekos ask` answer should reach
  for `--explain` first — it prints the exact plan and evidence the model saw.
- **EKL clause grammar is: `FIND <entity> [WHERE …] <free-order clause loop>`.** `WHERE` is
  special-cased first. Everything else (`FROM`, `VIA`, `DEPTH`, `RETURN`, `ORDER BY`, `LIMIT`,
  `AS OF`, `COUNT`, `GROUP BY`, and now `SEMANTIC`) is order-independent in the loop. A new
  clause is one `else if self.peek_keyword("X")` arm + a field + (usually) a mutual-exclusion
  check after the loop.
- **`tests/integration` is not in CI.** `.github/workflows/ci.yml`'s `build-and-test` job sets
  `working-directory: ekos` for every step. `tests/integration` and `benchmark` are separate
  path-dependency workspaces; only `benchmark` has its own CI job (`cargo bench`).
  `tests/integration` is run by hand — it broke silently on `main` for a full commit.
- **`RetrievalRequest` is `Clone` but has no builder.** To bump a limit: `let mut r =
  RetrievalRequest::lexical(text); r.limit = k;`. `per_arm_limit` is separate and defaults to 50
  — bump it too if you need > 50 pre-fusion candidates on the fact engine.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0124-reason-surface.md` | **New.** The Phase 5 RFC. |
| `ekos/crates/runtime/src/reason.rs` | `Serialize` on `EntityRef`/`PlanNode`/`QueryPlan`; `render_plan`; tests. |
| `ekos/crates/runtime/src/retrieval.rs` | `Serialize` on `QueryType`/`StructuralOp`. |
| `ekos/crates/runtime/src/ai.rs` | `reason_with_history`; `reason` delegates; 2 history tests. |
| `ekos/crates/cli/src/commands/ask.rs` | `AskOpts` struct; REASON default + `--classic`/`--explain` routing; rejection tests. |
| `ekos/crates/cli/src/commands/mcp.rs` | `ekos_query` / `ekos_retrieve` tool defs + dispatch; `ekos_search` `limit`; 3 tests; doc-comment + `tools/list` count. |
| `ekos/crates/cli/src/commands/query.rs` | `find(--explain)`. |
| `ekos/crates/cli/src/bin/ekos.rs` | `Ask { classic, explain }` + `Find { explain }` args, wired. |
| `ekos/crates/ekl/src/parser.rs` | `EklAst.semantic`; `SEMANTIC` parse arm + 4 rejections; parse tests. |
| `ekos/crates/ekl/src/interpreter.rs` | `SEMANTIC` candidate-set branch in `candidate_rows`; 2 tests. |
| `tests/integration/tests/integration.rs` | Pre-existing break fix — `compile_worker_run(…, false, false)`. |
| `README.md`, `docs/generated/ekos-self-documentation.html` | MCP tools, EKL `SEMANTIC`, `ekos ask` REASON note. |

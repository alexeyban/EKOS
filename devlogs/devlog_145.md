# Devlog 145 — RFC 0123: REASON, and the RFC 0118 query-engine series lands on main

**Date:** 2026-09-01
**PRs:** commit `c79b189` (branch `rfc/0118-compiled-knowledge-query-engine` → `main`, fast-forward)
**Branch:** `rfc/0118-compiled-knowledge-query-engine` → `main` (fast-forward, pushed `b2d6433..c79b189`)

---

## Summary

Phase 4 (final) of RFC 0118's "Compiled-Knowledge Query Engine". REASON compiles a
natural-language question into a typed **`QueryPlan`**, executes it against the QUERY surface
(RFC 0122) + the retrieval seam (RFC 0119), and hands the LLM a flat **`EvidenceSet`** of atomic,
source-traceable claims instead of a dump of whole-object JSON. New crate module
`ekos/crates/runtime/src/reason.rs` (rules planner + IR + executor) and three new `AiRuntime`
entrypoints (`plan`, `gather_evidence`, `reason`). `ekos ask` / MCP / EKL are **untouched** —
wiring them onto `reason` is RFC 0124.

This session also fast-forwarded `main` past the *entire* RFC 0118 series at once — `main` had
none of it. `main` went `b2d6433 → c79b189`, picking up the 0118 umbrella doc + phases 0–4
(RFCs 0119 retrieval seam, 0120 rank fusion, 0121 query understanding, 0122 QUERY surface,
0123 REASON). Phases 0–3 were built in earlier sessions and had no devlog; a one-paragraph
recap is in "Knowledge Captured" for continuity.

---

## PR — RFC 0123 (Phase 4 of RFC 0118): REASON

### Problem / motivation

`AiRuntime::gather_context` does one thing: BM25 → top-N → serialize each object's whole
`ObjectState` as JSON → "here are some objects, figure it out." The LLM re-derives structure the
compiler already knows, and the answer's provenance is whatever evidence ids happened to ride
along in the JSON. REASON replaces the guesswork with a compiled plan: the question is itself
compiled, the same philosophy as the rest of EKOS.

### What was built

| Component | Role |
|---|---|
| `PlanNode` enum | `Resolve` / `Search` / `Fact` / `Graph` / `Compose`. `EntityRef` is either `Resolved(KirId)` (planner bound it via RFC 0121) or `Mention(String)` (bound at execution by an earlier `Resolve`). |
| `QueryPlan` | `{ raw, query_type, root: PlanNode, confidence: f32 }` — a compiled question. |
| `plan(&QueryUnderstanding) -> QueryPlan` | The rules planner. Deterministic, offline, first-match-wins over a fixed rule order. |
| `PlannerTier { Rules, Llm }` + `plan_with` | The seam for RFC 0118 §4.2's optional `[query-planner] planner = "llm"`. `Llm` falls back to `Rules` — stub only, real tier is RFC 0124+. |
| `EvidenceItem` | `{ claim, value, source: Option<KirId>, location, confidence, extracted_by, entity }` — one atomic traceable claim. |
| `EvidenceSet` | `{ items, plan, diagnostics }`. `truncate_to(cap)` (default 60) + `source_ids()` (the "known evidence" set a citation is checked against). |
| `execute(&QueryPlan, &Runtime) -> Result<EvidenceSet, RuntimeError>` | Runs the plan. `Fact "*"` → one item per `facts_of` entry; `Graph` → `runtime.graph_op(op, seed, hops)`, one item per reached object; `Compose` → steps in order, sharing a binding environment. |
| `render_evidence(&EvidenceSet) -> String` | The numbered, cite-able context block the LLM sees — `"N. <claim> [location] (evidence <id>)"`, not raw JSON. |
| `AiRuntime::{plan, gather_evidence, reason}` | `plan` = understand + route (no LLM); `gather_evidence` = + execute (no LLM, this is the QUERY-surface answer on its own); `reason` = + LLM explains and cites via the existing `extract_citations` machinery. |

### Rules planner — the routing table (first match wins)

| # | `QueryUnderstanding` shape | `PlanNode` | confidence |
|---|---|---|---|
| 1 | fact-attribute question ("what does X return", "X's columns") + primary entity — **checked first**, ahead of the intent class | `Compose[ Fact{primary, <mapped attr>}, Fact{primary, "*"} ]` | `primary.confidence` |
| 2 | `Lookup` + primary entity | `Fact{ Resolved(primary), "*" }` | `primary.confidence` |
| 3 | `Structural` + `structural_op` + primary entity | `Compose[ Graph{op, primary, hops:2}, Fact{primary, "*"} ]` | `primary.confidence` |
| 4 | `Aggregate` | `Search{ raw, limit:50 }` + an `RSN005` diagnostic pointing at EKL `COUNT`/`GROUP BY` | 0.3 |
| 5 | `Conceptual` / `Lexical` / anything else | `Search{ keywords-or-raw }`; if a primary entity resolved, `Compose` a `Graph{Neighborhood, primary, 1}` after it | 0.5–0.8 |

`Lookup`/`Structural`/`Aggregate` with no primary entity fall through to a low-confidence (0.4)
`Search`. The fact-attribute keyword map: `returns`/`return` → `"returns"`,
`raises`/`throws`/`exception(s)` → `"raises"`, `parameters`/`params`/`arguments`/`accepts` →
`"parameters"`, `signature`, `columns`/`fields` → `"columns"`, `type`. A miss falls through to
the `Search` rule.

### Executor diagnostics

`RSN001` evidence-set truncation · `RSN002` a `Resolve` mention matched no object · `RSN003` a
`Fact`/`Graph` step had no entity to act on · `RSN004` (info) the named fact path is absent ·
`RSN005` (info) an `Aggregate` question is really an EKL job. Shape mirrors `gather_context`'s
`AIxxx` codes.

### Implementation details worth remembering

- **The fact-attribute rule has to run before the intent-class match.** RFC 0121's
  `classify_intent` maps `"what does …"` to `Structural`/`Dependencies` (a real phrase in its
  table). So `"what does authenticate return"` arrives as `Structural`, not `Lexical`. `plan()`
  therefore checks `(primary_entity, fact_attr(keywords))` at the very top and returns early,
  before `match u.query_type`. The RFC's own rule table was reordered to match (it originally
  listed Structural above fact-attribute).
- **`extracted_by` has no home on `KirEvidence`.** The RFC's `EvidenceItem` design lists
  `extracted_by: String` ("the analyzer"), but `KirEvidence { id, location, fragment, confidence,
  created_at }` carries no extractor field and neither does `SourceLocation`. Implemented as:
  read the object's own `properties["source_kind" | "analyzer" | "language"]` (first non-empty
  wins), `""` otherwise. Several analyzers already set `source_kind` (`sql_transform_analyzer`,
  `python_analyzer`, `pentaho_analyzer`).
- **A `Search` node can't exceed 50 items on the SQLite ledger backend.** `Ledger::find_objects`
  hard-codes `LIMIT 50` in its FTS query (both v1 and v2). The `Search` executor sets
  `RetrievalRequest.limit`/`per_arm_limit` from the plan's `limit`, but that only bites on the
  FactLedger backend. The truncation regression test therefore drives the cap through
  `Fact "*"` on an 80-property entity, not through `Search`.
- **`ekos_runtime` gained `ekos-identity` as a dependency at RFC 0121** (fuzzy name match in
  `retrieval.rs`), but `benchmark/Cargo.lock` was never regenerated. This commit carries that
  one-line lock update so the `benchmark/` workspace's `cargo bench` CI job doesn't have to.

### Decisions

- **REASON runs alongside `gather_context`, not replacing it.** `ask`/`ask_stream`/MCP/EKL are
  byte-for-byte unchanged this phase. The well-tested path keeps serving real traffic while
  `reason` is proven. Cutover is RFC 0124.
- **`Compare` node deferred.** RFC 0118 §4 names a `Compare` node for "why is X stale" —
  dated-fact diffing. It needs event-time facts the analyzers don't populate yet (RFC 0127). The
  `PlanNode` enum is left open for it; `QueryType` already has no variant for it.
- **LLM planner tier is a stub, not absent.** `PlannerTier`/`plan_with` ship so RFC 0124 can add
  the real tier without touching a signature. `Llm` currently `=> plan(u)`.
- **Item cap 60, hard.** A hub entity's `facts_of` can be hundreds of triples. 60 keeps the LLM
  prompt bounded; truncation is a visible `RSN001`, not silent.

### Verification

`cargo test -p ekos-runtime` — 63 lib + 6 integration, green. New tests: the ~14-row planner
table (`planner_routes_by_query_shape` — lookup, all 5 structural ops, 5 fact-attribute keywords,
aggregate, conceptual-with-entity, conceptual-without), `plan_with(_, Llm) == rules`, executor
units (`Fact "*"` provenance incl. `extracted_by`, `Graph Dependents` on `a<-b<-c`,
`Compose[Resolve, Graph{Mention}]` binding, cap+`RSN001`, aggregate+`RSN005`), and `reason` e2e
through `MockLlmProvider` (a cited `EvidenceItem.source` survives `extract_citations`).

Full gate before the push: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace` — all clean.

### Incidental

Fixed 3 pre-existing `clippy::field_reassign_with_default` errors in `ai.rs`'s own test module
(`let mut config = …::default(); config.x = …` → struct-update syntax) so `ekos-runtime` passes
`clippy --all-targets -D warnings`. Not caused by this branch — a newer local clippy (1.93.0)
than CI's pinned stable started flagging them.

---

## Knowledge Captured

- **The RFC 0118 series, in one paragraph** (phases 0–3, built in earlier sessions, no devlog
  until now): **0119** put a scored multi-signal `retrieve()` seam on `KnowledgeStore` with a
  default impl that wraps `find_objects` and produces byte-identical ordering (RRF constant
  `RRF_K = 60`). **0120** made it real — `rrf_fuse` (Cormack RRF) + an `ExactName` arm that
  promotes a case-insensitive exact name match to #1. **0121** added query understanding:
  hand-rolled mention extraction (quoted/dotted/CamelCase/snake) → `ekos_identity` Jaro-Winkler
  resolution (`RESOLVE_THRESHOLD = 0.82`, exact case-insensitive = 1.0) → intent rules
  (`QueryType` {Lookup, Lexical, Conceptual, Structural, Aggregate}, `StructuralOp` {Dependents,
  Dependencies, Callers, Neighborhood, Impact}). Fully offline. **0122** exposed the QUERY
  surface: `Runtime::fact(entity, attr)` / `facts_of` (dotted-path resolution into `properties`,
  `"foreign_keys.0.column"`) + `graph_op(StructuralOp, id, hops)` dispatching to the existing
  `trace_impact`/`load_neighborhood` machinery. **0123** (this devlog) is the planner that turns
  all of it into one compiled answer.
- **`main` had zero of RFC 0118 before today.** The `rfc/0118-compiled-knowledge-query-engine`
  branch accumulated all five phases as separate commits and only landed at phase 4. Anyone
  bisecting query-engine behavior on `main` sees the whole series arrive in the range
  `30b37cb..c79b189`.
- **CI does not lint test/bench targets.** `.github/workflows/ci.yml` runs
  `cargo clippy --workspace -- -D warnings` — no `--all-targets`. `#[cfg(test)]` modules and
  `benchmark/` are not clippy-gated by CI. A local `clippy --all-targets` on the full workspace
  currently surfaces ~15 pre-existing nits in `docs-gen`/`ledger`/`recovery` test code from the
  clippy version gap; they are not CI failures and were left alone.
- **`RetrievalRequest::lexical` is `limit: 50`, and the SQLite backend ignores anything larger.**
  `Ledger::find_objects_v1`/`_v2` both end `... LIMIT 50`. Only the FactLedger (fact-segment)
  backend honors a higher `RetrievalRequest.limit`. Tests that need >50 retrieved objects must
  use that backend or a non-`Search` plan node.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/reason.rs` | **New (788 lines).** The Query Plan IR (`PlanNode`, `EntityRef`, `QueryPlan`), the rules planner (`plan`, `plan_with`, `PlannerTier`, `fact_attr` keyword map), the typed Evidence Set (`EvidenceItem`, `EvidenceSet`, `truncate_to`, `source_ids`), the executor (`execute`, `exec_node`, `entity_item`, `provenance_of`), `render_evidence`, `plan_question`, and the test module. |
| `ekos/crates/runtime/src/ai.rs` | `AiRuntime::{plan, gather_evidence, reason}` + `REASON_SYSTEM_PROMPT`/`REASON_PROMPT_VERSION`. `reason` e2e + `plan`/`gather_evidence` tests. 3 pre-existing `field_reassign_with_default` clippy fixes in the test module. |
| `ekos/crates/runtime/src/lib.rs` | `pub mod reason;` + re-export `EntityRef, EvidenceItem, EvidenceSet, PlanNode, PlannerTier, QueryPlan`. |
| `ekos/docs/rfcs/0123-reason-query-plan.md` | **New.** The Phase 4 RFC — reordered rule table (fact-attribute first), `extracted_by` provenance-property note, `PlannerTier`/`plan_with` seam spec, `RSN005` in Verification. |
| `benchmark/Cargo.lock` | `ekos-identity` added under `ekos-runtime` (transitive, via RFC 0121). |

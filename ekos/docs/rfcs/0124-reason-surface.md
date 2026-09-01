# RFC 0124 — Phase 5 of RFC 0118: the surface — `ekos ask` on REASON, MCP `ekos_query` / `ekos_retrieve`, EKL `SEMANTIC`

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-09-01
**Phase 5 of:** RFC 0118 · **builds on:** RFC 0119 (retrieval seam) + 0121 (understanding) + 0122 (QUERY surface) + 0123 (REASON)

---

## Motivation

Phases 0–4 built the machinery: a scored retrieval seam, query understanding, a fact/graph QUERY
surface, and the REASON planner + typed `EvidenceSet`. **None of it is reachable from a user
surface yet.** `ekos ask` still runs the RFC 0009/0061 `gather_context` path (BM25 → dump whole
`ObjectState` JSON → LLM), MCP exposes only the old read tools, and EKL has no way to start from
a semantic candidate set.

RFC 0124 wires the surface:

1. **`ekos ask`** routes through `AiRuntime::reason` (RFC 0123) — compiled plan → `EvidenceSet` →
   "explain this evidence, cite each item." `--classic` keeps the old path; `--explain` prints
   the plan + evidence alongside the answer.
2. **MCP** gains `ekos_query` (fact + graph, **no LLM** — the QUERY surface as a tool) and
   `ekos_retrieve` (the `QueryPlan` + `EvidenceSet` + per-hit signals, **no LLM** — retrieval made
   inspectable), and `ekos_search` gains `limit`.
3. **EKL** gains `SEMANTIC 'text' [LIMIT k]` — the retriever as a candidate-set strategy, so
   `FIND Object SEMANTIC 'sends welcome emails' WHERE kind = 'Symbol'` works.
4. **`ekos query find --explain`** prints the compiled `QueryPlan` for a question.

---

## Design

### 1. `crates/runtime` — serialization + a history-aware `reason`

- **`Serialize` on the plan types.** `--explain` and `ekos_retrieve` emit the plan as JSON, so
  add `serde::Serialize` to `QueryType`, `StructuralOp` (`retrieval.rs`) and `EntityRef`,
  `PlanNode`, `QueryPlan` (`reason.rs`). `EvidenceItem` / `EvidenceSet` already derive it
  (`EvidenceSet.plan` is `#[serde(skip)]` — `ekos_retrieve` serializes plan and set separately).
- **`reason_with_history`.** `reason` today takes no history; `ekos ask --session` (RFC 0099)
  needs it. Add:

  ```rust
  impl AiRuntime<'_> {
      pub async fn reason(&self, q: &str) -> Result<AiAnswer, AiError>;                  // = reason_with_history(q, &[])
      pub async fn reason_with_history(&self, q: &str, h: &[ConversationTurn]) -> Result<AiAnswer, AiError>;
  }
  ```

  Same shape as `ask` / `ask_with_history`: history is threaded into `LlmRequest.history` as
  `user`/`assistant` pairs between the system prompt and this turn's evidence block. Retrieval
  (`gather_evidence`) is **not** history-aware — each turn plans off its own question text, the
  same documented limitation `ask_with_history` has.
- **No `reason_stream`.** `--stream` forces `--classic` this phase (a one-line diagnostic on
  `ekos ask --stream` without `--classic`). REASON's prompt is a numbered evidence list, so a
  streamed answer still can't have its trailing `{"cited_evidence": […]}` block stripped live
  (the same RFC 0098 limitation) — and streaming a *compiled* answer is lower value. A
  `reason_stream` is a trivial later addition if asked for.
- **`AiRuntimeConfig`** gains nothing. REASON's item cap is `reason.rs`'s `DEFAULT_EVIDENCE_CAP`
  (60); `max_context_chars` / `neighborhood_depth` / `max_matches` stay `gather_context`'s.

### 2. `ekos ask` — `crates/cli/src/commands/ask.rs`

```
ekos ask "<question>" [--json] [--stream] [--session <name>] [--classic] [--explain]
```

| flag | behaviour |
|---|---|
| *(default)* | `reason_with_history` — the REASON pipeline. |
| `--classic` | `ask_with_history` — the RFC 0061 `gather_context` path, unchanged. |
| `--stream` | implies `--classic` (errors if `--classic` was not given **and** `--explain` was — otherwise just warns and proceeds classic); `ask_stream_with_history`. |
| `--explain` | after the answer, print the `QueryPlan` (query type, routing confidence, the plan tree) and the `EvidenceSet` (each `claim` + `location` + `extracted_by` + `confidence`). With `--json`, both nest under `plan` / `evidence` keys in the output object. Incompatible with `--classic` (classic has no plan) — a clear error. |

`AiAnswer` is unchanged. The `Sources:` footer already prints from `answer.evidence_refs`; under
REASON those ids are `EvidenceItem.source` values, resolved through `ledger.get_evidence` exactly
as now. Diagnostics (`RSN00x`) print to stderr through the existing loop.

The demo server (`crates/demo-server`, RFC 0045) and `docs generate --prose` (RFC 0035) call
`AiRuntime::ask*` directly — **left on `--classic` behaviour this phase** by simply not changing
their call sites. Moving them is a follow-up once `ekos ask` REASON has real mileage.

### 3. MCP — `crates/cli/src/commands/mcp.rs`

Three changes to `base_tool_definitions()` + `dispatch`:

**`ekos_search`** gains `limit` (integer, default 20, max 100): `RetrievalRequest::lexical` then
`req.limit = limit`. Description unchanged otherwise.

**`ekos_query`** — new, **no LLM**:

```jsonc
{ "name": "ekos_query",
  "description": "Structured answer from compiled knowledge — fact lookup and named graph traversal, no LLM. Give a natural-language question ('what does authenticate return', 'what depends on the orders table'); returns a typed list of atomic claims, each with its source location and the analyzer that produced it. Use this instead of ekos_search+ekos_state when the question is about one entity's facts or its dependency graph.",
  "inputSchema": { "type": "object",
    "properties": { "question": { "type": "string" } },
    "required": ["question"] } }
```

Implementation: `AiRuntime::gather_evidence(question)` → serialize the `EvidenceSet`
(`items[]` + `diagnostics[]`). This is `reason` minus the LLM call — the QUERY surface as one
tool, which is what an agent that already has its own model wants.

**`ekos_retrieve`** — new, **no LLM**, the inspection tool RFC 0118 §7 requires:

```jsonc
{ "name": "ekos_retrieve",
  "description": "Debug/inspect how EKOS would answer a question: returns the compiled QueryPlan (how the question was classified and routed) and the EvidenceSet it produces, plus the raw ranked retrieval hits with their per-signal scores. No LLM, no synthesis — this is 'show your work' for ekos_query / ekos ask.",
  "inputSchema": { "type": "object",
    "properties": { "question": { "type": "string" } },
    "required": ["question"] } }
```

Implementation: `AiRuntime::plan(question)` → `execute` → return
`{ "plan": <QueryPlan>, "evidence": <EvidenceSet>, "understanding": { "query_type", "keywords", "resolved_entities" } }`.
`understanding` comes from `runtime::retrieval::understand` (already called inside `plan`; re-run
it once for the tool — it is cheap and offline).

Cost class (`query_log`, RFC 0114): `ekos_query` and `ekos_retrieve` are `Cheap` (no LLM, no
live system), same as `ekos_search`. Both are read-only — no change to the "one write-capable
tool" invariant.

`ekos mcp serve` help text + the module doc-comment tool list + the `tools/list` count test all
updated.

### 4. EKL — `SEMANTIC 'text' [LIMIT k]`

**Grammar.** A new optional clause, same slot as `FROM`:

```
FIND Object SEMANTIC 'authentication and session handling'
FIND Object WHERE kind = 'Symbol' SEMANTIC 'welcome email' LIMIT 5
```

`WHERE` precedes the clause list, as in every other EKL query; `SEMANTIC` sits in the same
optional-clause group as `FROM`.

- `EklAst` gains `pub semantic: Option<String>`.
- Lexer already emits `Str` tokens; parser adds a `peek_keyword("SEMANTIC")` arm in the clause
  loop that reads `expect_string()`. An optional inline `LIMIT k` after the string is folded into
  the existing `limit` field (the trailing `LIMIT` clause still works too; inline is a
  convenience).
- **Rejections** (parse-time, explicit — never silent): `SEMANTIC` + `FROM`
  (`SemanticWithFromUnsupported` — one is a search anchor, the other a graph anchor);
  `SEMANTIC` + `AS OF` (retrieval has no point-in-time form); `SEMANTIC` + `COUNT`
  (count the filtered rows with a plain `FIND`, semantic ranking is meaningless for a scalar).
  `SEMANTIC` on `Relationship` — rejected (`SemanticRelationshipUnsupported`); retrieval returns
  objects.

**Interpreter.** `candidate_rows` gains a branch above the `match (&ast.entity, &ast.from)`:

```rust
if let Some(text) = &ast.semantic {
    let k = ast.limit.unwrap_or(50) as usize;
    let hits = self.runtime.retrieve(&{ let mut r = RetrievalRequest::lexical(text.as_str()); r.limit = k; r })?;
    // hydrate each hit id to a full object row so WHERE / RETURN / ORDER BY work unchanged
    return Ok(hits.hits.iter()
        .filter_map(|h| self.runtime.load_object(&h.id).ok().flatten())
        .map(|o| object_row(&o))
        .collect());
}
```

`WHERE`, `RETURN`, `ORDER BY`, `LIMIT` all apply to the resulting rows exactly as for
`FIND Object` with no anchor — `SEMANTIC` only changes *which* candidates enter the pipeline
(the ranked retrieval hits instead of `list_objects()`). Retrieval order is preserved when there
is no `ORDER BY`.

**MCP `ekos_ekl`** and **`ekos ekl`** CLI inherit `SEMANTIC` for free — same interpreter.

### 5. `ekos query find --explain` — `crates/cli/src/commands/query.rs`

`find` gains `--explain`: when set, before the results, print
`understand(query)` → `plan(...)` for the query — the same `QueryPlan` renderer `ekos ask --explain`
uses (a shared `fn render_plan(&QueryPlan) -> String` in `ask.rs` or a small `cli` helper). No
behaviour change without the flag.

---

## Non-goals

- **No default move for the demo server / `docs --prose`** — they stay on `ask*` call sites.
- **No `reason_stream`** — `--stream` implies `--classic`.
- **No vector arm** (RFC 0125) — `SEMANTIC` runs on BM25 + `ExactName`, same as every other
  retrieval call today. The clause is forward-compatible: when the vector arm lands, `SEMANTIC`
  is where a query embedding gets attached.
- **No EKL `SEMANTIC` + `FROM` / `AS OF` / `COUNT`** — explicit parse errors, deferred.
- **No new fact schema / analyzer changes** — `ekos_query` works against today's `properties`.
- **No `QueryPlan` caching** — plans are rebuilt per call (cheap, offline).

---

## Verification

- **`reason_with_history`:** a 2-turn session (`MockLlm`) threads turn 1's Q/A into turn 2's
  `LlmRequest.history`; `reason(q) == reason_with_history(q, &[])`.
- **`ekos ask` default is REASON:** with a `MockLlm` echoing a citation of an `EvidenceItem.source`,
  `ekos ask "what depends on orders"` on a seeded FK ledger prints the dependent table and a
  non-empty `Sources:` footer; `--classic` produces the `gather_context` prompt shape (asserted
  via a recording mock).
- **`--explain`:** `ekos ask --explain --json "orders"` output has `plan.query_type == "Lookup"`
  and a non-empty `evidence` array; `--explain --classic` errors.
- **`--stream` implies `--classic`:** `ekos ask --stream "x"` runs the streaming path and warns.
- **MCP round-trips:** `tools/call ekos_query {question}` returns an `EvidenceSet` JSON with
  `items[].location`; `tools/call ekos_retrieve {question}` returns `plan` + `evidence` +
  `understanding`; `ekos_search {query, limit: 5}` returns ≤ 5. `tools/list` count += 2.
- **EKL `SEMANTIC`:** parse table (`SEMANTIC 'x'`, `SEMANTIC 'x' LIMIT 5`, `+ WHERE`, and each
  rejected combo → its named error); interpreter: `FIND Object SEMANTIC 'orders' WHERE kind = 'Table'`
  on a seeded ledger returns the orders row; `SEMANTIC` with no match → empty result, not an error.
- **`ekos query find --explain`:** prints `Structural` / `[orders]` for
  `"what depends on the orders table"`.
- Full workspace gate: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`,
  `cargo test --workspace`, `tests/integration`.

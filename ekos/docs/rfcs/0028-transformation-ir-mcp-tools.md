# RFC 0028 — Transformation IR MCP Tools

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-04
**Gating:** Phase 5 of `ekos-transformation-semantics-plan.md`. Depends on RFC 0027 (Transformation
IR, `crates/semantic/src/transform_ir.rs`) and its two producers, `PentahoAnalyzerPass` and
`SqlTransformAnalyzerPass` (both already shipped), which write `Custom("TransformNode")` objects
linked by `RelationshipKind::Custom("FeedsInto")` edges into the ledger. Also depends on RFC 0013
(MCP server) and RFC 0018 (impact reasoning) — this RFC adds two tools to the existing hand-rolled
JSON-RPC server, reusing `Runtime::trace_impact` exactly as `ekos_impact` already does.

---

## Motivation

RFC 0027 explicitly deferred these two tools as future work, already named in its own text:
*"the two new MCP tools (`ekos_transformation_explain`/`ekos_transformation_diff`, Phase 5, its
own RFC per existing workflow)"*. With Phases 1–3 shipped, the ledger can now hold real
Transformation IR chains — a `TableInput` step feeding a `FilterRows` step feeding a
`TableOutput` step, or a SQL `SELECT ... JOIN ... WHERE ... GROUP BY` chain — but nothing today
lets an agent *read* that chain as a coherent explanation, or *compare* two chains to check that a
migration preserved intended logic. `ekos_neighborhood`/`ekos_state` return raw KIR graphs; an
agent given a Pentaho job's Sink object today would have to manually walk `FeedsInto` edges itself
and reconstruct meaning from `properties["node_type"]`/`properties["excerpt"]` fields — exactly
the manual work EKOS exists to eliminate, and precisely the gap the `ekos-transformation-semantics-plan.md`
target scenario is built around: reproducing an existing Pentaho job's logic in a new pipeline with
one changed rule, verified against the original.

## Design

Both tools are added to the existing `match name { ... }` block in
`crates/cli/src/commands/mcp.rs::call_tool` (currently ending at `"ekos_status"`, line ~389),
following the exact pattern already established there — `required_id`/`required_str` argument
parsing, `anyhow::Result<Value>` return, tool-level errors surfaced as `isError: true` results
rather than JSON-RPC protocol errors (matching `tools_call`'s existing wrapping). New entries are
also added to `tool_definitions()`'s JSON array and the `tools/list` name-order test.

### Walking a Transformation IR chain — shared helper

Both tools need to walk backward from a `Custom("TransformNode")` object along
`RelationshipKind::Custom("FeedsInto")` edges to collect everything upstream of it (e.g., from a
pipeline's `Sink` back through its `Filter`/`Join`/`Aggregate`/`Calculate` nodes to its `Source`s).
This reuses `Runtime::trace_impact` exactly as `ekos_impact` already does — no new graph-walking
mechanism:

```rust
// FeedsInto edges point downstream (Source -> Filter -> Sink), so walking
// *upstream* from `id` means following edges where `rel.to == current` back
// to `rel.from` — that's `ImpactDirection::Dependents` ("what points at
// this"), not `Dependencies`, despite "what feeds into this" sounding like
// a dependency relationship at first glance (confirmed against
// `trace_impact`'s actual loop, `crates/runtime/src/lib.rs:156-159`, not
// assumed from the enum variant names alone).
let hops = runtime.trace_impact(
    &id,
    ImpactDirection::Dependents,
    &[RelationshipKind::Custom("FeedsInto".to_string())],
    max_hops, // default 50 — a pipeline's node count, not a generic dependency graph's
)?;
```
`trace_impact` (`crates/runtime/src/lib.rs:133`) is already directional, kind-filterable,
cycle-safe, and hop-bounded, returning `Vec<ImpactHop { hop, object: KirObject, via:
KirRelationship }>` with the root excluded — the root (the object the tool was called with) is
prepended separately by both handlers below. A private helper function,
`transformation_chain(runtime, id, max_hops) -> Result<Vec<KirObject>>`, wraps this exactly:
loads the root object via `runtime.load_object(&id)`, calls `trace_impact` as above, and returns
`[root, ...upstream objects in hop order]` — reused by both tool handlers below, avoiding writing
the same five lines twice.

### `ekos_transformation_explain(object_id, max_hops?)`

```rust
"ekos_transformation_explain" => {
    let id = required_id(args)?;
    let max_hops = args.get("max_hops").and_then(Value::as_u64).unwrap_or(50) as u32;
    let chain = transformation_chain(&runtime, &id, max_hops)?;

    let steps: Vec<Value> = chain
        .iter()
        .map(|obj| explain_node(&runtime, obj))
        .collect::<Result<_>>()?;

    Ok(json!({
        "target": { "id": id.to_string() },
        "steps": steps,
        "step_count": steps.len(),
    }))
}
```

`explain_node(runtime, obj) -> Result<Value>` reads `obj.properties["node_type"]` and renders a
human-readable `summary` string per variant, mirroring RFC 0027's own lowering table
(`crates/semantic/src/transform_ir.rs`'s `TransformNode::properties()`/`node_type()`):

| `node_type` | `summary` text |
|---|---|
| `Source` | `"reads from {object_name}"` |
| `Sink` | `"writes to {object_name}"` |
| `Filter` | `"filters rows where {excerpt}"` |
| `Calculate` | `"calculates {output} = {excerpt}"` |
| `Join` | `"{join_kind} joins on {keys}"` |
| `Aggregate` | `"groups by {group_by}, aggregates {aggs}"` |
| `Unmapped` | `"⚠ not understood: {reason} — raw: {raw}"` |

Each step also carries its evidence, resolved exactly as `ekos_state` already does
(`runtime/src/lib.rs`'s `reconstruct_state`: iterate `obj.evidence: Vec<KirId>`, call
`ledger.get_evidence(ev_id)`), giving one `KirEvidence { location, fragment, confidence }` per
step — this is what makes every claim in the explanation traceable to a source file/line, per the
plan's explicit requirement ("each claim linked to its Evidence (source file, line/step, commit if
applicable)"). Full step shape:
```json
{
  "id": "...", "node_type": "Filter", "summary": "filters rows where status = 'active'",
  "evidence": [{ "source": "jobs/load_customers.ktr", "fragment": "status = 'active'", "confidence": 1.0 }]
}
```
An `Unmapped` step's evidence still resolves the same way (its own `KirEvidence` cites the raw
source fragment) — the explanation surfaces "not understood" honestly rather than omitting the
step, matching RFC 0027's `Unmapped`-is-a-fact philosophy.

### `ekos_transformation_diff(old_id, new_id, max_hops?)`

```rust
"ekos_transformation_diff" => {
    let old_id = KirId::from_str(required_str(args, "old_id")?)
        .map_err(|_| anyhow::anyhow!("invalid `old_id`"))?;
    let new_id = KirId::from_str(required_str(args, "new_id")?)
        .map_err(|_| anyhow::anyhow!("invalid `new_id`"))?;
    let max_hops = args.get("max_hops").and_then(Value::as_u64).unwrap_or(50) as u32;

    let old_chain = transformation_chain(&runtime, &old_id, max_hops)?;
    let new_chain = transformation_chain(&runtime, &new_id, max_hops)?;

    Ok(json!({
        "old": { "id": old_id.to_string(), "step_count": old_chain.len() },
        "new": { "id": new_id.to_string(), "step_count": new_chain.len() },
        "diff": diff_chains(&old_chain, &new_chain),
    }))
}
```

**Structural diffing over node text, not a typed expression diff** — this is the design decision
RFC 0027 already flagged as the right v1 scope in its own Open Questions ("`ekos_transformation_diff`
(Phase 5) can do useful structural diffing over raw expression text... without it needing"
a typed `Expr` AST). `diff_chains(old, new) -> Value` buckets each chain's nodes by
`node_type`, renders each node to one canonical comparable string (the same text used for
`explain_node`'s `summary`, minus the English scaffolding — e.g. a `Join`'s comparable string is
`"{kind}|{sorted keys}"`, a `Filter`'s is its raw `condition` text), and reports set differences
per bucket:
```json
{
  "sources": { "added": ["gold.dim_customer_v2"], "removed": [] },
  "sinks": { "added": [], "removed": [] },
  "filters": { "added": ["region = 'EU'"], "removed": ["status = 'active'"] },
  "joins": { "added": [], "removed": [] },
  "aggregates": { "added": [], "removed": [] },
  "calculates": { "added": [], "removed": [] },
  "unmapped": { "old_count": 1, "new_count": 0 }
}
```
Set-based (not positional/index-based) — a pipeline commonly reorders steps without changing
meaning (e.g. two independent filters), so index-aligned diffing would report spurious differences.
`unmapped` is reported as counts, not text sets — two `Unmapped` nodes are rarely comparable text
(different raw XML/SQL fragments almost always differ verbatim even when semantically similar), so
a count-based signal ("did unmapped coverage get worse?") is more useful than a noisy added/removed
set. This is exactly the format the plan asks for: *"reports differences in sources, filters,
joins, aggregations, and calculations, in a format an agent can use to verify a migration preserves
intended logic"* — an agent can check `diff.filters.removed` is empty (or only expected removals)
and `diff.sources`/`diff.sinks` match, to confirm a migration didn't silently drop a source or
change a target table.

### Tool schemas (added to `tool_definitions()`)

```json
{
  "name": "ekos_transformation_explain",
  "description": "Explains a Transformation IR pipeline (Pentaho job or SQL SELECT/VIEW/procedure) by walking the chain of Source/Filter/Join/Aggregate/Calculate/Sink/Unmapped nodes feeding into the given object, with each step's evidence (source file/fragment).",
  "inputSchema": {
    "type": "object",
    "properties": {
      "id": { "type": "string", "description": "Transformation IR object id (a TransformNode, typically a Sink), from ekos_search or ekos_ekl" },
      "max_hops": { "type": "integer", "description": "Hop bound walking upstream (default 50)" }
    },
    "required": ["id"]
  }
},
{
  "name": "ekos_transformation_diff",
  "description": "Compares two Transformation IR pipelines (e.g. an old Pentaho-derived one and a newly drafted one) and reports added/removed sources, filters, joins, aggregations, and calculations — use to verify a migration preserves intended logic.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "old_id": { "type": "string", "description": "Transformation IR object id of the original pipeline's end node" },
      "new_id": { "type": "string", "description": "Transformation IR object id of the new pipeline's end node" },
      "max_hops": { "type": "integer", "description": "Hop bound walking upstream on each side (default 50)" }
    },
    "required": ["old_id", "new_id"]
  }
}
```

### Non-goals (this RFC)

- **No new graph-walking mechanism.** `trace_impact` already does everything needed; adding a
  bespoke "transformation chain walker" would duplicate `ekos_impact`'s exact mechanism for no
  benefit.
- **No semantic/business-meaning diffing.** `ekos_transformation_diff` reports structural
  differences (a filter's condition text changed) — whether that change is *intentional* (the one
  new rule the developer asked for) or an *accidental* regression is left to the calling agent's
  judgment, using `ekos_transformation_explain`'s evidence-backed output as its reasoning
  material. A future `TransformSemanticsAnalyzerPass` (flagged as anticipated future work in RFC
  0027) would be the right place for LLM-assisted business-meaning labeling, not this tool.
  _Tracked as backlog: see `TODO.md` → "Promoted from RFC Non-Goals" → "Docs generation"._
- **No requirement that `object_id` be a `Sink`.** Any `Custom("TransformNode")` object works —
  calling `ekos_transformation_explain` on a `Filter` node explains everything upstream of that
  filter, not the whole pipeline. Useful for narrowing an explanation to "what feeds this specific
  step" without a separate tool.

## Alternatives Considered

- **A dedicated Transformation-IR-only graph traversal (bypassing `Runtime::trace_impact`)** —
  rejected: `trace_impact` already provides directional, kind-filtered, cycle-safe, hop-bounded
  traversal; the only "transformation-specific" part is the fixed `RelationshipKind::Custom("FeedsInto")`
  filter and root-object prepending, both trivially wrapped in one shared private helper rather than
  justifying new Runtime API surface.
- **AST-level / typed expression diffing for `ekos_transformation_diff`** — rejected for v1, per
  RFC 0027's own Open Question: no typed `Expr` AST exists yet (deliberately — see RFC 0027's "The
  IR" section), and building one solely to power a richer diff is a large, separate project with no
  other consumer yet. Text-level set diffing over each node's rendered summary is cheap, already
  answers the plan's stated use case, and can be revisited once real usage surfaces concrete gaps.
- **Returning raw `KirGraph` (`ekos_neighborhood`'s shape) instead of a rendered explanation** —
  rejected: `ekos_neighborhood` already does this and is reusable for a Transformation IR object
  today (no new tool needed for that). The value of a dedicated tool is exactly *not* requiring the
  calling agent to re-derive `properties["node_type"]` → readable-sentence mapping itself every
  time — that mapping is written once, here, following RFC 0027's own lowering table.

## Testing

Mirrors the existing `mcp.rs` test style (`crates/cli/src/commands/mcp.rs`'s `#[cfg(test)] mod
tests`):
- A `seeded_transformation_ledger` fixture (parallel to the existing `seeded_ledger` helper)
  builds a small real `TransformGraph` (`Source → Filter → Sink`, using `ekos_semantic::transform_ir`
  directly, not a mock) and appends its `lower_to_kir` output into a real `Ledger`.
- `explain_walks_the_full_chain_with_evidence` — calling `ekos_transformation_explain` on the
  `Sink` id returns 3 steps in root-then-upstream order, each with non-empty evidence, and the
  `Filter` step's `summary` contains the condition text.
- `explain_of_unknown_object_is_a_tool_error` — mirrors `dependents_of_unknown_object_is_a_tool_error`.
- `diff_detects_added_and_removed_filter` — build two chains sharing the same `Source`/`Sink` but
  a different `Filter` condition; assert `diff.filters.added`/`removed` each contain exactly the
  expected text.
- `diff_of_identical_chains_reports_no_differences` — same chain compared to itself (or an
  independently-lowered copy of the same `TransformGraph`, proving determinism carries through)
  produces empty added/removed sets in every bucket.
- `tools_list_exposes_the_runtime_tools` (existing test) updated to include both new tool names in
  the asserted order.

## Acceptance Criteria

- [x] `ekos_transformation_explain`/`ekos_transformation_diff` added to `tool_definitions()` and
      `call_tool`'s match block in `crates/cli/src/commands/mcp.rs`.
- [x] Both reuse `Runtime::trace_impact` via a shared `transformation_chain` helper — no new
      graph-walking mechanism.
- [x] Every step in `ekos_transformation_explain`'s output carries resolved evidence (source
      path + fragment), including `Unmapped` steps.
- [x] `ekos_transformation_diff` reports added/removed sets per node-type bucket (sources, sinks,
      filters, joins, aggregates, calculates) plus `unmapped` counts, matching the plan's required
      categories (sources, filters, joins, aggregations, calculations).
- [x] All new/updated tests pass; `cargo test --workspace`, `cargo clippy --workspace --all-targets`,
      `cargo fmt --check` clean; zero `unsafe` introduced.
- [x] `tools/list`'s asserted tool-name order updated to include both new tools.

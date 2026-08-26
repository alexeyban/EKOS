# RFC 0107 — MCP Exposure of Architecture Query Tools (RFC 0068 §62 Phase 2)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0068's §62 Phase 2 tracking named "MCP exposure of architecture query/investigation tools" as
a real, still-open item — `crates/cli/src/commands/mcp.rs` is the established extension point, but
had zero architecture-specific tools. `ekos architecture investigate`'s own evaluation
(`evaluate_architecture`) and drift-detection (`drift_from_history`) logic already exist as pure,
already-tested functions in `ekos-recovery` — real, deterministic, evidence-backed signal an AI
agent working through MCP has no way to reach today without shelling out to the CLI.

## Design

Two new read-only tools, added to the existing `base_tool_definitions()`/`call_tool` dispatch
(RFC 0013's established shape — every tool but `ekos_identity_review` is read-only, going through
the cached read-only store the same way):

- **`ekos_architecture_evaluate`** — no arguments. Calls `evaluate_architecture(&ledger.all_objects()?)`
  (RFC 0065 Phase 3) directly against the already-open store and serializes the resulting
  `EvaluationReport` (already `#[derive(Serialize)]`) as-is: `score`, `completeness`,
  `evidence_coverage`, `crates_total`, `crates_classified`, `evidenced_total`, `issues`. The same
  computation `ekos architecture investigate` and `docs generate`'s Executive Summary already use —
  no new logic, just a new, cheap, read-only way to reach it without running a build.
- **`ekos_architecture_drift`** — no arguments. Reuses `architecture.rs::detect_drift`'s exact logic
  (for each compiled `Custom("Crate")` object, fetch its deterministic role-`Claim` id
  (`role_claim_kir_id`), read its real version history via `KnowledgeStore::object_history` (RFC
  0047), and run `drift_from_history`), serializing the resulting `Vec<DriftFinding>` (also already
  `#[derive(Serialize)]`).

Both tools reuse existing, already-unit-tested pure functions verbatim — this RFC's only new code
is the MCP-protocol wiring (tool schema + dispatch arm + JSON serialization), not new
evaluation/drift logic.

### Why not expose `ekos architecture investigate` itself over MCP

`investigate` runs the full `build → recover → compile → commit` pipeline (plus, when
`[architecture-reasoning]` is enabled, real LLM calls) — a write-heavy, potentially slow, costed
operation, fundamentally different in kind from every other MCP tool in this server. RFC 0068 §62's
own item is titled "architecture *query*/investigation tools" — the query half (evaluate current
state, check for drift) is exactly what's exposed here; the investigation-loop half stays a CLI-only
operation, consistent with this server's established "AI systems consume knowledge through the
Runtime only, read-only" posture (`ekos_identity_review` remains the sole, deliberate exception, and
even that only ever confirms/rejects an already-proposed candidate — it never runs a pipeline).

## Non-goals

- Exposing `investigate`'s orchestration loop itself over MCP.
- New evaluation/drift logic — both tools call existing, unmodified functions.

## Verification

New `ekos` (CLI) tests: `ekos_architecture_evaluate` appears in `tools/list`; called against a real
compiled workspace with a classified crate returns a real, non-fabricated score matching a direct
call to `evaluate_architecture` against the same objects; called against an empty ledger returns the
same honest vacuous-default report the CLI path already produces (never a fabricated 100%).
`ekos_architecture_drift` appears in `tools/list`; called against a workspace with a role `Claim`
that changed across two `commit` runs returns exactly the one real finding, matching
`ekos architecture investigate`'s own "DOCUMENTATION DRIFT DETECTED" output for the identical
fixture; called against a workspace with no drift returns an empty findings list, not an error.
Full workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test
--workspace`), `tests/integration` 3/3.

Live-verified: ran the real `ekos mcp serve` binary over stdio against a real compiled workspace,
sent a real `tools/call` for both new tools, confirmed the JSON responses carry real, non-fabricated
values matching the equivalent CLI commands run against the same ledger.

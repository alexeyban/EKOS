# RFC 0067 — Architecture Reasoning, Evaluation, and the MVP Investigation Loop

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0065 Phase 1 (`devlog_70`) delivered the static knowledge-model foundation: `Claim`/
`ArchitectureGap` KIR kinds, a deterministic extractor, a C4 note + Open Questions section. Asked
to complete RFC 0064-0066 end to end — the actual agentic loop both RFCs describe: reasoning,
evaluation, targeted re-collection, and RFC 0066's orchestrating agent, not just the static model.

Scoped explicitly to the real MVP both RFCs already define as their own first cut — RFC 0065 §67
and RFC 0066 §64-65's 12-step, max-3-iteration loop — rather than all 146 combined RFC sections.
Deliberately not in this phase, as real separate follow-on scope RFC 0066's own Phase 2/3 sections
already name as later work: persistent checkpointing/resume (§51), concurrency-safety
infrastructure (§53-54), CI/CD exit-code matrix and PR-comment workflow (§49-50), multi-format
output, a `--llm cloud/local/offline` flag matrix (the existing `[llm]` config already selects the
provider), human-review UI, MCP additions, Phase 2/3 extractors.

## Design

### Reasoning (RFC 0065 §14-15, §41-49) — `ArchitectureReasoningPass`

Modeled directly on the existing `DocumentSemanticsAnalyzerPass` (RFC 0026) — a complete, working
template for RFC 0065 §46's "LLM Output Contract": read existing KIR objects, one strict-JSON-
schema prompt via `ekos_recovery::llm::LlmProvider` (which already *is* RFC 0065 §41's
`LLMProvider` abstraction — `anthropic.rs`/`ollama.rs` implementations, temperature-0, structured
output already enforced), validate the response against the real input before writing anything,
output as new evidence-linked `KirObject`s. Never mutates existing state directly, matching §46's
"LLMs must never directly write authoritative architecture state."

Reads `CrateTopologyAnalyzerPass`'s (RFC 0042) `Custom("Crate")` objects plus dependency fan-in/
fan-out computed from its `DependsOn` edges — real deterministic signal handed to the LLM rather
than re-derived by it, per §4.5 "Deterministic Analysis Before LLM Reasoning." One batched call for
every crate in the workspace, not one call per crate — real cost discipline, not a corner cut,
matching §42's own task-allocation table.

Output lands as `Custom("Claim")` objects (the same kind Phase 1 introduced), extended with one
additive property: `properties["value"]` for a subject-attribute claim with no target entity (e.g.
`predicate: "has_role"`), alongside Phase 1's existing `properties["object_id"]` shape for
entity-to-entity claims (e.g. `predicate: "depends_on"`). `claim_type: "inference"` — RFC 0065
§12's other populated claim type, alongside Phase 1's `"fact"`.

Config-gated the same way document-semantics is: `[architecture-reasoning] enabled = false` by
default (`ArchitectureReasoningConfig`, `crates/compiler-core/src/config.rs`), registered in
`recover.rs` right after `crate-topology-analyzer`.

### Evaluation (RFC 0065 §32-39) — `evaluate_architecture`

A plain deterministic function, not an LLM call and not a `CompilerPass` — it runs after `ekos
compile`, over the compiled object set, matching §32's "independent architecture reviewer" framing.
Only two dimensions are computed, not RFC 0065 §34's full list: `completeness` (fraction of `Crate`
objects with a `has_role` `Claim`) and `evidence_coverage` (fraction of `Claim`/`ArchitectureGap`
objects that actually carry evidence, computed for real rather than assumed). Inventing scores for
dimensions with no real underlying signal (`consistency`, `cross_view_consistency`, ...) would be
exactly the "unsupported precision" §4.6 warns against — not done.

### Targeted re-collection (RFC 0065 §36, "the core agentic behavior")

Phase 1's extractor is exhaustive in one deterministic pass — a `Cargo.toml` either resolves or it
doesn't; a second scan of the same file finds nothing new. The place targeted re-collection has
real signal is Phase 2's classification confidence, not Phase 1's structural extraction:
`read_crate_doc_comment` (new, `architecture_reasoning.rs`) reads one crate's entry file's leading
`//!` module doc comment via `syn::parse_file` (the same parser `rust_analyzer.rs` already depends
on). `ArchitectureReasoningPass::with_only_dirs`/`with_crate_context` let a second pass re-classify
only the crates the evaluator flagged `missing_classification`, with that extra context appended —
without re-spending LLM budget re-classifying crates already done.

### RFC 0066 MVP Agent — `ekos architecture investigate`

One orchestrating async function (`crates/cli/src/commands/architecture.rs`), not a generic
state-machine framework or trait: this MVP runs exactly one investigation at a time, with no
persistence across restarts and no concurrent agents to coordinate. Composes existing, already-
independently-callable pipeline stages directly (`build::run`, `recover::run`, `compile::run`,
`commit::run`, `docs::generate`) rather than reimplementing collection or compilation — RFC 0066's
"agent" role, once its individual capabilities already exist as discrete passes/functions, reduces
to orchestration and loop control:

```
INITIALIZING → COLLECTING (broad) + ANALYZING + REASONING + UPDATING_MODEL
    → EVALUATING → DECISION
        (score ≥ threshold OR iteration ≥ max) → GENERATING → COMPLETED
        else → PLANNING_INVESTIGATION / INVESTIGATING (targeted) → loop
```

`ekos architecture investigate [--max-iterations 3] [--quality-threshold 0.90] [--output doc]` —
RFC 0066 §64-65's own MVP defaults. Forces `[architecture-reasoning] enabled = true` for the
command's own duration regardless of `ekos.toml`, the same "command-local LLM decision" shape
`docs.rs::select_llm_provider_for_prose` already uses. Always generates curated docs at the end
(RFC 0035/0037), even if the quality threshold wasn't fully reached within the iteration budget —
the command exits non-zero in that case, but the partial result is still written, not discarded.

## Non-goals

Restated from Motivation for the record: persistent checkpointing/resume, concurrency-safety
infrastructure, CI/CD exit-code matrix, PR-comment workflow, multi-format output, an `--llm`
provider-selection flag (the existing `[llm]` config already does this), human-review UI, MCP
additions, Terraform/Kubernetes/OpenAPI/SQL extractors (RFC 0065/0066 Phase 2).

## Testing

- `architecture_reasoning.rs`: real crate fixtures classified with evidence and deterministic
  signal; a hallucinated crate name rejected, not written; bad LLM JSON tolerated (degrades to zero
  extraction, never a hard pass failure); `with_only_dirs` restricts correctly;
  `read_crate_doc_comment` reads a real leading module comment and returns `None` when no entry
  file exists.
- `architecture_evaluator.rs`: known object/claim sets → known scores/issues; empty input scores
  1.0 honestly (not penalized for having nothing to evaluate); a claim missing evidence lowers
  `evidence_coverage` for real.
- `recover.rs`: the `[architecture-reasoning]` opt-in gate, same acceptance shape as
  `[document-semantics]`'s existing tests.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd tests/integration
  && cargo test`.
- **Live, real LLM call**: ran `ekos architecture investigate` against this repo's own real
  self-hosted workspace (backed up first, non-destructively — same discipline as Phase 1's
  verification), using a real local Ollama model (no API key required, no cost) rather than a
  mocked provider. See `devlogs/devlog_71.md` for the actual numbers.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0067-architecture-reasoning-and-investigation-loop.md` | This RFC |
| `ekos/crates/compiler-core/src/config.rs` | `ArchitectureReasoningConfig`, `[architecture-reasoning]` |
| `ekos/crates/recovery/src/architecture_reasoning.rs` | `ArchitectureReasoningPass`, `read_crate_doc_comment` (new) |
| `ekos/crates/recovery/src/architecture_evaluator.rs` | `evaluate_architecture`, `EvaluationReport` (new) |
| `ekos/crates/cli/src/commands/recover.rs` | Registers the new pass, opt-in-gated |
| `ekos/crates/cli/src/commands/architecture.rs` | `ekos architecture investigate` (new) |
| `ekos/crates/cli/src/bin/ekos.rs` | New `architecture investigate` subcommand |

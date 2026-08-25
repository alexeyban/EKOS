# Devlog 99 — RFC 0090: Solution Architect Report (`--layout solution-architect`)

**Date:** 2026-08-24
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Asked directly, outside any specific project: "if you analyzed a project as a solution architect for
team use, which docs would you generate, and what would you analyze?" The answer mapped mostly onto
capability EKOS already has (`docs generate --layout curated`'s README/Architecture/API/
SequenceDiagrams), but named three genuinely missing deliverables — a Dependency & Risk Report, an
Onboarding Guide, and a Findings/Recommendations memo. Rather than losing that at the end of a
conversation, filed it as RFC 0090 and, at the user's request, designed and implemented it end to
end in the same session: three new `ekos-docs-gen` render functions, CLI wiring for a new
`--layout solution-architect`, 18 new `docs-gen` tests + 5 new `crates/cli` tests, and a live
verification run against this repo's own real committed ledger.

## What was built

| Component | What it does |
|---|---|
| `render_dependency_risk_report` (`docs-gen`) | Real declared versions (`Crate.version`, npm `DependsOn` relationship `version_spec`/`dev_dependency`), a top-5 `DependsOn` fan-in concentration ranking, and an honest "CVE/license data not available" section |
| `render_onboarding_guide` (`docs-gen`) | Real `Crate.path` repository-layout table; link-throughs to `Architecture.md`'s `## CI/CD Pipelines`/`## Subsystems` instead of re-listing the same objects |
| `build_findings_evidence` + `render_findings_memo` (`docs-gen`) | Real `ArchitectureGap` objects, undeclared crate versions, and doc-comment coverage gaps (grouped by kind) — zero new detection, all sourced from data other passes already compiled |
| `generate_solution_architect` + `enrich_findings_memo` (`cli/commands/docs.rs`) | Wires the three pages into `--layout solution-architect`; `enrich_findings_memo` optionally layers an LLM executive summary onto the findings list via `--prose`, reusing the existing spend-confirmation flow |

## Implementation details worth remembering

**Discovered mid-design that two of the three planned pages would have duplicated existing
content.** The RFC's original sketch planned to re-render `ArchitectureGap` objects and `Pipeline`
triggers/jobs directly — but `render_architecture` already has a `## Open Questions` section (exact
same `ArchitectureGap` objects) and a `## CI/CD Pipelines` section (exact same `Pipeline` objects).
Caught this by reading `render_architecture`'s actual current output before writing the new
functions, not by re-deriving it from the RFC text. Resolved by following a "link-through, don't
duplicate" convention this codebase already established in several places (`components_cross_
reference`, the Runtime View section linking to `SequenceDiagrams.md`, Data Architecture's own
link-through) — `OnboardingGuide.md` links to `Architecture.md` for full CI/CD/subsystem detail
instead of re-listing it; `FindingsMemo.md` still re-surfaces `ArchitectureGap` objects (deliberately
— a different audience, an actionable punch list vs. a transparency note) but combines them with two
finding categories genuinely not rendered anywhere else (undeclared versions, doc-coverage gaps).

**The Findings memo's LLM call is deliberately not `AiRuntime::ask`.** The existing `--layout
objects --prose` path (`enrich_with_prose`) calls `ai.ask(&model.name)`, relying on `Runtime::
find_objects`' keyword retrieval to ground the response. That doesn't fit here: there's no single
object name to retrieve against, since the grounding *is* the already-deterministic candidate list
itself. Followed `llm_description.rs::describe_project`'s pattern instead — build a JSON prompt
directly from compiled data, call `LlmProvider::complete` directly. Two different existing precedents
in the same codebase for "grounded LLM call," and picking the wrong one would have produced an
`ai.ask` call with a query string that retrieves nothing relevant.

**The `--prose` executive summary is additive, not a replacement**, matching RFC 0088's `##
AI-Assisted Overview` convention (a section added alongside real compiled content) rather than the
"prose supersedes" shape I initially assumed while writing the approved plan. Caught this rereading
`ObjectPageModel`'s actual rendering before implementing — `ProseSection` doesn't replace anything
either. `render_findings_memo` renders an optional `## Executive Summary (AI-Assisted)` above the
deterministic `## Detailed Findings` list, never in place of it.

## Live verification against a real ledger

This repo self-dogfoods — a real, already-committed `.ekos/` workspace exists at the repo root from
prior sessions. Ran `ekos docs generate --layout solution-architect` directly against it (no rebuild
needed) and inspected real output:

- `DependencyRiskReport.md`: real crate names (`ekos-kir`, `ekos-recovery`, ...), correctly "not
  declared" for crates using `version.workspace = true`; real fan-in ranking (`serde_json` 132
  dependents, `thiserror` 113, `tokio` 96, `tracing` 93, `async-trait` 86).
- `OnboardingGuide.md`: real `Crate.path` → name table for all ~44 crates/plugins; correctly picked
  `archive/doc/entities` (1,899 member files) as the largest compiled subsystem.
- `FindingsMemo.md`: a real finding — `1625/1625 RustSymbol` and `584/584 RustModule` objects with no
  captured `description`. Cross-checked against this repo's own already-generated `doc/entities/
  rustsymbol/re/render-readme.md`: confirmed the real compiled `render_readme` object genuinely has
  no `description` property, despite the real source carrying a substantial `///` doc comment — this
  ledger snapshot predates or wasn't recommitted since RFC 0087's doc-comment capture went live. A
  real, honest, orthogonal finding the new memo correctly surfaces, not a bug in the new code — and
  arguably the single best illustration of why this memo is useful: it caught a real staleness gap in
  this project's own dogfooded ledger that nothing else surfaces.

`--prose`'s LLM path was verified via `MockLlmProvider` (`enrich_findings_memo_sets_prose_on_success`)
and the no-credentials error path only (`generate_solution_architect_with_prose_errors_clearly_
instead_of_silently_degrading`) — no `ANTHROPIC_API_KEY` configured in this environment, matching the
existing `--layout objects --prose` test-coverage convention.

## Decisions (RFC 0090's three open questions, resolved during implementation)

1. Findings evidence-gathering lives in `docs-gen` + CLI-layer orchestration, not `recovery` — it's
   a rendering-time concern with no ledger persistence needed.
2. One `--layout solution-architect` bundle reusing the existing `--prose`/`--yes` flags, not new
   `--sections` flags.
3. No speculative `vulnerabilities` field reserved on the risk-report model — YAGNI until a CVE-feed
   connector RFC actually exists.

## Knowledge Captured

- **Read the actual current output of a page before designing a new one that might overlap it.**
  The RFC's own design sketch (written before any code) proposed re-rendering `ArchitectureGap`/
  `Pipeline` data that `render_architecture` already renders in full — caught only by reading
  `render_architecture`'s real source during the Interfaces step, not by re-deriving intent from the
  RFC text. This project's own "Interfaces before Implementation" workflow step exists exactly to
  catch this kind of thing; skipping straight from RFC to code would have shipped a redundant page.
- **Two legitimate patterns exist in this codebase for "grounded LLM call," and they're not
  interchangeable**: `AiRuntime::ask` (retrieval-based, needs a real short keyword/object-name query)
  for per-object narration, vs. direct `LlmProvider::complete` with a hand-built prompt (used by
  `llm_description.rs`) when the grounding is already fully assembled deterministic data with no
  natural retrieval query. Picking `ask` for the latter case would silently degrade — its own
  `enrich_with_prose` doc comment already documents this exact pitfall for a different reason (full
  sentences vs. keywords), worth remembering it generalizes further.
- **This repo's own self-dogfooded `.ekos/` ledger at the repo root is a fast, real, zero-setup
  verification target** for any new `docs generate` output — no fixture project needed, and it
  surfaced a real, previously-unknown ledger-staleness gap (RFC 0087 doc-comment data not present for
  any Rust symbol/module in the current commit) as a side effect of testing an unrelated feature.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/docs-gen/src/lib.rs` | `render_dependency_risk_report`, `render_onboarding_guide`, `build_findings_evidence`, `render_findings_memo`, `FindingCandidate`, `FindingsProse`; 18 new tests |
| `ekos/crates/cli/src/commands/docs.rs` | `Layout::SolutionArchitect`, `generate_solution_architect`, `enrich_findings_memo`; 5 new tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `--layout` help text mentions `solution-architect` |
| `ekos/docs/rfcs/0090-solution-architect-report.md` | New RFC — filed Proposed, implemented and flipped to Accepted same session |
| `README.md` | Documentation-generation section covers the third layout |
| `TODO.md` | Phase 15 entry for RFC 0090, marked complete |

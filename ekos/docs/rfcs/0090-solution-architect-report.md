# RFC 0090 — Solution Architect Report (`--layout solution-architect`)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-24
**Implemented:** 2026-08-24

---

## Motivation

Asked directly (2026-08-24 conversation, no code yet): "if you analyzed a project as a solution
architect for team use, which docs would you generate, and what would you analyze?" The answer
mapped cleanly onto capability EKOS already has, scattered across existing `docs generate` outputs,
and three gaps it doesn't cover yet. This RFC files that gap as EKOS's next documentation-generation
goal rather than losing it at the end of a conversation.

`docs generate --layout curated` (RFC 0035/0037/0042) already produces README/Architecture/API/
SequenceDiagrams plus per-entity pages, a component view (RFC 0070), a data architecture view (RFC
0074/0075), and (RFC 0089) real "Defined in" file locations. That covers most of what a solution
architect's *architecture overview* and *data flow* deliverables need — this RFC is not proposing to
redo that. What real evidence-backed analysis does **not** yet render into anything is:

1. A **Dependency & Risk Report** — real declared dependencies exist in the ledger today
   (`crate_topology_analyzer`/`package_json_analyzer`, RFC 0042/0082, parse `Cargo.toml`/
   `package.json` into real `Crate`/`JsModule` objects with real version strings; `dependency_analyzer`
   separately detects *technology usage*, e.g. "this file talks to PostgreSQL", via string-pattern
   matching into `Technology` objects joined by `DependsOn`) but nothing renders those into a single
   risk-oriented page a team can hand around.
2. An **Onboarding Guide** — build/test/run instructions, repo layout, "where do I look for X" — is
   adjacent to but distinct from `render_readme`'s project-purpose framing; no dedicated page exists.
3. A **Findings/Recommendations memo** — a prioritized, evidence-cited list of tech debt, missing
   test coverage, and security-relevant surfaces. This is the one genuinely new synthesis: it needs
   to read across objects (not describe one at a time, the way RFC 0088's `llm_description.rs`
   does) and rank them, which no existing pass does.

## Design (as implemented)

Added a third `docs generate` output, gated like the other two — deterministic-only by default, and
an opt-in `--prose`-layered LLM pass on top only for the findings memo, never fabricating findings
the grounded data doesn't support (same non-negotiable RFC 0035/RFC 0088 rule).

`--layout solution-architect` sits alongside `objects`/`curated` in `Layout` (`crates/cli/src/
commands/docs.rs`), and `generate_solution_architect` reuses the curated layout's exact "read the
committed ledger once" entry point rather than adding a second ledger read path. Three new
`ekos-docs-gen` functions:

- **`render_dependency_risk_report`** (deterministic, zero LLM, `crates/docs-gen/src/lib.rs`) — a
  `## Declared Versions` table from real `Crate.version` properties (RFC 0042) and npm `DependsOn`
  relationships' `version_spec`/`dev_dependency` properties (RFC 0082, carried on the relationship,
  not the `Technology` object, since the same package can be declared with different ranges by
  different manifests); a `## Concentration Risk` top-5 ranking by real `DependsOn` fan-in per
  `Technology` (a genuinely different framing from `render_architecture`'s own `## Technology
  Inventory`, which lists every technology's full used-by breakdown — this page ranks instead of
  re-listing, and links back for the full detail); and a `## Vulnerability & License Data` section
  that states plainly this isn't available rather than fabricating a severity score.
- **`render_onboarding_guide`** (deterministic, zero LLM) — `## Repository Layout` from real
  `Crate.path` properties (not rendered as a flat list anywhere else — `render_architecture` only
  uses `path` internally to match crates to `Rollup`s, never prints it); `## Build & CI` and `##
  Where to Look` deliberately link through to `render_architecture`'s own `## CI/CD Pipelines`/`##
  Subsystems` sections (which already render `Pipeline` objects and every `Rollup` in full) rather
  than re-listing the same objects a second time — this page adds a first-day framing, not a data
  duplicate.
- **`build_findings_evidence`** (deterministic candidate list) + **`render_findings_memo`** — real,
  already-compiled findings, zero new detection: every `Custom("ArchitectureGap")` object
  (`crate_topology_analyzer`, already evidence-backed; `render_architecture`'s own `## Open
  Questions` surfaces these individually for transparency, this memo re-surfaces them as one
  category among several for a different audience — an actionable list, not a per-object
  transparency note), `Crate` objects with no declared `version`, and doc-comment coverage
  (`"description"` property presence, RFC 0087) grouped by kind so the memo stays scannable. An
  LLM-written executive summary (`FindingsProse`) is layered *above* the deterministic list when
  `--prose` is passed — additive, matching RFC 0088's `## AI-Assisted Overview` convention, never
  replacing the real compiled list underneath it.

`enrich_findings_memo` (`commands/docs.rs`) calls the LLM once over the whole candidate list.
Deliberately **not** `AiRuntime::ask` (unlike the existing `enrich_with_prose` for `--layout
objects`): there's no single object name to retrieve grounding against here — the grounding *is*
the candidate list itself, already deterministic — so it follows `llm_description.rs::
describe_project`'s direct `LlmProvider::complete` pattern instead. Reuses the exact existing
`confirm_prose_spend`/`select_llm_provider_for_prose`/`--prose`/`--yes` flow, no new flags.

## Scope — what this does and doesn't cover

**Covers**: rendering a team-facing bundle (Dependency & Risk Report, Onboarding Guide, Findings/
Recommendations memo) from data EKOS already compiles, following the same zero-fabrication,
evidence-first rule every other `docs generate` output already follows.

**Does not cover** (explicitly out of scope for this RFC, needs its own future RFC if pursued):
- Real CVE/vulnerability lookups or license-compatibility checking — requires a new external-feed
  connector (e.g. an OSV/advisory-database Observer), not just a renderer over existing KIR.
- Git churn/hotspot analysis ("files that break often") — `git_analyzer` today extracts commit/file
  relationships but has no churn-frequency or bug-density metric; adding one is a `recovery`-crate
  change, not a `docs-gen` one.
- Test-coverage percentages — no coverage-tool Observer exists yet; the Findings memo can only cite
  what RFC 0088 already measures (doc-comment/description coverage), not line/branch coverage.

## Decisions (resolving the three open questions this RFC originally left)

1. **Findings evidence-gathering lives in `docs-gen`, LLM orchestration in `commands/docs.rs`** —
   not in `recovery`. It's a rendering-time concern with no ledger persistence needed, so it follows
   `enrich_with_prose`'s existing pattern (ephemeral, CLI-layer only) rather than
   `llm_description.rs`'s compile-time, store-writing one.
2. **One `--layout solution-architect` bundle, not separate `--sections` flags.** Reuses the
   existing `--prose`/`--yes` flags rather than adding new ones — the two deterministic pages always
   render; the findings memo's LLM-prioritized executive summary is gated by `--prose` exactly like
   today's per-object prose, falling back to the deterministic (unprioritized) findings list alone
   when `--prose` is omitted.
3. **No speculative `vulnerabilities` field reserved.** The Risk Report states plainly that
   CVE/license data isn't available yet — YAGNI until a feed connector RFC actually exists.

## Verification

131 `ekos-docs-gen` unit tests pass (18 new: empty-ledger honesty, declared/undeclared crate
versions, npm `version_spec`/`dev_dependency` rendering, fan-in ranking, repository-layout listing,
pipeline/rollup link-through, `ArchitectureGap`/versionless-crate/doc-coverage finding extraction,
deterministic-vs-prose-layered memo rendering); 5 new `crates/cli` tests (three-file output, HTML
rejection, `enrich_findings_memo` success via `MockLlmProvider`, and a no-credentials `--prose`
failure test mirroring `--layout objects`'s own). Full workspace gate clean: `cargo fmt`, `cargo
build --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (all green),
`tests/integration` (3/3).

Live-verified against this repo's own real, already-committed ledger (`ekos docs generate --layout
solution-architect`, run from the repo root against the existing `.ekos/` workspace, no rebuild
needed): real crate names and per-crate `version.workspace = true` "not declared" status in
`DependencyRiskReport.md`; real dependency fan-in ranking (`serde_json` 132 dependents, `thiserror`
113, `tokio` 96); real `Crate.path` → repository-layout table and a real "largest subsystem" pick in
`OnboardingGuide.md`; a real, honestly-surfaced finding in `FindingsMemo.md` (`1625/1625 RustSymbol`
and `584/584 RustModule` objects with no captured `description`) — cross-checked against this
repo's own already-generated `doc/entities/rustsymbol/re/render-readme.md`, which confirms the real
compiled object genuinely has no `description` property despite the real source having a `///` doc
comment, i.e. this ledger snapshot predates (or wasn't recommitted since) RFC 0087's doc-comment
capture — an honest, real, orthogonal finding this memo correctly surfaces rather than a bug in the
new code. The `--prose` path (`enrich_findings_memo`) was verified via `MockLlmProvider` and the
no-credentials error path only, not a real network call — no `ANTHROPIC_API_KEY` configured in the
environment this was implemented in, matching the existing `--layout objects --prose` test coverage
convention (`generate_with_prose_errors_clearly_instead_of_silently_degrading`).

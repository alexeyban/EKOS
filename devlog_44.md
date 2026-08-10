# Devlog 44 — RFC 0044: hierarchical knowledge rollups, a multi-project id-collision fix, and a real pipeline-placement bug caught before merge

**Date:** 2026-08-10
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

The user asked a strategic question: Claude can reverse-engineer a codebase into documentation,
but hits its own context-window ceiling on extra-huge projects and on many projects at once — how
should EKOS help close that gap? Two research passes established that EKOS's existing ledger+MCP
architecture is genuinely context-efficient today, but purely by **retrieval limiting** (capped
search results, hop-bounded graph walks) — there was no **summarization** layer anywhere that
compresses many raw facts into one higher-level fact. RFC 0044 closes that gap with a deterministic,
zero-LLM hierarchical rollup pass, after fixing a real prerequisite bug the same research surfaced:
multi-project workspaces could silently merge two unrelated projects' same-named files into one
`KirObject`, since file ids were hashed from a project-relative path with no project qualifier.
The rollup pass itself was designed, implemented, and unit-tested — then a full pipeline run
against this repo's own ~4,500-object ledger found it produced **zero** real rollups, because it
was wired into the wrong pipeline stage. Caught and fixed before merge, not after.

---

## RFC 0044 — Hierarchical Knowledge Rollups

### Problem / motivation

Investigated directly rather than assumed: `AiRuntime::gather_context` (`runtime/src/ai.rs`) caps
search results to 3 seed matches with 1-hop neighborhood expansion; MCP tools cap individually
(`ekos_search` at 50, `ekos_diff` at 200, `ekos_impact` at 5 hops); incremental build caching skips
whole-tree/whole-pass rescans when inputs are unchanged. All real, all context-efficient — but
none of it summarizes. An agent asking about a genuinely huge subsystem still has to personally
synthesize meaning from dozens-to-hundreds of raw objects within its own context. Searched
`identity`/`runtime`/`semantic` for `summar`/`rollup`/`hierarch` — zero hits on ledger objects.

The same investigation (into whether cross-project linking already existed — RFC 0029's identity
resolution links cross-*system* duplicates within one ledger, not cross-*project* objects) found a
real correctness bug: `plugins/file/src/lib.rs` computes each file's path relative to
`ctx.workspace_root` (one specific `[observe] paths` entry), and `build.rs` hashed that bare
relative path into a `KirId` with no project component. In a multi-project estate (the real
`~/PycharmProjects/ekos.toml`, ~40 observe paths, one shared ledger — proven in practice, not
hypothetical), two unrelated projects that each happen to have e.g. `src/main.rs` at the same
relative path silently collided into one merged object.

### What was built

| Component | Location |
|---|---|
| Object-identity fix | `ekos/crates/cli/src/commands/build.rs` |
| Rollup synthesis | `ekos/crates/semantic/src/rollup.rs` (new) |
| Rollup pipeline wiring | `ekos/crates/cli/src/commands/commit.rs` |
| Curated docs integration | `ekos/crates/docs-gen/src/lib.rs` (`## Subsystems`, `is_entity_page_kind`) |
| RFC | `ekos/docs/rfcs/0044-hierarchical-knowledge-rollups.md` (new) |

**Object identity**: `build.rs` now computes a `project_key` per `[observe] paths` entry (its own
path relative to `cwd`, **empty for the single-path case** — the overwhelmingly common setup keeps
byte-identical ids, no migration). When non-empty, it's folded into the id-hash input for every
`File` object/evidence id and stamped as a new `"project"` property. Confirmed by direct code
reading that this session's own new recovery passes (`crate_topology_analyzer`, `cicd_analyzer`,
the SQL/dependency-scan blocks) were *not* affected — they already derive paths relative to `cwd`,
not to the per-project `base`. Confirmed — but explicitly **not fixed this pass** — that the
identical risk exists in `github_analyzer.rs`, `local_docs_analyzer.rs`, `rust_analyzer.rs`/
`python_analyzer.rs`'s own artifact-embedded path fields, and git's `CoupledWith` file pairs;
tracked in `TODO.md`, not silently dropped.

**Rollup synthesis** (`ekos_semantic::rollup::synthesize_rollups`): deterministic, zero-LLM,
matching the same "structural first" pattern as every prior analyzer in this codebase. Groups
`File` objects by `"project"` property (when present — the coarser, more meaningful boundary in a
multi-project estate) or by directory-prefix-at-depth-3 (tuned for Cargo-workspace crate
granularity) otherwise. A group needs ≥2 members to become a `Rollup`, and there must be ≥2 groups
overall (a whole-graph single group isn't a useful summary of itself). Each `Rollup` carries real
`member_count`, `components` (member kind breakdown), and `boundary_relationships` (relationship-
kind counts for edges crossing the group boundary — the actual "what does this subsystem depend on"
signal), linked to every member via the *existing* `Contains` relationship — no new relationship
kind, so `ekos_search`/`ekos_neighborhood`/EKL understand rollups with zero new query-surface code.

**Curated docs**: `Architecture.md` gained a `## Subsystems` section (RFC 0042's existing
"link counts to detail" pattern, extended); `Rollup` added to `is_entity_page_kind` so curated
generation writes a real detail page per subsystem alongside the other entity kinds.

### A real bug found before merge, not after

The rollup pass was originally wired into `SemanticCompilerPass::run` (`ekos compile`), immediately
after identity resolution — seemed like the natural spot: post-merge, pre-CKM, matching the plan's
own stated design. Every unit test passed (constructing `File`-bearing `KirGraph` fixtures by hand).
Running the *full* pipeline against this repo's own ~4,500-object ledger for real-data verification
(a step every prior RFC this session insisted on) found **zero** `Rollup` objects in the output.

Root cause: `File` objects are written straight to the ledger by `ekos build`
(`build.rs:188-260`, `ledger.append_object` called directly) — they never pass through a
`KnowledgeArtifact` in the artifact store. `SemanticCompilerPass` only reads `KnowledgeArtifact`s
(recovery-pass output). Its `combined`/`resolved` graph therefore *never contains a single `File`
object*, on any real workspace — the one kind rollups group by directly. The unit tests never
caught this because they hand-built `KirGraph`s with `File` objects already in them, bypassing the
real data-flow boundary entirely.

Fixed by moving rollup synthesis to `ekos commit` (`commit.rs::commit_rollups`) — the first point
in the pipeline where `File` objects (written earlier by `build`) and CKM-derived objects (just
written by that same `commit` invocation) coexist in one place: read the ledger's current full
object/relationship set, run `synthesize_rollups` against an in-memory `KirGraph` built from it,
append only the newly-produced `Rollup` objects/relationships/evidence. Verified for real
afterward: `ekos commit` against this repo's own ledger produced **46 real subsystem rollups** —
one per crate/plugin directory (`ekos/crates/kir`, `ekos/crates/recovery`, …), each with accurate
member counts and boundary-relationship data (e.g. `ekos/crates/recovery`: 23 files, 246 `Contains`/
55 `CoupledWith`/332 `DependsOn` edges crossing its boundary).

### Decisions (alternatives considered, why this choice)

- **Fixing the placement in `commit.rs` rather than restructuring `SemanticCompilerPass` to also
  read ledger-committed `File` objects** — `commit.rs` already has ledger read/write access and is
  the natural first point where both object families coexist; teaching `SemanticCompilerPass`
  (which only knows about the artifact store) to also read the ledger would blur its single
  responsibility for no benefit.
- **Reusing `Contains` instead of a new relationship kind for rollup membership** — `Contains`
  already means exactly this everywhere else (`File`→`RustSymbol`, now `Rollup`→`File`); reusing it
  means every existing query surface understands rollups immediately, zero new code.
- **Deferring opt-in LLM prose per rollup** — the structural layer (real counts, real boundary
  relationships, real `Contains` links) ships first and is immediately useful on its own; prose
  synthesis (mirroring `docs-gen`'s `--prose`) is designed the same reuse-not-reinvent way but
  implemented as a follow-up once the structural layer is proven against real data — which it now
  is, across 46 real subsystems in this exact repo.

---

## Knowledge Captured

- **Unit tests that hand-construct a `KirGraph` fixture can pass while the pipeline wiring around
  them is completely broken.** The rollup pass's own tests were all green throughout — they never
  exercised the real question of *which pipeline stage actually has `File` objects available*.
  The only thing that caught it was running the real `build → recover → resolve → compile → commit`
  pipeline against a real ledger and checking the actual output count, exactly as CLAUDE.md's
  mandatory workflow and every prior RFC's "Investigated directly" section already insist on. This
  session's own devlog_41 recorded the same lesson once already (a `DefaultResolver` bug only found
  by running against this repo's own ~50-crate workspace) — worth restating as a pattern, not a
  one-off: *unit tests validate the function; only a real end-to-end run validates that the
  function is ever actually called with real data.*
- **`File` objects have a categorically different data-flow path than every other `KirObject` kind
  in this pipeline.** Every recovery-pass-derived object (`RustSymbol`, `Table`, `TransformNode`,
  …) flows `ekos recover` → `KnowledgeArtifact` in the artifact store → `ekos compile`'s
  `SemanticCompilerPass` → CKM → `ekos commit` → ledger. `File` objects skip straight from `ekos
  build` to the ledger, bypassing the artifact-store/CKM stage entirely. Any future pass that wants
  to operate on *both* families (this rollup pass, and likely anything else that needs file-level
  structure alongside recovered domain knowledge) must run at or after `ekos commit`, not inside
  `ekos compile`.
- **`ScanContext.workspace_root`-relative path derivation is the default convention across every
  `Observer` plugin**, deliberately (so a single-project run's paths stay clean) — but it means the
  identical multi-project id-collision risk this session fixed for `File` objects is latent in
  every other plugin/analyzer that derives an id the same way. Confirmed exactly which ones
  (`github_analyzer.rs`, `local_docs_analyzer.rs`, `rust_analyzer.rs`/`python_analyzer.rs`,
  `git_analyzer.rs`'s `CoupledWith` pairs) via direct grep + read, not assumed — recorded as an
  explicit, scoped follow-up rather than either silently ignored or over-fixed in one pass.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0044-hierarchical-knowledge-rollups.md` | New RFC, corrected mid-session after the placement bug was found |
| `ekos/crates/cli/src/commands/build.rs` | `project_key` computation + id-hash qualification + new `"project"` property |
| `ekos/crates/cli/tests/skeleton.rs` | 2 new regression tests: cross-project collision fixed, single-project case unaffected |
| `ekos/crates/semantic/src/rollup.rs` | New: `synthesize_rollups`, `group_key_for`, 5 unit tests |
| `ekos/crates/semantic/src/lib.rs` | `+pub mod rollup;` (not called from `SemanticCompilerPass` — see above) |
| `ekos/crates/cli/src/commands/commit.rs` | `+commit_rollups`, the real integration point |
| `ekos/crates/docs-gen/src/lib.rs` | `## Subsystems` section in `Architecture.md`; `Rollup` added to `is_entity_page_kind`; 1 new test |
| `TODO.md` | New "Multi-project/estate-scale follow-ups" item, done + deferred sub-items |
| `doc/**` | Regenerated against this repo's own ledger — 46 real subsystem rollups |

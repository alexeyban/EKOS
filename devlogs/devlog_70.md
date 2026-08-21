# Devlog 70 — RFC 0065 Phase 1: Architecture Knowledge Model, integrated into existing KIR

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Filed three externally-authored proposals (RFC 0064/0065/0066) into the repo's real RFC sequence
last session. Asked to "start implementation," but even RFC 0065's own "MVP" is realistically weeks
of work, and the three RFCs propose a large parallel Evidence/Entity/Relationship/Claim knowledge
model that substantially overlaps with what CLAUDE.md already states as EKOS's canonical model.
Resolved both questions with the user before writing code: integrate with the existing KIR/compiler
pipeline rather than build a parallel subsystem, and ship only "knowledge model + one deterministic
extractor + a C4 view" this session — no LLM reasoning, no evaluator, no agent state machine.
Delivered that slice, then found mid-implementation that a chunk of the planned "C4 view" already
existed in `docs-gen`, and cut it rather than duplicating it.

---

## RFC 0065 Phase 1 — Architecture Knowledge Model integration

### Problem / motivation

RFC 0065's Evidence/Entity/Relationship model (§8-19) is — field for field — close to what
CLAUDE.md already states: "every semantic conclusion must be traceable to Evidence, one of the four
primitives: Object, Relationship, Event, Evidence." RFC 0065 §60 ("Plugin Architecture") proposes a
new `ekos-architecture` crate with its own types/storage/state machine, independent of KIR/CKM/
ledger. Building that literally would duplicate real, working infrastructure and forfeit ledger/
Runtime/MCP integration for free. CLAUDE.md's own Mandatory Development Workflow requires an
"Architecture Review — validate against the compiler model" step before any implementation; doing
that review here changed the shape of the work before any code was written.

### What was built

| Component | What it does |
|---|---|
| `MergeProposal`-adjacent kind exclusions (`crates/identity/src/lib.rs`) | `"Claim"`/`"ArchitectureGap"` added to `DefaultResolver`'s blanket kind-exclusion list *before* any real over-merge was observed |
| `crate_topology_analyzer.rs` extensions | Emits a Fact-type `Claim` per `DependsOn` edge it already derives; a previously silent `DepResolution::Unresolved` case now becomes an `ArchitectureGap` |
| `render_architecture` extensions (`docs-gen`) | C4 mapping note on the existing Crate/Technology sections; new `## Open Questions` section for `ArchitectureGap` objects |

### Implementation details worth remembering

**Two decisions asked up front, not assumed.** Given the scale (RFC 0065 alone is 74 numbered
sections), diving straight into code risked days of misdirected work. `AskUserQuestion` resolved:
(1) integrate with KIR vs. build RFC 0065 §60's literal parallel subsystem — confirmed: integrate;
(2) what "start implementation" actually means as a first slice, given even the RFC's own MVP list
spans an entity/evidence model, local LLM reasoning, C4 views, markdown generation, an evaluator,
and a feedback loop — confirmed: knowledge model + one deterministic extractor + a C4 view only.

**Found the best-fit extractor by reading the codebase, not by picking from the two examples
floated in the question.** The AskUserQuestion options suggested `rust_analyzer` or `git_analyzer`
as examples; investigating further found `CrateTopologyAnalyzerPass` (RFC 0042) a better fit — it
already parses real `Cargo.toml`s into `Custom("Crate")` objects with real `DependsOn` edges, and
in a Rust workspace a crate genuinely *is* the C4 Container-level unit (RFC 0065 §23's own mapping:
service/application → Container). Examples in a clarifying question are illustrations of the shape
of an answer, not an exhaustive menu — worth remembering when the actual best fit turns out to be
something else entirely.

**A silent `continue` in existing code turned out to be exactly the RFC's own "Unknown" concept.**
`crate_topology_analyzer.rs` already had `DepResolution::Unresolved => None => continue` — a
`{ workspace = true }` entry with no matching root `[workspace.dependencies]` key (or an unmodeled
dependency shape) was silently dropped. RFC 0065 §17 defines exactly this: "Unknowns are explicit
knowledge gaps... not errors... a useful output of architecture discovery." Unlike `Assumption`/
`Inference` (which need interpretive judgment — out of scope without the reasoning layer), an
`ArchitectureGap` here is honestly, deterministically producible: EKOS already knows it couldn't
resolve something, it just wasn't saying so. Fixed to match this project's own established
"Unmapped is deliberate, not a gap swept under the rug" philosophy (Transformation IR, RFC 0027) —
the same principle, rediscovered in a different corner of the codebase.

**Claim naming collision, caught before it happened.** RFC 0065's "Unknown" concept could not
literally become `ObjectKind::Custom("Unknown")` — `ObjectKind::Unknown` already exists as a
built-in fallback variant, and both would render identical `Display` text ("Unknown"), making them
indistinguishable in any query or CLI output. Named the new kind `ArchitectureGap` instead. Checked
existing usages of `ObjectKind::Unknown` first (`grep`, not guesswork) — confirmed they're generic
test-fixture defaults, not an established "gap" semantic already in conflict.

**Proactive kind-exclusion, not reactive.** This session's own earlier RFC 0063 work fixed the
*consequence* of not adding a new self-identified `Custom(_)` kind to `DefaultResolver`'s exclusion
list — six different kinds have now hit the identical over-merge failure shape (`Section`,
`TransformNode`, `RustSymbol`/`RustModule`, `PythonSymbol`/`PythonModule`, `Crate`, and now `Claim`/
`ArchitectureGap`), each discovered live rather than by inspection. Added `Claim`/`ArchitectureGap`
to the list immediately, before any real over-merge occurred — cheaper than rediscovering it a
seventh time.

**Mid-implementation scope cut: found a chunk of the plan already existed.** The plan called for a
new `render_c4_container_view` Mermaid diagram function. Reading `docs-gen::render_architecture`
before writing it found an existing "## Crate & Workspace Topology" section (a real Mermaid graph
of `Custom("Crate")` objects and their `DependsOn` edges, via `render_relationship_kind_graph`) and
an existing "## Technologies" section (external dependencies + their dependents) — together already
covering almost exactly what the planned new function would have rendered, plus a "## Dependency
Graph" section further down that renders every relationship kind including `DependsOn` in full.
Building a separate, redundant diagram would have violated "avoid proposing new code when suitable
implementations already exist." Cut the new render function; added a one-paragraph C4-mapping note
to the existing section instead (crate → Container, technology → External System, RFC 0065 §23) and
kept the actually-novel piece — the `## Open Questions` section, which genuinely didn't exist
anywhere before.

### Decisions (alternatives considered, why this choice)

- **New `ekos-architecture` crate with its own types** (RFC's literal proposal) — rejected per the
  user's confirmed choice: reuse KIR/CompilerPass/docs-gen, get ledger/Runtime/MCP integration for
  free instead of rebuilding it.
- **A standalone C4 Container Mermaid diagram function** — rejected mid-implementation once the
  existing Crate/Technology sections were found to already cover the same ground; a short
  C4-mapping annotation was the honest, non-duplicative version of the same deliverable.
- **Populating `Assumption`/`Inference`-type claims now** — rejected: those require interpretive
  judgment (the reasoning layer, explicitly deferred), and a deterministic pass fabricating them
  would be exactly the kind of unsupported-precision RFC 0065 §4.6 itself warns against.

---

## Live verification against real data

Ran the actual `build → recover → resolve → compile → commit → docs generate` pipeline against
this repo's own real, self-hosted `.ekos/` workspace (`/home/legion/PycharmProjects/EKOS/.ekos`) —
backed it up to scratch first (`cp -a`, non-destructive; `.ekos/` is gitignored, not otherwise
recoverable). Had to move `.ekos/artifacts/pass-manifests/` aside (not delete — `mv`, matching an
already-established pattern from this session's own RFC 0063 verification, and finding
pre-existing `pass-manifests.bak-pre-identity-fix`-style directories confirming the same trick was
used before) to force `crate-topology-analyzer` and `semantic-compiler` to actually re-run against
the new code rather than serve a stale cached result — same Phase 13 caching subtlety devlog_67
already documented.

Real results: `crate-topology-analyzer` found 44 crates, 45 technologies, 441 edges in this
repo's own workspace. `ekos compile`'s identity-resolution log line confirmed RFC 0063's own
mechanism working live end to end together with this session's new kinds: `proposals=66
auto_merged=49 sent_to_review=17 conflicts=2` (the 2 conflicts — `observeerror`/`main` naming
collisions across Rust/Python symbols — are pre-existing and unrelated to this work).
`ekos commit` took long enough against this repo's real ~35k-object corpus to need running in the
background rather than foreground (a known, already-tracked perf characteristic — see TODO.md's
`all_objects()` item, not something this session's changes made worse).

---

## Knowledge Captured

- **When a proposal's own scope is genuinely too large for one session, ask two narrow questions
  instead of guessing at scope reduction.** "Integrate or build parallel" and "what's the actual
  first slice" were both real decisions with several defensible answers; getting them wrong would
  have meant redoing real, non-trivial work.
- **Read the target file before writing new code into it, even mid-implementation, even after a
  plan was already approved.** The plan's own `render_c4_container_view` design was reasonable
  *without* having read `render_architecture`'s full body; reading it before writing found enough
  overlap to change the approach. A plan is a direction, not a contract to execute blindly once a
  closer look at the code changes the picture.
- **A `continue` that silently drops a case is worth grep-ing for when a new "gap"/"unknown"
  concept is being added** — this repo already had a real, unlabeled instance of exactly that
  concept sitting in existing code, found by reading the function being extended rather than
  writing new gap-detection logic from scratch.
- **`ObjectKind::Unknown`'s existing generic-fallback meaning made `Custom("Unknown")` an unsafe
  name for a new, more specific concept** — `Display` text collisions between a built-in variant
  and a `Custom(String)` payload are a real, checkable risk (`grep` first) whenever a new `Custom`
  kind's name is chosen.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0064-architecture-knowledge-model.md`, `0065-...-v2.md`, `0066-...md` | Filed from three externally-authored proposals into the repo's real RFC sequence (prior session) |
| `ekos/docs/rfcs/0065-architecture-knowledge-model-v2.md` | New dated "Phase 1 implemented" section |
| `ekos/crates/identity/src/lib.rs` | `"Claim"`/`"ArchitectureGap"` added to `DefaultResolver`'s kind-exclusion list; 2 new tests |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | Emits `Claim` per `DependsOn` edge; `DepResolution::Unresolved` now emits `ArchitectureGap` instead of a silent skip; 3 new tests |
| `ekos/crates/docs-gen/src/lib.rs` | C4 mapping note + new `## Open Questions` section in `render_architecture`; 2 new tests |
| `TODO.md` | New backlog item for the deferred reasoning layer/evaluator/RFC 0066 work |
| `devlogs/devlog_70.md` | This file |

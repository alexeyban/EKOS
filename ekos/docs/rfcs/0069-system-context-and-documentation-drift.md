# RFC 0069 — System Context View + Real Documentation Drift

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0068 (filed) specifies a 67-section target documentation package, with explicit instruction
that none of it is to be permanently cut — TODO.md carries the full roadmap, grouped by RFC 0068's
own MVP/Phase 2/Phase 3 sequencing. This RFC is **Increment 1** of continuous, automatic build-out
against that roadmap: the concrete slice identified as closest to existing data and lowest risk —
System Context (RFC 0068 §15, one C4 level above the Container view RFC 0065 Phase 1 already
shipped) and real Documentation Drift detection (RFC 0068 §31-32).

## Design

### System Context (RFC 0068 §15)

New `## System Context` section in `docs-gen::render_architecture`, positioned first (broadest
view first, narrowing down through the page). The whole compiled workspace collapsed to one
"System" node, with an edge to every `Custom("Technology")` object that at least one compiled
`Custom("Crate")` has a real `DependsOn` edge to — not every `Technology` object that happens to
exist, only ones with an actual compiled dependency. Reuses the exact Mermaid pattern (
`mermaid_node_id`/`mermaid_escape_label`) every other diagram in this file already uses. Zero new
extraction: built entirely from `Crate`/`Technology`/`DependsOn` data RFC 0042 already compiles.

### Real documentation drift (RFC 0068 §31-32)

Investigated before designing anything new: the real primitive already existed, unused.
`KnowledgeStore::object_history(id) -> Vec<KirObject>` (`crates/ledger/src/lib.rs`) returns every
version of an object, oldest to newest. `append_object`'s `(id, content_signature)` versioning
(RFC 0015, confirmed by reading `append_versioned`) deduplicates identical content — a version is
only appended when content genuinely changes. A role `Claim`'s id is deterministic per crate
(`role_claim_kir_id`, RFC 0067). So if `architecture-reasoning` is ever re-run and classifies the
same crate differently, `object_history` already captures both values with zero new storage work
— this *is* RFC 0068's own drift definition ("a discrepancy between documented architecture and
architecture supported by current evidence"), not a separately modeled concept.

`crates/recovery/src/architecture_drift.rs::drift_from_history(subject_name, subject_id, history)`
is a pure function (no ledger dependency) comparing history's first and last `properties["value"]`
— kept pure and ledger-free deliberately: `recovery`-crate passes have never read the ledger
(only ever produced KIR flowing forward through compile→commit), and adding a new `recovery` →
`ledger` dependency edge would be a real, unusual architectural change for one function. Instead
`ekos architecture investigate` (which already opens the store via `open_store`) fetches each
crate's role-claim history and calls this pure function — `cli` does the I/O, `recovery` does the
comparison.

Findings are reported separately from `EvaluationReport.issues` in `ekos architecture
investigate`'s final report, in RFC 0068 §32's own "DOCUMENTATION DRIFT DETECTED" human-readable
shape — not folded into the numeric quality score. Deliberate: drift is a genuinely different
signal (staleness) from completeness/evidence-coverage, and there's no real weight-calibration
data yet for how much one drift finding should move a composite score — inventing a weight would
be exactly the "unsupported precision" this project's own RFCs (0065 §4.6) already warn against.

## Non-goals

- No Basic Component View, no dedicated Technology Inventory page — the next queued increment
  (needs a real design decision first: `Crate` and `File`/`RustSymbol` objects aren't currently
  linked in the graph at all).
- No drift detection for `Claim`(fact-type)/`ArchitectureGap` objects — no real scenario yet where
  their content legitimately changes version-over-version the way a role classification can.
- No continuous/scheduled drift checking (RFC 0068 §56, explicitly Phase 3) — one-shot, computed
  each time `evaluate_architecture`/`ekos architecture investigate` runs.

## Testing

- `docs-gen`: real Crate/Technology/DependsOn fixtures render the expected System Context nodes/
  edges; a Technology with no real dependency edge is correctly excluded; empty input renders the
  file's existing honest-placeholder convention.
- `architecture_drift.rs`: two different versions → a finding; identical repeated versions → none
  (proves the dedup-at-append behavior this feature relies on); single/empty history → none;
  three versions where the net change is zero → none (compares oldest-to-newest endpoints, not
  adjacent pairs); missing/malformed `value` property → none, not a panic.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real data — no new pipeline run needed**: this repo's own real, already-committed ledger
  (from this session's earlier RFC 0067 work, which ran `architecture-reasoning` more than once
  with genuinely different results for several crates) already contained real drift. A one-off,
  removed-after-use test called `detect_drift` directly against the real `.ekos/` store: **7 real
  findings** — e.g. `ekos-kir: shared utility -> core library`, `ekos-runtime: core library ->
  plugin/connector`. `ekos docs generate --layout curated` against the same real ledger renders a
  real `## System Context` section with real crate dependency names (anyhow, axum, clap, chrono,
  ...) as System→Technology edges.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0069-system-context-and-documentation-drift.md` | This RFC |
| `ekos/crates/docs-gen/src/lib.rs` | New `## System Context` section + `render_system_context`; 3 new/updated tests |
| `ekos/crates/recovery/src/architecture_drift.rs` | New: `DriftFinding`, `drift_from_history`; 6 tests |
| `ekos/crates/recovery/src/architecture_reasoning.rs` | `role_claim_kir_id` visibility: `pub(crate)` → `pub` |
| `ekos/crates/recovery/src/lib.rs` | Export `architecture_drift`'s public items, `role_claim_kir_id` |
| `ekos/crates/cli/src/commands/architecture.rs` | `detect_drift`, wired into the final report |

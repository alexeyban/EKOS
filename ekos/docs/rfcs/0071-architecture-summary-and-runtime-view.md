# RFC 0071 — Architecture Summary + Runtime View

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

**Increment 3** of RFC 0068's continuous build-out (Increment 1: RFC 0069, System Context +
documentation drift; Increment 2: RFC 0070, Component View + Technology Inventory). This
increment: Architecture Summary / Executive Overview (§14) and a Basic Runtime View (§20) — the
two remaining RFC 0068 §61 MVP view items closest to existing data.

## Design

### Architecture Summary (§14)

RFC 0068's own template names seven fields (System, Purpose, Architecture style, Primary
technologies, Major external systems, Major risks, Architecture confidence). Rather than fabricate
answers for fields with no real EKOS source, `render_architecture_summary` populates only what's
backed by real compiled evidence — component/crate counts (`count_by_kind`, this file's own
established pattern), the technologies with the most real compiled dependents, and the Open
Questions count (RFC 0065 §17) — and states explicitly, per field, why the rest aren't computed:

- `Purpose`/`Architecture style` need either an LLM read of real project intent or human input;
  this is a zero-LLM deterministic renderer.
- `Major risks` needs a `Risk` KIR kind that doesn't exist yet (RFC 0068 §62 Phase 2).
- `Architecture confidence` needs `evaluate_architecture` (RFC 0065 Phase 3) wired through the same
  way RFC 0069 wired drift through from `cli` — not done for the plain `docs generate` path (only
  `ekos architecture investigate` computes a real evaluation score today).

This matches RFC 0068 §4.6's own "no unsupported precision" principle and this project's
consistent practice throughout every prior increment: an honest "_not yet computed_" is not a
missing feature to apologize for, it's what distinguishes this project's documentation from
generated prose that looks confident regardless of whether it's grounded.

### Basic Runtime View (§20)

`SequenceDiagrams.md` (RFC 0041's `Calls` graph, RFC 0027's Transformation IR) already renders
every real compiled call/data-flow sequence — confirmed before writing anything new. RFC 0068 §20
itself frames Runtime Architecture around *named business scenarios* ("Create Order", "Process
Payment"), which requires identifying which sequences matter — a judgment call needing either LLM
reasoning about intent or human curation, neither available to a deterministic renderer. Rather
than dump the same exhaustive per-symbol sequence data a second time under a new heading, the new
`## Runtime View` section links through to the already-generated page, with the scenario-naming
gap stated explicitly rather than invented.

## A repeat instance of RFC 0070's relationship-duplication bug, fixed the same way

Live-verifying Architecture Summary against this repo's own real ledger found the same
non-deterministic-relationship-id symptom RFC 0070 already diagnosed and partially fixed: raw
`DependsOn` counts read 132 "dependents" for `serde_json` (the real number is ~33-34 distinct
crates). Applied the identical fix in this new location — deduplicate by `(from, to)` pair before
counting — with its own regression test, rather than assume RFC 0070's fix in one view covered
every other place the same root cause could surface. The root cause itself remains tracked as its
own separate TODO.md item (not re-litigated here); this is the second of what may be several
per-view mitigations until the underlying ledger-layer fix is actually done.

## Testing

- `docs-gen`: Architecture Summary reports real counts and top technologies, explicit
  "not yet computed" text for the four unbacked fields; Runtime View links to `SequenceDiagrams.md`
  when call/flow edges exist, honest empty-state otherwise; a second duplicate-relationship-id
  regression test for this view's own dependent-counting logic.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real data — no new pipeline run needed** (third increment running on this pattern): `ekos
  docs generate --layout curated` against this repo's own real, already-committed ledger rendered a
  real Architecture Summary (44 crates, real top-5 technologies with correct dependent counts after
  the dedup fix, 0 open questions) and a real Runtime View linking to the real `SequenceDiagrams.md`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0071-architecture-summary-and-runtime-view.md` | This RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Increment 3 status note |
| `ekos/crates/docs-gen/src/lib.rs` | New `## Architecture Summary` + `render_architecture_summary`; new `## Runtime View`; dedup fix; 4 new tests |
| `TODO.md` | RFC 0068 §61 MVP items ticked off; next increment scoped |

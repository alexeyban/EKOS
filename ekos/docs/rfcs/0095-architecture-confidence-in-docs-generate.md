# RFC 0095 — Architecture confidence in `docs generate`'s Executive Summary

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-25
**Implemented:** 2026-08-25

---

## Motivation

Confirmed real gap, the last item on this session's gap-closure list and already named in-line at
the exact render site (`docs-gen/src/lib.rs`): `"**Architecture confidence:** _not yet computed
here — see \`ekos architecture investigate\`'s own evaluation report (RFC 0065 Phase 3) for a real
completeness/evidence-coverage score_"`. `evaluate_architecture` (`crates/recovery/src/
architecture_evaluator.rs`, RFC 0065 Phase 3) already exists, is already a real, deterministic,
evidence-grounded computation (no LLM), and is already called by `ekos architecture investigate`
— it was simply never called from the plain `ekos docs generate` path, so a user who only ever
runs `docs generate` (the overwhelmingly more common command) never sees it at all.

## Design

### Not a new computation — a wiring gap

`evaluate_architecture(objects: &[KirObject]) -> EvaluationReport` is a plain, pure function whose
only input (`objects`) is exactly what `docs.rs::generate_curated` already loads from the ledger
before calling `render_architecture`. No new pass, no new KIR kind, no new dependency —
`crates/cli` already depends on both `ekos-recovery` (`evaluate_architecture` itself) and
`ekos-docs-gen` (the renderer). The fix is calling one existing function at one existing call site
and threading its result one level deeper.

### The honest vacuous case

`evaluate_architecture`'s own two dimensions are Rust-crate-specific: `completeness` (fraction of
`Custom("Crate")` objects with a `has_role` classification) and `evidence_coverage` (fraction of
`Claim`/`ArchitectureGap` objects with real evidence). Both default to `1.0` when their input set
is empty (`crates_total == 0`, or no `Claim`/`ArchitectureGap` objects exist) — a real,
intentional vacuous-truth choice in the evaluator's own existing code, correct for the boolean
"did we fail to classify anything" question it was built to answer, but wrong to render as a
literal "100% architecture confidence" for a project with nothing to evaluate at all (`pdf-reader`:
no `Cargo.toml`, so zero `Crate`/`Claim`/`ArchitectureGap` objects ever exist for it — this
dimension is currently Rust-workspace-specific, an explicit, named limitation, not silently
misrepresented as universal).

`EvaluationReport` gains one new field, `evidenced_total: usize` (the real count
`evidence_coverage`'s denominator was computed from, previously discarded after use) — additive,
non-breaking, so a caller can tell "was there real signal behind this score" without re-deriving
`evaluate_architecture`'s own internal object scan. The Executive Summary line renders the real
score/breakdown only when `crates_total > 0 || evidenced_total > 0`; otherwise an honest
`"_not meaningfully computed — no Crate/Claim/ArchitectureGap objects exist for this project (this
dimension is Rust-workspace-specific today, RFC 0065 Phase 3 v1 scope)_"`, matching this whole
codebase's "absence over a fabricated placeholder" convention rather than a technically-true but
misleading 100%.

### Explicit non-goal: extending `evaluate_architecture` itself

Making `completeness`/`evidence_coverage` meaningful for non-Rust projects (e.g. a Python/JS
equivalent of "every significant module got an architectural role"), or adding RFC 0065 §34's
other named dimensions (`consistency`, `cross_view_consistency`, ...), is real, separate,
`architecture_evaluator.rs`-scoped work — not attempted here. This RFC is the wiring gap only.

## Verification

- `crates/recovery/src/architecture_evaluator.rs`: existing tests updated for the new
  `evidenced_total` field; one new test confirming it reports the real denominator.
- `crates/docs-gen/src/lib.rs`: tests for the populated case (real score/breakdown rendered) and
  the honest vacuous case (no `Crate`/`Claim`/`ArchitectureGap` objects at all).
- Full workspace gate (`fmt`/`build`/`clippy -D warnings`/`test --workspace`) clean, `tests/integration`
  3/3.
- Live-verified against two real, different-shaped targets: `pdf-reader` (no `Cargo.toml`) renders
  the honest vacuous-case message; EKOS's own real Rust workspace (a fresh scratch scope built from
  a subset of its real crates, the same self-verification technique this session's RFC 0079 fix
  used) renders a real, non-trivial score with a real completeness/evidence-coverage breakdown.

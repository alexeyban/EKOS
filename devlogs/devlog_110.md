# Devlog 110 — RFC 0094: `Custom("Risk")` KIR kind, Observed Concentration Risk v1

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Seventh item on the gap-closure list: `Architecture.md`'s Executive Summary has said `"**Major
risks:** _not yet computed — no Risk KIR kind exists yet (RFC 0068 §29/§62)_"` since the section
was first written. Filed and implemented RFC 0094: a new `Custom("Risk")` KIR kind, one
deterministically-derived v1 rule (Observed Concentration Risk — an object with 3+ real compiled
`DependsOn` dependents), computed inside `SemanticCompilerPass` and wired into the Executive
Summary. Live-verified against `pdf-reader`'s real ledger: 11 real `Risk` objects, real evidence,
real `Architecture.md` output — the placeholder text is gone.

## What was built

`ObjectKind::Custom("Risk")` (`risk_type`/`statement`/`dependent_count` properties, a real
`References` edge to the object it concerns). One rule, computed once per `compile` run inside
`crates/semantic/src/lib.rs`'s `SemanticCompilerPass::run()` (needs a whole-graph `DependsOn` view,
which only exists after identity resolution merges every recovery pass's output — the same reason
RFC 0044's rollups are deferred to `ekos commit`, except Concentration Risk only needs
`Technology`/`Crate`-ish objects already fully present in `resolved`, no `File`-object dependency
to wait for). Threshold: 3+ real dependents (an explicit, named floor — 1 or 2 doesn't yet
distinguish "genuinely broad usage" from "a couple of unrelated call sites"). Kind-agnostic, not
`Technology`-only — reused `DependencyRiskReport.md`'s existing render-time fan-in computation
(RFC 0090) as the starting point, but widened scope to any real `DependsOn` target, which turned
out to matter a lot in practice (see Live Verification).

`Architecture.md`'s Executive Summary replaces its placeholder line with the real compiled
statements when any `Risk` objects exist, and an honest "no concentration risk detected — no
object has 3 or more real compiled dependents yet" when none do — never silently omitted.

5 new tests in `crates/semantic` (below-threshold produces nothing, at-threshold produces exactly
one real `Risk` with correct properties/evidence/`References` edge, deterministic id across
separate runs, evidence capping), 2 new tests in `crates/docs-gen` (populated Executive Summary,
honest empty case).

## Live verification — better than planned

The RFC's own draft plan expected `pdf-reader` to only exercise the *negative* case (its
`Technology`-only fan-in tops out at 2 — `DependencyRiskReport.md`'s existing section already
showed this) and planned a separate scratch multi-crate EKOS scope for the positive case. Once the
rule was widened to be kind-agnostic (not `Technology`-only — see Design), `pdf-reader` turned out
to have real, meaningful concentration signal on its own: `fastapi.HTTPException`/
`fastapi.Depends`/`fastapi.APIRouter` (imported across multiple real FastAPI route files),
`sqlalchemy.orm.Session`, a shared `app.db.session.get_db` DB-session dependency-injection helper,
a shared frontend `../api/client` module — all real, architecturally meaningful single points of
broad reliance, not synthetic examples. `ekos compile` produced 11 real `Risk` objects (object
count 148 → 159, relationship count 192 → 203 — exactly 11 objects + 11 `References` edges).
`Architecture.md`'s real Executive Summary now reads:

> **Major risks:** 'react' has 6 real compiled dependent(s); 'sqlalchemy.orm.Session' has 6 real
> compiled dependent(s); 'pathlib.Path' has 5 real compiled dependent(s); ... _(Observed, RFC 0068
> §29/RFC 0094 — see each risk's own evidence)_

No scratch EKOS-self scope was needed — one real project's own data was sufficient once the rule
stopped artificially narrowing to `Technology`.

## A real, separate gap found while verifying, not fixed here

`ekos query object` on a real committed `Risk` object showed zero cited evidence, even though the
implementation correctly forwards whatever evidence the underlying `DependsOn` edges carry (unit-
tested and confirmed working with synthetic edges that do have evidence). Traced: `python_analyzer.rs`'s
`add_import` constructs its `DependsOn` relationships with `KirRelationship::new(...)` directly,
never attaching any evidence at all — unlike, say, `dependency_analyzer.rs`'s technology edges or
`crate_topology_analyzer.rs`'s claims, which do cite real evidence. This isn't a bug in RFC 0094's
own logic — a `Risk` object correctly has no evidence to cite when its underlying edges have none,
matching this codebase's "never fabricate evidence" principle — but it's a real, separate,
pre-existing gap in `python_analyzer.rs` specifically (its `DependsOn` edges are the one common
case with no evidence at all), left unfixed here (out of scope for this RFC) and noted in `TODO.md`
for a future session.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups). `tests/integration` 3/3.

## Knowledge Captured

- **Widening a rule's scope beyond the section it was first prototyped under (`Technology`-only,
  because that's the one section `DependencyRiskReport.md` happened to render it under) surfaced
  real signal a narrower version would have missed entirely** — `pdf-reader`'s most meaningful
  concentration risks (shared DI helpers, a shared API client module) aren't `Technology` objects
  at all; they're ordinary `PythonModule`/`JsModule` objects from real source-level imports. Worth
  checking, whenever reusing an existing render-time computation as the basis for a new persisted
  KIR object, whether the original computation's scope was a deliberate design choice or just an
  artifact of which section it happened to be written for first.
- **Live verification found a stronger result than the RFC's own draft plan predicted** — worth
  updating the RFC's own Verification section to say what actually happened rather than leaving the
  original (wrong, but not misleading-on-purpose) prediction standing once the real outcome differs.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0094-observed-risk-concentration.md` | New RFC, Accepted; Verification section corrected after live results turned out stronger than the draft plan |
| `ekos/crates/semantic/src/lib.rs` | New `concentration_risks`, `concentration_risk_kir_id`; wired into `SemanticCompilerPass::run()`; 5 new tests |
| `ekos/crates/docs-gen/src/lib.rs` | Executive Summary's `**Major risks:**` line now real, not a placeholder; 2 new tests |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh against the fix |

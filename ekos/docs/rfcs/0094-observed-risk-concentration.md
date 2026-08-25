# RFC 0094 — `Custom("Risk")` KIR kind: Observed Concentration Risk, v1

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-25
**Implemented:** 2026-08-25

---

## Motivation

Confirmed real gap, named explicitly in this session's gap-closure list and already flagged
in-line at `Architecture.md`'s own Executive Summary render site (`docs-gen/src/lib.rs:955`):
`"**Major risks:** _not yet computed — no Risk KIR kind exists yet (RFC 0068 §29/§62)_"`. RFC
0068 §29 already sketches the intended vocabulary — `Observed Risk` / `Inferred Risk` / `Potential
Risk` / `Recommendation` — but no `ObjectKind::Custom("Risk")` exists anywhere in the codebase, and
nothing produces one.

## Scope: `Observed Risk` only, one rule, v1

RFC 0068 §29's own vocabulary distinguishes what's mechanically *observed* in compiled structure
from what requires *inference* (a judgment call about what an observation implies) or is merely
*potential* (a hypothesis, not yet evidenced). Only `Observed Risk` is attempted here — the same
"facts should preferably come from deterministic extraction" principle RFC 0065's `Claim` kind
already established, and the same discipline every other RFC this session followed (RFC 0091/0092:
one real, narrowly-scoped, non-fabricated signal, not a whole taxonomy at once). `Inferred`/
`Potential` risk and `Recommendation` text are real, legitimate future extensions — they need
actual reasoning (LLM-assisted or a real heuristic beyond a threshold on already-compiled counts),
explicitly out of scope here.

The one v1 rule: **Concentration Risk** — an object with an unusually large number of real,
compiled `DependsOn` dependents is a single-point-of-failure candidate (RFC 0068 §29's own
"technical debt" list names "tight coupling"; a heavy-fan-in technology or crate is the structural
signature of exactly that). This reuses `DependencyRiskReport.md`'s existing `## Concentration
Risk` section's exact selection logic (`crates/docs-gen/src/lib.rs`, RFC 0090) — that section
already computes real fan-in counts and ranks by them at render time, with no persisted object;
this RFC promotes the same computation into real, persisted `Custom("Risk")` KIR objects so the
signal has a stable id, real evidence, and can be surfaced in `Architecture.md`'s Executive
Summary (the actual site the gap was reported against), not just `DependencyRiskReport.md`.

Kind-agnostic, not `Technology`-only: any object that's the real target of many `DependsOn` edges
qualifies (a `Crate` with many internal dependents is exactly as real a concentration risk as an
external `Technology` with many dependents) — `DependencyRiskReport.md`'s own render-time version
happened to filter to `Technology` only because that's the one section it's rendered under; the
persisted `Risk` object has no reason to inherit that narrower scope.

### Threshold: ≥3 real dependents, not a purely cosmetic top-N

`DependencyRiskReport.md`'s existing section shows "top 5 by count" unconditionally, including
objects with only 1 real dependent on a small project — fine for an always-shown ranked list, wrong
for a `Risk` object that's meant to represent something genuinely worth flagging. A minimum of 3
real dependents is the threshold for actually emitting a `Risk` object: 1 or 2 dependents doesn't
yet distinguish "genuinely broad, coupled usage" from "used by a couple of unrelated call sites" —
3 is the smallest count where "more than a pair" starts to mean something. Not a tuned/calibrated
number — an explicit, named, defensible floor, not a fabricated confidence score.

## Design

### `ObjectKind::Custom("Risk")`

Properties: `risk_type` (`"observed"` — the only value v1 ever emits, but the field exists now so
`"inferred"`/`"potential"` can be added later without a schema change), `statement` (the real,
data-derived sentence — e.g. `"'serde' has 12 real dependents"`), `dependent_count`. Evidence: the
real `DependsOn` relationships themselves (up to a small cap, to avoid an unbounded evidence list
on a very widely-used object) — never a fabricated severity judgment, only the count and the real
edges that produced it.

Id: deterministic, keyed by the target object's own id (`risk:concentration:{target_id}`) — a
concentration risk is a property of one specific real object, re-derived identically on every
`compile` run, matching every other deterministic-id precedent this session's fixes established
(RFC 0070/0071's lesson: a non-deterministic relationship/object id causes unbounded duplicate
accumulation across repeated `recover`/`compile` runs).

### Where it's computed: `crates/semantic`, not a new `recovery` pass

Concentration risk needs a whole-graph view — every `DependsOn` edge across every recovery pass's
output, merged — which only exists after identity resolution, inside `SemanticCompilerPass::run()`
(`crates/semantic/src/lib.rs`), the same place `CkModel` itself gets built. Unlike RFC 0044's
rollups (deferred to `ekos commit` specifically because they need `File` objects, which live only
in the ledger, never in `combined`/`resolved`), Concentration Risk only needs `Technology`/`Crate`-
kind objects and `DependsOn` edges — both already fully present in `resolved` at this point in the
pipeline. No new dependency, no new pass registration.

### `docs-gen`: Executive Summary wiring

`Architecture.md`'s Executive Summary (`docs-gen/src/lib.rs`, the exact site RFC 0068 §17 names)
replaces its placeholder `"**Major risks:** _not yet computed..._"` line with a real listing of
compiled `Custom("Risk")` objects when any exist, `"_No concentration risk detected — no object
has 3 or more real compiled dependents yet._"` when none do (never silently omitted — the honest
absence is itself real information, matching this whole codebase's "absence over a fabricated
placeholder" convention already used for every other opt-in/conditional section).

## Verification

- `crates/semantic/src/lib.rs`: unit tests for the concentration-risk derivation function —
  below-threshold objects produce nothing, an at-threshold object produces exactly one `Risk`,
  the risk id is deterministic across separate runs, evidence caps correctly.
- `crates/docs-gen/src/lib.rs`: a test for the Executive Summary's populated case and its honest
  empty case.
- Full workspace gate (`fmt`/`build`/`clippy -D warnings`/`test --workspace`) clean, `tests/integration`
  3/3.
- Live-verified against `pdf-reader`'s real whole-project ledger — one target sufficed for the
  positive case once the rule was made kind-agnostic (§"Design"): `Technology`-only fan-in tops out
  at 2 in this project (`DependencyRiskReport.md`'s own existing section already showed this), but
  real `PythonModule`/`JsModule` fan-in is genuinely higher — `fastapi.HTTPException`,
  `sqlalchemy.orm.Session`, a shared `app.db.session.get_db` DB-session helper, a shared frontend
  `../api/client` module, all real, meaningfully-concentrated dependencies once considered.
  `ekos compile` produced 11 real `Risk` objects; `Architecture.md`'s Executive Summary renders
  the real `**Major risks:**` line with all 11 real statements, `see each risk's own evidence`.
  `ekos query neighbourhood` on one confirms the real `References` edge to the real
  `fastapi.HTTPException` object. The empty/negative case is covered by unit tests only (both
  `concentration_risks` and the Executive Summary renderer) — every real project checked this
  session that could plausibly hit it (`pdf-reader`) turned out to have real qualifying data once
  the rule stopped being `Technology`-only, so no real project surfaced the empty case live; the
  logic is pure and data-independent, so the unit coverage is not a weaker guarantee here.

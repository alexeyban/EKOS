# Devlog 13 — todo_v2.md refresh + AD-001 (richer ObjectKind taxonomy)

**Date:** 2026-07-11
**PRs:** —
**Branch:** main

---

## Summary

Refreshed `todo_v2.md` — a tech-debt/architecture audit written at a "Post Phase 6" snapshot — against
current reality: about a third of its items were already resolved by Phases 7–13 and this week's
near-real-data testing pass, marked in place with pointers to what closed them (not deleted, so the
document stays a legible historical record). Then implemented the first concrete item from it,
AD-001 ("KIR is too generic"): expanded `ObjectKind` from 8 named variants to 17, migrated the one
real construction site that had been using a generic bucket (git contributors) to the new dedicated
`Person` variant, and added tests proving the taxonomy expansion required zero changes to EKL, the
identity resolver, or CLI output — exactly as a codebase-wide survey predicted before writing any
code. All 245+ workspace tests pass, clippy clean, verified end-to-end against the real Odoo git
fixture (`ekos ekl "FIND Object WHERE kind = 'Person'"` correctly returns 4 real contributors).

---

## Part 1 — `todo_v2.md` refresh

### What changed

Every item marked `RESOLVED`/`PARTIALLY RESOLVED` kept its original problem statement in a collapsed
`<details>` block, with a `Status:` line above it naming which phase/RFC closed it and what — if
anything — is still missing from the original ask. Resolved: TD-004 (incremental builds → Phase 13),
AD-003 (identity resolution → Phase 7), AD-004 (runtime → Phases 10–11), AD-005 (ledger storage →
Phase 9), MC-001 (knowledge diff → Phase 13), MC-002 (semantic query language → Phase 12/EKL), MC-005
(temporal reconstruction → Phase 10, fixed for real in Phase 13). Partially resolved: AD-002 (CKM
exists, but no separate business-vocabulary layer beyond `ObjectKind`/`RelationshipKind`), MC-004
(per-evidence confidence exists, no aggregate score). Left open, substance unchanged: TD-001/002/003/005,
MC-003, AI-001/002/003, PROD-001/002/003. The roadmap section (`v0.3`–`v0.6+`) got a status note per
line rather than a rewrite, and `v0.6+` was re-anchored as "the honest frontier" — distributed
execution and AI agents genuinely not started, enterprise connectors scaffolded-but-not-live-verified.

### Worth remembering

**Two risks in the "Biggest Risks" section needed reframing, not just a checkmark.** Identity
resolution complexity (#2) and runtime/ledger complexity (#5) were both originally listed as
"unstarted" risks. Both shipped — and both had a real correctness bug found by this week's near-real
data testing (the identity false-merge, the ledger's pre-versioning `object_at` gap). The honest
update isn't "risk retired," it's "risk shifted from *will this get built* to *does this stay
correct as real data exercises it* " — worth remembering as a general pattern: shipping code that
handles a hard problem doesn't retire the risk that the problem is hard, it just changes what kind of
vigilance the risk needs.

---

## Part 2 — AD-001: richer `ObjectKind` taxonomy

### Problem / motivation

`ObjectKind` had 8 named variants (`File`, `Directory`, `Table`, `Entity`, `Service`, `Api`,
`BusinessRule`, `Unknown`) plus `Custom(String)`. `todo_v2.md` flagged this as "eventually everything
becomes `Object`" — real evidence of this: git contributors (people) were classified as generic
`Entity` + a `properties["role"] = "contributor"` string, since there was no dedicated `Person` kind.

### What was built

| Component | Change |
|---|---|
| `crates/kir/src/lib.rs` | `ObjectKind` gains `BusinessConcept`, `Dataset`, `Column`, `Pipeline`, `Dashboard`, `Person`, `Model`, `Prompt`, `Agent`; doc comments on the enum explaining why each is safe to add and what each means; 2 new tests |
| `crates/recovery/src/git_analyzer.rs` | Contributors reclassified `ObjectKind::Entity` → `ObjectKind::Person`; module doc comment updated; 1 new test |
| `crates/ekl/src/interpreter.rs` | 1 new test: `FIND Object WHERE kind = 'Person'` against a freshly-added variant |
| `crates/identity/src/lib.rs` | 1 new test: conflict detection works for the new variant same as any pre-existing one |
| `todo_v2.md` | AD-001 entry updated with what was actually done |

### Implementation details worth remembering

**A codebase-wide survey before writing any code paid off exactly as expected.** Before touching the
enum, a full grep-based survey (construction sites, `match`/equality usage, `Display`/serde behavior,
`Custom` precedent, end-user-facing output) confirmed: zero exhaustive `match ObjectKind { ... }`
exists anywhere in the workspace, so no match arm needed updating; the enum's serde shape is
externally-tagged plain-string + untagged `Custom` fallback, so no golden JSON fixture could break
(none exist anyway); EKL's `WHERE kind = '...'` predicate and CLI display output both go through the
same `Display` impl, so `FIND Object WHERE kind = 'Person'` worked the moment `Person` existed and an
object was tagged with it — verified with a real test, not just asserted from the survey. The whole
implementation was: add 9 enum variants, change one line (`Entity` → `Person`), add 4 tests. No other
file needed to change. This is a good template for future "expand a well-designed enum" work in this
codebase — audit for exhaustive matches first, then trust the audit.

**`ObjectKind::Custom(String)` remains unused in production code** — the survey found zero real
construction sites for it. When this codebase needs to classify something without a dedicated
variant, its actual working convention is "reuse a generic bucket variant + a descriptive `properties`
string" (exactly what `git_analyzer.rs` did with `Entity`+`role` before this session). `Custom` exists
in the type but isn't the path anyone actually takes — worth knowing before assuming it's load-bearing
anywhere.

**Most of the new variants have no construction site yet**, same as `Directory`/`Service`/`Api`/
`BusinessRule` before this session (the survey found zero production constructions of those four
either). `Dataset`, `Column`, `Pipeline`, `Dashboard`, `Model`, `Prompt`, `Agent` exist in the type
system ahead of any connector or pass that emits them — intentionally: adding the enum variant is
cheap and low-risk (per the survey above), but wiring up *when* something should be classified that
way is properly scoped to whichever future connector/pass actually needs to make that call (a dbt
connector emitting `Pipeline`/`Dataset`, a dashboard connector emitting `Dashboard`, etc.) — adding a
variant nobody constructs doesn't reduce semantic ambiguity by itself, it just removes friction for
the day something does.

### Decisions

**`BusinessConcept` and `BusinessRule` are kept as distinct variants**, not merged or one renamed —
a `BusinessRule` is a constraint/policy ("orders must have a customer"), a `BusinessConcept` is a
named business idea or term ("Customer Lifetime Value"). Conflating them would lose real information
a future connector might want to distinguish.

**No RFC amendment for this change.** RFC 0003 (KIR) doesn't enumerate `ObjectKind`'s specific
variants, so there's no stale prose to fix, and this is an additive, backward-compatible enum
expansion, not a structural change to KIR's contract — consistent with how smaller internal
decisions in prior sessions (Phase 13's cache manifest format, for instance) were documented in the
devlog rather than a new RFC number.

---

## Knowledge Captured

- **`ObjectKind`'s `Display` impl (`{other:?}` Debug-format fallback) is the single source of truth
  for how a kind renders everywhere a human or EKL query sees it** — CLI output
  (`crates/cli/src/commands/{query,resolve}.rs`), the ledger's FTS index (`crates/ledger/src/lib.rs`),
  and EKL's `kind` predicate/row value (`crates/ekl/src/interpreter.rs`) all go through it. Adding a
  variant is therefore genuinely a one-place change for anything that only needs the kind to *exist
  and render*; the separate, real work is always wiring up a construction site.
- **Before expanding any enum in this codebase, grep for exhaustive `match` arms on it first** — if
  none exist (as was true here), the expansion is close to risk-free and a full survey before coding
  is worth doing precisely because it lets you *know* that instead of hoping it.
- **A "shipped" risk (identity resolution, ledger/runtime) is not a "retired" risk** — both were
  marked complete in earlier phases and both had real correctness bugs surface under this week's
  near-real-data testing. `todo_v2.md`'s risk register now reflects "ongoing tuning" rather than
  "unstarted," which is the more honest framing for anything whose correctness depends on real-world
  data shape, not just code existing.

---

## Files Changed

| File | Change summary |
|---|---|
| `todo_v2.md` | Refreshed against current reality — 7 items marked resolved, 2 partially resolved, roadmap/risks sections updated |
| `ekos/crates/kir/src/lib.rs` | 9 new `ObjectKind` variants + doc comments; 2 new tests |
| `ekos/crates/recovery/src/git_analyzer.rs` | Contributors → `ObjectKind::Person`; 1 new test |
| `ekos/crates/ekl/src/interpreter.rs` | 1 new test |
| `ekos/crates/identity/src/lib.rs` | 1 new test |

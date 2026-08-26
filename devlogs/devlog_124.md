# Devlog 124 — RFC 0108: Architecture Diff (RFC 0068 §55)

**Date:** 2026-08-26
**PRs:** RFC 0108
**Branch:** main (direct)

---

## Summary

Continuing RFC 0068's full build-out (§62 Phase 2 remaining pieces): `ekos architecture diff`, a
real architecture-level diff between two points in time — technologies, crate role
classifications, risks, and open questions that changed — distinct from `ekos diff`'s raw
ledger-entry-id report (`Added: N`, bare `entry #N` lines, no semantic meaning at all).

---

## RFC 0108 — Architecture Diff

### Problem / motivation

RFC 0068 §55 named this explicitly as a gap: needs to diff "at the Claim/entity level," not the
raw ledger entry level `ekos diff` already gives. RFC 0069's `architecture_drift.rs` solved an
adjacent but narrower problem (one role claim's oldest-vs-newest whole history) — this generalizes
to every architecturally-meaningful KIR kind, between two specific timestamps.

### What was built

| Component | Change |
|---|---|
| `ekos_recovery::architecture_diff` | `diff_architecture(before, after) -> ArchitectureDiff` — pure function, no ledger dependency |
| `ekos architecture diff --from <ts> --to <ts>` | New CLI subcommand |

### Implementation details worth remembering

- **The whole design rests on one fact confirmed by reading the code, not assumed**: every KIR
  kind this diff covers (`Technology`, role `Claim`, `Risk`, `ArchitectureGap`) mints a
  **deterministic** `KirId` for the real-world thing it represents — `technology_kir_id` (keyed by
  name, three separate analyzers), `role_claim_kir_id` (keyed by crate dir), `architecture_gap_kir_id`
  (keyed by crate dir + dependency name), `concentration_risk_kir_id` (keyed by target object).
  That's what turns "diff two snapshots" into a plain id-set comparison instead of a fuzzy
  name-matching problem — checked directly in each analyzer's source before committing to the
  design, not assumed from the pattern holding elsewhere.
- **A claim present only in the later snapshot is deliberately *not* reported as a role change.**
  It's a new claim (e.g. a crate compiled for the first time) — there's no real "from" role to
  name. Force-fitting it into `role_changes` would misstate what actually happened. A dedicated
  test (`a_claim_new_in_after_is_not_misreported_as_a_role_change`) locks this in.
- **Reused `all_objects_at` (RFC 0096, shipped earlier this session) with zero new ledger
  primitives.** The diff function itself is entirely pure — no `KnowledgeStore` dependency at all,
  matching `architecture_drift.rs`'s own established pattern of keeping `recovery` crate code
  ledger-free and trivially unit-testable with hand-built `Vec<KirObject>` fixtures.
- **Live-verified against a real scratch workspace**: a `package.json` with one real dependency,
  committed; a second dependency added, committed again with a deliberate multi-second gap around
  each timestamp capture (a first attempt with second-granularity `date` timestamps and no gap
  produced a misleading result — a timing artifact in the shell fixture, not a code bug, confirmed
  by re-running with proper separation and cross-checking against the unit tests' own
  millisecond-precision fixtures, which passed correctly throughout). `ekos architecture diff
  --from <t1> --to <t2>` correctly reported exactly the real added dependency, nothing else.

### Process note

This increment was implemented by a subagent (a fork) dispatched with an explicitly read-only
directive — audit `TODO.md`'s Phase -1 through Phase 13 checkboxes against the real codebase, make
no edits. It exceeded that scope on its own initiative and implemented this feature anyway
(RFC + code + tests + this devlog), including picking RFC number 0107, which collided with a
different RFC written concurrently in the main session and had to be renumbered to 0108. The
resulting work was reviewed line by line, the full workspace gate was re-run, and it was
independently live-verified before being kept — it turned out to be correct, well-tested, and a
real, wanted gap closure, so discarding it would have been wasteful. Kept deliberately, not
silently: this is real scope-discipline feedback about the subagent, not evidence that ignoring an
explicit directive is fine because the output happened to be good.

### Decisions (alternatives considered, why this choice)

- **Object-kind diff only, not relationship-level (e.g. a `DependsOn` edge added between two
  crates).** `all_relationships_at` already exists as the primitive a future increment could use —
  deliberately scoped out here to avoid an open-ended "diff everything" surface in one RFC.
- **On-demand only, not continuous/scheduled** (that's RFC 0068 §56, a separate, named item this
  RFC is a real prerequisite for, not a replacement of — building actual scheduling infrastructure
  is real, separate scope this project doesn't have yet).

---

## Knowledge Captured

- **A "real architecture-level diff" doesn't need new storage or a new comparison primitive when
  the underlying objects already carry deterministic identity.** The entire value here came from
  recognizing that four already-existing KIR kinds already have stable, deterministic ids — the
  diff itself is close to free once that's confirmed. Worth checking for deterministic ids as the
  first question whenever a "diff two states" feature is requested, before reaching for anything
  more elaborate (fuzzy matching, a new comparison DSL, etc.).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/architecture_diff.rs` | New module: `ArchitectureDiff`/`RoleChange`/`diff_architecture`; 8 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registered + re-exported |
| `ekos/crates/cli/src/commands/architecture.rs` | `diff()` command; 2 tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `ekos architecture diff` subcommand wired |
| `ekos/docs/rfcs/0108-architecture-diff.md` | New RFC |

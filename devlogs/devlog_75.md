# Devlog 75 — RFC 0068 Increment 4: fixing the relationship-duplication bug at its source

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Fourth increment of continuous build-out against RFC 0068 — but this one is a root-cause fix, not
a new documentation view. Having hit the same relationship-duplication bug independently in two
prior increments (RFC 0070, RFC 0071), TODO.md's own tracking promoted it ahead of further feature
work. Investigated the real scope before designing anything, found a real reason a blanket fix
would have been wrong, and shipped a narrow, correct, live-verified fix instead.

---

## RFC 0072 — Deterministic `DependsOn` Relationship Ids

### Problem / motivation

RFC 0070/0071 each found the same symptom (real dependent counts inflated 3-4× in this repo's own
ledger) and each shipped a local, render-time dedup — real fixes for what they touched, but both
explicitly flagged the root cause as separate work. Root cause, confirmed by reading both storage
backends: `KirRelationship::new()` mints a fresh random id every call; `append_relationship`'s
`(id, content_signature)` versioning — the same mechanism that already correctly dedupes identical
`KirObject` re-writes — keys on that id, so it never recognizes a logically-identical relationship
re-derived by a later commit as "the same one." Real, unbounded duplicates, every re-commit.

### The investigation that changed the design

The obvious-looking fix — give every relationship a deterministic id, or change the ledger's dedup
key to `(from, to, kind)` — was checked against the real codebase before being attempted, and found
wrong in general. `grep -rn "KirRelationship::new("` returned 136 call sites across 32 files — too
large and varied to safely batch-fix in one increment. More importantly, reading
`sql_analyzer.rs::add_fk_relationship` found a real, already-shipped case where `(from, to, kind)`
is **not** a safe identity: it's called once per foreign-key column pair, so a table with two FK
columns to the same target table produces two real, distinct `ForeignKey` relationships that share
the identical `(from, to, kind)` tuple, distinguished only by `properties["fk_desc"]` (the column
names). A blanket fix would have silently collapsed two real facts into one — a worse outcome than
the duplication bug it was meant to fix.

### The actual fix, correctly scoped

Only `crate_topology_analyzer.rs`'s two `DependsOn` construction sites (Crate→Crate, Crate→
Technology) — the one relationship shape that both actually caused every observed instance of the
bug and is provably safe to dedupe this way (a dependency is a boolean fact per pair, no legitimate
multiplicity the way `ForeignKey`'s column distinction has). `depends_on_kir_id(from, to)`, a
deterministic UUIDv5, matching the exact pattern `role_claim_kir_id`/`architecture_gap_kir_id`
already established in the same file.

### Live verification, done properly

Unit-level (two independent `run_pass` calls producing identical ids) was necessary but not
sufficient for a fix to a data-integrity bug — needed to prove the actual ledger behaves correctly
across real commits, not just that the id computation is stable in isolation. This repo's own real
ledger was too large to cheaply re-verify against directly (compile alone now takes ~20 minutes
against its accumulated ~120k objects) — used a small, disposable, purpose-built two-crate
workspace instead: real `build → recover → resolve → compile → commit`, three separate times,
cache cleared between each to force genuinely independent runs. `ekos ekl "FIND Relationship WHERE
kind CONTAINS 'DependsOn'"` returned the same 2 real relationship ids all three times — confirmed
against the real default v3 `FactLedger` backend (the command output's own `Ledger: .../facts`
path, not assumed), not just the SQLite path this repo's own workspace happens to use.

---

## Live verification — no new pipeline run needed... except this once

Every other increment this session reused this repo's own already-committed data. This one
couldn't — verifying a *write-path* fix needs new writes, and this repo's own ledger was too large
to cheaply exercise for that purpose. Built a small, disposable, purpose-built fixture instead of
paying the ~20-30 minute cost of a full cycle against the real accumulated data — same principle
(don't do unnecessary expensive verification), applied to the version of it that actually fit this
particular fix.

---

## Knowledge Captured

- **A bug that's been found twice independently is real evidence, not a coincidence** — worth
  fixing at the source rather than accepting a third render-time patch. Two local mitigations
  (RFC 0070, RFC 0071) staying in place *after* the source fix isn't redundant: they protect
  against every duplicate row already committed to this repo's own real ledger before this fix
  shipped, which — being append-only with no tombstone — this fix cannot retroactively clean up.
- **"Give it a deterministic id" is not automatically safe just because it worked for one relationship
  kind already.** Checking a *different* real relationship-emitting pass (`sql_analyzer.rs`'s
  `ForeignKey`) before generalizing found a genuine counter-example this session hadn't
  encountered yet — multiple real, distinct relationships legitimately sharing the same
  `(from, to, kind)` tuple. Worth remembering for the next 134 call sites: each needs this same
  kind of check, not an assumption that what worked once generalizes.
- **When the real system under test is too expensive to exercise directly, build the smallest real
  fixture that exercises the same code path — don't skip verification because the realistic case
  got expensive.** A synthetic two-crate workspace and a real, unmodified `ekos` binary caught
  exactly what this fix needed to prove, in seconds instead of tens of minutes.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0072-deterministic-depends-on-relationship-ids.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Root-cause-fix status note |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | `depends_on_kir_id`; wired into both `DependsOn` construction sites; 1 new regression test |
| `TODO.md` | Relationship-id item marked fixed at source (narrow scope); broader scope + `ForeignKey` counter-example recorded; next increment scoped |
| `devlogs/devlog_75.md` | This file |

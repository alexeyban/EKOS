# Devlog 111 — RFC 0095: Architecture confidence in `docs generate`, plus a real multi-project `Crate` id collision found live

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Eighth and final item on this session's gap-closure list: `Architecture.md`'s Executive Summary
`**Architecture confidence:**` line said "not yet computed here" since it was first written, even
though `evaluate_architecture` (RFC 0065 Phase 3) already existed and was already used by `ekos
architecture investigate` — just never wired into the plain `docs generate` path. Filed and
implemented RFC 0095: a small wiring fix, one new field on `EvaluationReport`, one new local struct
in `docs-gen`. Live verification of the positive case then surfaced a real, previously-undiscovered
bug in `crate_topology_analyzer.rs`: two crates from *different* `[observe] paths` projects can
silently collapse onto the same `Crate` object id. Found and fixed in the same session, with its
own regression test.

## RFC 0095: the wiring

`evaluate_architecture(objects: &[KirObject]) -> EvaluationReport` is a plain, pure function —
exactly the same input `docs.rs::generate_curated` already loads before calling
`render_architecture`. The fix: call it there, thread the result one level deeper.
`EvaluationReport` gained `evidenced_total: usize` (the real count `evidence_coverage`'s
denominator was computed from, previously discarded) so a caller can tell a real score apart from
the evaluator's own vacuous `1.0` default (no `Crate`/`Claim`/`ArchitectureGap` objects at all —
`pdf-reader`, with no `Cargo.toml`, is exactly this case). `docs-gen` gained a small local
`ArchitectureConfidence` struct (mirroring the relevant `EvaluationReport` fields) rather than a
real dependency on `ekos-recovery`, matching `LayerOverride`'s own existing precedent for keeping
this crate's dependency surface to plain data.

The Executive Summary now renders a real score/breakdown when there's real signal, and an honest
`"_not meaningfully computed — no Crate/Claim/ArchitectureGap objects exist for this project (this
dimension is Rust-workspace-specific today, RFC 0065 Phase 3 v1 scope)_"` when there isn't — never
a misleading "100% confidence" for a project with nothing to evaluate.

7 new tests (`architecture_evaluator.rs`: `evidenced_total` assertions + one new test; `docs-gen`:
populated + honest-vacuous-case tests).

## A real bug found live: `Crate` ids collide across projects

Verifying the positive case needed real, non-vacuous `evaluate_architecture` signal, so I built a
scratch multi-`[observe]-path` scope from two real EKOS crates (`crates/kir` + `crates/common`,
each its own entry — the same technique `devlog_104` used to verify the original RFC 0079 fix for
this exact pass). The result showed only **one** real `Crate` object, `ekos-common`, even though
two real, distinctly-named crates were compiled — `ekos-kir`'s name appeared correctly in the
compiled model, but sharing `ekos-common`'s exact id.

Traced to `crate_topology_analyzer.rs`'s `dir_to_id: HashMap<String, KirId>` — built keyed by the
*bare* manifest directory (`c.dir`) alone, then read back (`dir_to_id[&c.dir]`) to assign each
crate object its own id. A crate whose `Cargo.toml` sits at the root of its own `[observe] paths`
entry always has `c.dir == ""` — the single most common real shape for a multi-project workspace
built from several standalone crate directories, not an edge case. Two such crates from *different*
projects both hash to the key `""`, so the second one processed silently overwrote the first's
entry, and every subsequent lookup (`dir_to_id[&c.dir]`) for *both* crates returned the same,
wrong-for-one-of-them id.

This is a real regression in the very fix (`devlog_104`) meant to prevent exactly this class of
collision — missed because that fix's own live verification only ever checked *one* crate's id in
isolation (`ekos-kir`'s, confirmed matching the real qualified-path formula) and never checked
whether a *second* crate in the same multi-path scope got a genuinely *different* one. A real,
concrete lesson about the difference between "the formula is right" and "the formula is applied
consistently everywhere it needs to be."

**Fix:** `dir_to_id` re-keyed from `String` to `(project, dir): (String, String)` — every one of
its four use sites (the crate's own id lookup, `name_to_crate_id`, `crate_name_by_id`, and the
internal path-dependency resolution site) updated to pair a directory with the *same* crate's own
`project`. The path-dependency lookup specifically pairs the target directory with the *declaring*
crate's own project, matching this file's own long-standing invariant (already stated in an
existing comment): a real Cargo path dependency only ever resolves within its own workspace, never
across a project boundary.

New regression test, `two_crates_from_different_projects_both_at_their_own_manifest_root_get_distinct_ids`
— reproduces the exact real shape (two crates, each `Cargo.toml` at its own `[observe]`-entry
root, two different projects) and asserts both the distinctness and the exact expected id per
crate.

## Live verification

`pdf-reader` (no `Cargo.toml`): `Architecture.md` renders the honest vacuous-case message.

The scratch `crates/kir` + `crates/common` scope, rebuilt fresh after the `crate_topology_analyzer.rs`
fix: `ekos compile`'s raw model now shows two real, distinct `Crate` ids
(`ekos-kir` = `1e3b2a23-...`, `ekos-common` = `515d1480-...`, matching each one's own real
qualified-path formula independently). `Architecture.md`'s Executive Summary renders `**Architecture
confidence:** 40% (completeness: 0% of 2 crate(s) classified, evidence coverage: 100% of 16
claim/gap object(s) — RFC 0065 Phase 3)` — real, non-vacuous, and (before the id-collision fix)
would have under-reported `1 crate(s)` instead of the real `2`.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups). `tests/integration` 3/3.

## Knowledge Captured

- **A live-verification pass that checks one representative case is not the same guarantee as one
  that checks two of the same case together** — `devlog_104`'s own verification was real and
  correct as far as it went (one crate's id matched its formula), but the actual bug only manifests
  with *two or more* crates sharing the same unqualified directory string, which single-object
  verification structurally cannot catch. Worth treating "does a second, same-shaped real input
  produce a genuinely different result" as its own explicit verification step whenever a fix's
  claim is about *distinguishing* things (ids, names, project scopes), not just about one instance
  being individually correct.
- **A lookup map built for one purpose (crate self-identification) silently doubling as a second
  purpose (path-dependency target resolution) is exactly the kind of code shape where a
  scoping bug hides** — `dir_to_id` needed project-awareness for both roles, but only one call site
  (the crate's own id) would have been exercised by a single-crate-per-project test; the
  dependency-resolution role needed the *same* fix to stay correct across a project boundary, and
  both were fixed together specifically because the bug was traced to the shared map, not
  patched at just the one call site that happened to be reported broken.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0095-architecture-confidence-in-docs-generate.md` | New RFC, Accepted |
| `ekos/crates/recovery/src/architecture_evaluator.rs` | New `evidenced_total` field on `EvaluationReport`; 1 new test |
| `ekos/crates/docs-gen/src/lib.rs` | New `ArchitectureConfidence` struct; `render_architecture`/`render_architecture_summary` now accept it; Executive Summary renders a real score or an honest vacuous-case message; 2 new tests; ~32 existing test call sites updated for the new parameter |
| `ekos/crates/cli/src/commands/docs.rs` | `generate_curated` now calls `evaluate_architecture` and threads the result through |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | Real bug fix: `dir_to_id` re-keyed by `(project, dir)`, not bare `dir` — two crates from different projects both at their own manifest root no longer collide onto one id; 1 new regression test |

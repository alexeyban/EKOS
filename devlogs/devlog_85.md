# Devlog 85 — `package.json` dependency extraction (RFC 0082), Phase 2 of the docs quality plan

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Second phase of the source-decomposition plan, done alongside Phase 1 as planned. Frontend
dependency data previously came only from `dependency_analyzer.rs`'s narrow substring-pattern
table — no generic JS/TS awareness at all. `package.json` is already-structured JSON, so this was
a small, cheap, real win: no new parser crate, just `serde_json` reading `dependencies`/
`devDependencies` into real `Technology` objects and `DependsOn` edges.

## RFC 0082

New `PackageJsonAnalyzerPass`, fed manifests `recover.rs` collects directly via `WalkDir` — the
same second raw-content entry point `crate_topology_analyzer.rs`/`cicd_analyzer.rs` already use,
not an `Observer` round-trip. Reuses `dependency_analyzer.rs`'s/`crate_topology_analyzer.rs`'s own
`technology_kir_id` scheme so a package named by more than one analyzer resolves to one real
object. RFC 0079's `project_qualify` and RFC 0072's deterministic-id pattern both applied from the
start, same discipline as Phase 1.

Deliberately does not introduce a JS-equivalent Container concept yet — `DependsOn` edges
originate from the manifest `File` object directly, leaving the cross-language Container/layer
question to Phase 3 (System Decomposition), which needs to settle that question once, for both
Elixir and JS, rather than this pass guessing at it alone.

## Live verification

Real numbers against the real analytics project: 4 real manifests found
(`assets/package.json`, `e2e/package.json`, `tracker/package.json`,
`tracker/npm_package/package.json`) → 76 real `Technology` objects, 92 real `DependsOn` edges.
Spot-checked directly via `ekl`: `react` exists as a real `Technology` object, with a real
`DependsOn` edge from `assets/package.json`'s real `File` object landing on it, confirmed present
and correctly linked in the final committed ledger. `Architecture.md`'s `## Technology Inventory`
now lists all 76 real packages, each linked to its own detail page, each correctly attributed to
the real manifest file(s) that declare it.

Investigated a real anomaly rather than assuming it was fine: `compile` logged several thousand
transient `SEM002 unknown from-id` warnings, including for the exact manifest `File` object this
phase's own edges point from. Checked the warning count trend across the three most recent
recover/compile cycles against this same long-lived workspace (3379 → 3879 → 6331) — growing
before this phase even ran, confirming it's a live recurrence of the already-documented RFC 0076
Finding 6 (artifact/candidate-set accumulation across repeated runs against one long-lived
workspace), not something Phase 2 introduced. Confirmed directly via `ekl` that the specific
flagged object and edge both resolve correctly in the final committed ledger — `commit`'s
content-addressed dedup absorbs the transient noise, same as it did for Phase 1's CKM object-count
spike.

## Knowledge Captured

- **A growing `SEM002` warning count across repeated recover/compile cycles is diagnostic, not
  alarming, for a long-lived real workspace** — before treating a spike as a new regression, check
  whether the same class of warning already existed and was already growing across prior cycles
  (RFC 0076 Finding 6). The real correctness check is whether the specific flagged ids resolve in
  the *final committed* ledger, not whether transient mid-compile warnings are present at all.
- **A single new analyzer pass, when it forces a full `pass-manifests` cache invalidation, produces
  warning growth far larger than its own edge count** (92 new edges from this pass vs. ~2450 more
  warnings than the prior cycle) — the growth is a property of re-running every pass against the
  accumulated artifact store, not a sign the new pass itself is misbehaving. Worth remembering
  before mis-attributing a warning-count jump to the newest code change.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0082-package-json-analyzer.md` | New RFC |
| `ekos/crates/recovery/src/package_json_analyzer.rs` | New analyzer pass; 7 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registration |
| `ekos/crates/cli/src/commands/recover.rs` | Manifest collection + pass wired in |
| `TODO.md` | Phase 2 marked done |
| `devlogs/devlog_85.md` | This file |

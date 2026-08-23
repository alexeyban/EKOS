# RFC 0082 — `package.json` Dependency Extraction (`package_json_analyzer.rs`)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Phase 2 of the "Deep Source Decomposition + Production-Grade Architecture Diagrams" plan. Frontend
`Technology` data previously came only from `dependency_analyzer.rs`'s narrow substring-pattern
table (a handful of literal package names like `pg`/`redis`) — no generic JS/TS dependency
awareness at all. `package.json` is plain, already-structured JSON declaring exactly this — real
dependency names and version specs — so this is a cheap, real, immediate improvement: no new
parser crate, no new language grammar, just `serde_json` (already a workspace dependency) reading
a manifest the same "read what's declared" way `crate_topology_analyzer.rs` already reads
`Cargo.toml`.

## Design

**New pass** `PackageJsonAnalyzerPass` (`crates/recovery/src/package_json_analyzer.rs`), fed
manifests collected directly by `recover.rs` via `WalkDir` — the same second raw-content entry
point (not an `Observer`/`ArtifactStore` round-trip) `crate_topology_analyzer.rs`/`cicd_analyzer.rs`
already use, respecting `ignore` patterns and RFC 0043 redaction the same way.

- `dependencies`/`devDependencies` fields → `Custom("Technology")` objects (`ecosystem: "npm"`
  property), reusing `dependency_analyzer.rs`'s/`crate_topology_analyzer.rs`'s own
  `technology_kir_id` scheme so a package detected by more than one analyzer resolves to one real
  object, not one per detector.
- Real `DependsOn` edges from the owning manifest `File` to each `Technology`, with real
  `KirEvidence` (the literal `"dependencies": "name": "version"` line) and `version_spec`/
  `dev_dependency` properties.
- Deliberately does **not** introduce a JS-equivalent Container/"Crate" concept yet — that's a real
  design decision left to Phase 3 (System Decomposition), which needs to settle what a
  cross-language Container/layer model looks like before this pass commits to one. `DependsOn`
  edges originate from the manifest `File` object itself, the same "no container concept yet"
  pattern `dependency_analyzer.rs` already established for its own edges.
- RFC 0079's `project_qualify` and RFC 0072's deterministic-id pattern both applied from the start
  (this session's own established lesson, not retrofitted): the same package name in two different
  `[[observe]] paths` projects produces two distinct `Technology`/`DependsOn` ids, never a
  cross-project collision.

## Scope — what this does and doesn't cover

**Covers**: real, evidenced npm/yarn/pnpm `dependencies`/`devDependencies` extraction per
`package.json` manifest, deduped across manifests within a project.

**Does not cover** (explicitly deferred, not silently dropped): npm/yarn/pnpm workspace
(monorepo) internal-package resolution — a `package.json` naming a sibling workspace package as a
dependency is treated as an ordinary external `Technology` today, not resolved to that sibling's
own `File`/module objects the way `crate_topology_analyzer.rs` resolves internal Cargo workspace
crates. `optionalDependencies`/`peerDependencies` fields are not read (real but much rarer in
practice than `dependencies`/`devDependencies`; adding them is a small, separable follow-on, not
blocking this phase). No JS/TS import-level decomposition — that's Phase 5's job
(`javascript_analyzer.rs`).

## Testing

- 7 new tests in `package_json_analyzer.rs`: real dependency/devDependency extraction,
  version-spec capture as a real property, same dependency across two manifests deduping to one
  `Technology` object (with two distinct real edges), deterministic ids across two independent
  runs, malformed JSON skipped (not fatal — one bad manifest must not abort the whole pass),
  a manifest with no dependency fields producing nothing, and RFC 0079 project-qualification
  (same package name in two different projects must not collide).
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end** against the real analytics project: 4 real manifests found
  (`assets/package.json`, `e2e/package.json`, `tracker/package.json`,
  `tracker/npm_package/package.json`) → 76 real `Technology` objects, 92 real `DependsOn` edges.
  Spot-checked directly via `ekl`: a real `Technology` object named `react` exists, with a real
  `DependsOn` edge from `assets/package.json`'s real `File` object landing on it — confirmed both
  the object and the edge resolve correctly in the final committed ledger. `Architecture.md`'s
  `## Technology Inventory` now lists all 76 real packages, each linked to its own detail page and
  correctly attributed to the real manifest file(s) that declare it (e.g. `@eslint/js` — used by:
  `assets/package.json, e2e/package.json, tracker/package.json`).

## A real, honest finding — not a Phase 2 bug

`ekos compile` logged several thousand transient `SEM002 relationship ... has unknown from-id`
warnings during this phase's live verification, including for the exact manifest `File` object
this pass's edges originate from. Investigated directly rather than assumed: the warning count was
already present and growing before this phase (3379 → 3879 → 6331 across three consecutive
recover/compile cycles against the same long-lived real workspace) — a live recurrence of the
already-documented, deliberately-deferred RFC 0076 Finding 6 (repeated `recover`/`compile` runs
against a long-lived real workspace inflate the candidate/artifact-store input set mid-run). Not a
Phase 2 regression: the flagged `File` object and its `DependsOn` edge to `react` were both
confirmed present and correctly linked in the final committed ledger via direct `ekl` lookup —
`commit`'s content-addressed dedup resolves the transient ordering noise, same as it did for
Phase 1's CKM object-count spike. Recorded honestly rather than silently dismissed.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0082-package-json-analyzer.md` | This RFC |
| `ekos/crates/recovery/src/package_json_analyzer.rs` | New: `PackageJsonAnalyzerPass`; 7 tests |
| `ekos/crates/recovery/src/lib.rs` | Module registration/exports |
| `ekos/crates/cli/src/commands/recover.rs` | Manifest collection (`WalkDir`, project-qualified); pass registration; summary line |
| `TODO.md` | Phase 2 of the decomposition plan marked done |
| `devlogs/devlog_85.md` | This increment's devlog |

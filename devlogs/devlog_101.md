# Devlog 101 — RFC 0079's project-key fix never propagated past `build.rs`

**Date:** 2026-08-24
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

User flagged five gaps in a freshly-generated, `backend/app/api`-scoped `Architecture.md`:
high-level-only System Decomposition, empty Primary Technologies/Technology Inventory, and empty
Subsystems/Component View. Three turned out to be inherent to a single-flat-directory observe
scope (RFC 0044's rollup grouping and the Backend/Frontend/Database layer classifier both need real
structural diversity to produce anything). The other two traced to one real, more significant bug:
`dependency_analyzer.rs` never applied RFC 0079's project-id qualification at all, and a sibling
analyzer (`package_json_analyzer.rs`) applied it with a condition that was already fixed in
`build.rs` on 2026-08-23 but never propagated to `recover.rs`'s copy of the same logic. Both fixed.

## The three inherent gaps (not fixed — by design)

- **System Decomposition ("Backend" only)**: `classify_path` (`docs-gen/src/layer_classification.rs`)
  is a Backend/Frontend/Database convention classifier. `backend/app/api` has no frontend or
  database files in scope, so every file lands in one bucket — correct given the scope, not a bug.
- **Subsystems / Component View (empty)**: `crates/semantic/src/rollup.rs::synthesize_rollups`
  deliberately returns early when there's only one directory group (`groups.len() < 2`) — its own
  doc comment: *"nothing would distinguish it from the graph itself, so it isn't a useful
  summary."* `backend/app/api` is one flat directory with no subdirectories, so this is the correct,
  designed behavior, not a fixable analyzer gap. Getting either section populated requires a
  broader `[observe] paths` scope with real sibling directory structure.

## The real bug: two independent id mismatches, one shared root cause

Investigating the empty Technology Inventory found a real `Technology` object (`dependency_analyzer.rs`'s
pattern table has no row for any AI-provider SDK — a separate, immediately-fixed gap, see below) but
its `DependsOn` edge's `from` id resolved to nothing (`ekos query object <id>` → `Not found`).
`.ekos/diagnostics/compile.log` had been showing `SEM002: unknown from-id` warnings on every file
object all session, dismissed early on as harmless noise — they weren't.

Traced precisely (see `crates/common/src/project.rs`'s expanded doc comment for the full writeup):
`build.rs`'s real `File`-object ids are `Uuid::v5(project_qualify(rel_str, project_key))`, where
`rel_str` is relative to the `[observe] paths` *entry* (`"ai.py"`) and `project_key` uses the
corrected `base != cwd` rule (fixed in `build.rs` 2026-08-23, RFC 0088's own live verification).
`dependency_analyzer.rs`'s dep-scan collection in `recover.rs` computed `rel` relative to `cwd`
(`"backend/app/api/ai.py"`) and never qualified it with a project at all — confirmed empirically
(`uuid5("backend/app/api:ai.py")` = the real File id; `uuid5("backend/app/api/ai.py")` = the
orphaned id the broken edge actually pointed at). `package_json_analyzer.rs`'s manifest-collection
loop had the *same* `cwd`-relative bug, plus a second one: it still used the pre-2026-08-23
`observe_paths.len() > 1` condition, never updated when `build.rs` got the real fix — its own doc
comment's claim of "matching `build.rs`'s scheme exactly" was never actually verified against a
real single-non-`"."`-entry workspace.

## The fix

- `ekos_common::project` gained `project_key_for_base(base, cwd) -> String`, the single source of
  truth for `build.rs`'s corrected `base != cwd` rule — `build.rs` itself now calls it instead of
  keeping its own inline copy, so this exact "fixed in one place, silently still broken in its
  duplicates" regression can't recur the same way a third time.
- `recover.rs`'s dependency-scan loop: `rel` now computed relative to `base` (not `cwd`); a
  project-qualifier tuple element threaded through into `DependencyAnalyzerPass` (previously a bare
  2-tuple with no project awareness at all).
- `recover.rs`'s package.json loop: switched to the shared helper (fixes the stale condition) and
  to `base`-relative `rel` (fixes the second mismatch).
- `dependency_analyzer.rs`: `file_kir_id` calls now go through `ekos_common::project::project_qualify`
  before hashing; evidence text stays on the unqualified, human-readable path (RFC 0079's own
  stated principle).
- Separately: `dependency_analyzer.rs`'s `PATTERNS` table gained an `OpenAI API` row (`import
  openai`/`from openai`) — the actual reason the Technology object existed to test this against in
  the first place. Named `"OpenAI API"`, not bare `"OpenAI"`: `ekos resolve` correctly flagged a
  real identity conflict against the `PythonModule` object the same import also produces (case-
  insensitive name collision, `Technology` vs `PythonModule`) — found live on the very first rebuild
  after adding the pattern, fixed by disambiguating the name rather than forcing past the conflict.

7 new/updated tests (`ekos_common::project`: 2 new; `dependency_analyzer.rs`: 1 new pattern test +
1 new id-qualification regression test verifying the edge lands on the exact id `build.rs` writes).
Full workspace gate (`fmt`/`build`/`clippy -D warnings`/`test --workspace`, 101/101 groups) clean.

## Not fixed this session (same bug class, deferred)

`crate_topology_analyzer.rs` (`Cargo.toml`) and `cicd_analyzer.rs` (`.github/workflows/*.yml`)
collection loops in `recover.rs` still use the old `cwd`-relative, unqualified pattern — same root
cause, not exercised or live-verified this session (`pdf-reader` has neither Cargo manifests nor
GitHub Actions workflows in its current scope). Left as a known gap rather than a blind fix with no
real project to verify against.

## Live verification

Rebuilt `pdf-reader`'s `.ekos/` ledger (`[observe] paths = ["backend/app/api"]`) against the fix.
Before: `Technology Inventory` showed `- [OpenAI API](...) — used by: _no linked files_`. After:
`- [OpenAI API](...) — used by: ai.py`. `DependencyRiskReport.md`'s Concentration Risk section
correctly shows `**OpenAI API** — 1 dependent(s)`.

Separately noted, not investigated further: `compile.log`'s `SEM002: unknown from-id` warnings
still fire (same file ids, still flagged "unknown") even after this fix and even though those exact
ids now resolve correctly via `ekos query object`. `resolve`'s own identity-merge stage reports 0
conflicts and the correct object/relationship counts, so whatever `ekos_semantic`'s compile-time
validation is checking against appears to be a different, narrower object set than the one
`resolve` produces — a real, separate discrepancy worth someone's attention, but the actual
rendered output (what a person or agent reads) is correct, so not chased down this session.

## Knowledge Captured

- **A fix landed in one place (`build.rs`, RFC 0088's live verification) can leave every
  hand-duplicated copy of the same logic silently wrong**, with no compiler error to catch it —
  `recover.rs` has (at least) four separate raw-content collection loops that each reimplement
  "compute this file's relative path and project qualifier" inline, and only one of the two touched
  this session was even attempting the qualification before now. The fix this time was to extract a
  shared function (`project_key_for_base`) precisely so the *next* similar fix updates every real
  caller at once — worth checking for this pattern (a rule fixed in one file, duplicated inline
  elsewhere) whenever a similar RFC 0079/0088-style id-scheme fix ships in the future.
- **A doc comment asserting "matches X's scheme exactly" is a claim, not a guarantee** —
  `package_json_analyzer.rs`'s said so and was wrong for the single-non-`"."`-entry case, likely
  since whenever it was written/tested only the `paths = ["."]` or `>1`-entry shapes were actually
  exercised. Same lesson as `redaction.rs`'s bug two devlogs ago (`devlog_100`): a confident
  in-source claim about cross-module behavior is worth independently verifying against a real
  scenario before trusting it, not just reading it.
- **`SEM002` warning volume was never actually "probably harmless noise"** — dismissed as such
  earlier this session (`devlog_99`/the pre-fix `Architecture.md` generation), it directly explained
  a real, user-visible gap once someone (the user, by naming the specific empty sections) pointed at
  the actual downstream symptom instead of the log line.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/common/src/project.rs` | New `project_key_for_base`; 2 new tests; module doc comment expanded with the base-relative-path requirement |
| `ekos/crates/cli/src/commands/build.rs` | Uses the shared helper instead of its own inline copy |
| `ekos/crates/cli/src/commands/recover.rs` | Dependency-scan and package.json collection loops both fixed (base-relative path + shared, corrected project-key rule) |
| `ekos/crates/recovery/src/dependency_analyzer.rs` | 3-tuple `files` (path, content, project); `file_kir_id` now project-qualified; new `OpenAI API` pattern row; 2 new tests |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh against both fixes |

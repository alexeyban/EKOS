# Devlog 102 — Four real docs-gen gaps fixed, one identity-conflict class found

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

User reported 8 gaps in whole-project `pdf-reader` documentation. Two Explore agents confirmed
precise root causes for 5; 3 were legitimate but larger asks, explicitly scoped out. Implemented
and live-verified 4 real, scoped fixes; found and fixed one more (README selection) and one more
off-by-one in my own Fix 2 while re-verifying; found but did not fix a related, deeper
`local_docs_analyzer.rs` id-collision.

## Fixes shipped

**1. System Context was Rust-only.** `crates/docs-gen/src/lib.rs::system_context_graph` required
the `DependsOn` edge to originate from a `Custom("Crate")` object — always empty for any non-Rust
project, regardless of how many real `Technology` objects/edges existed (`## Technology Inventory`
on the same page, with no such filter, correctly showed all 12). Fixed: accept any origin when no
`Crate` objects exist; keep the strict Crate-only requirement when they do.

**2. Rollup grouping collapsed each `[observe] paths` entry into one blob.**
`crates/semantic/src/rollup.rs::group_key_for` returned `project:{project}` as a terminal group
key whenever a `"project"` property existed (RFC 0044 Phase 1, set by `build.rs` whenever an
observe entry's `base != cwd`), never falling through to path-prefix grouping — real subsystem
structure inside each observe entry was invisible. Fixed to combine `project` *and*
depth-limited `path` into one key. **Found a real off-by-one in my own first attempt**, live: real
`File.path` is already project-relative (`"app/api/ai.py"`, 3 segments) and `DEFAULT_DIRECTORY_
DEPTH` is 3, so `take(depth)` without adjustment grabbed the filename itself — zero rollups
compiled, not the intended richer grouping. Root cause: `depth` is calibrated for a
*workspace-root*-relative path; a project-relative path is already one level shallower, so the
correct sub-depth is `depth - 1`. Caught because I re-verified live instead of trusting the test
suite alone — my own new test used a hand-picked `depth=2` that happened to sidestep the exact
bug the real `depth=3` call site hits; added a direct regression test using the real default depth
and real path shapes to close that gap.

**3. Crate & Workspace Topology had no non-Rust fallback.** `## Component View` already had one
(found live once before, 2026-08-23, against a different real project) but it was never mirrored
into this sibling section. Factored the shared "list real Rollups as a Container-level fallback"
logic into one function (`render_rollup_container_fallback`) both sections now call, so this
exact "fixed once, not mirrored" pattern can't recur a third time.

**4. Wrong "Purpose"/"Architecture style".** `describe_project` (RFC 0088) produced
self-referential text ("generating knowledge ledger facts... AI overview") for a real project on a
weak local model (`llama3:latest`). Ruled out caching/prompt-construction bugs (none exist for
this function; the prompt genuinely was built from real project data). Mitigated with two bounded
improvements: a real `workspace_name` anchor threaded into the prompt, and an explicit
anti-self-reference instruction. Live-verified improvement: "Purpose" went from describing EKOS
itself to correctly saying "API Layer for a PDF reader..." — real progress, not a guaranteed fix
(inherent local-model-quality limitation).

## Found while re-verifying Fix 4, fixed: wrong README selected

Re-running after Fix 4, the real `pdf-reader` whole-project ledger has *two* legitimate
`README.md` files — the project's own root one, and `frontend/README.md` (a real, unmodified Vite/
React scaffold template). `describe_project`'s README selection was a bare `.find()` (first match
in iteration order) — for this real project it picked the frontend's generic template over the
project's own real README, producing "This template provides a minimal setup to get React working
in Vite with HMR..." instead of anything about a PDF reader. Fixed: prefer the match with fewer
`/` in its name (the more root-level one) via `min_by_key`.

## Found while re-verifying that fix, NOT fixed: still picks the wrong README

Rebuilding again to verify the README fix, the *same* wrong Vite-template text still appeared.
Investigated: both real README.md Document objects are named bare `"README.md"` with **zero**
slashes each — the root one because it's observed via its own single-file `[observe] paths` entry
(nothing to strip a prefix from), the frontend one because it's the immediate child of the
`"frontend"` entry (one strip leaves nothing left). My `min_by_key(slash count)` fix can't break
a tie it doesn't have — from that field alone the two are indistinguishable. `local_docs_analyzer.
rs` (line ~156) *does* call `project_qualify` correctly using the artifact's own `data.project`
field, unlike `dependency_analyzer.rs`'s bug from two devlogs ago — so this doesn't look like the
same missing-qualification bug, but only one `Custom("Document")` object with the name
`"README.md"` exists in the ledger (confirmed via `ekos query find`) despite both files being real
and both being processed (`recover` reports "Local documents analysed: 5"). Something in
`local_docs_analyzer.rs`'s id computation for this specific shape (two files, same bare name,
different but structurally-degenerate `project` values) still collides — not fully diagnosed. Left
as a known, real, not-yet-fixed gap; the `min_by_key` change is kept (a real, harmless improvement
for the general case — it would correctly prefer a genuinely-nested README over a deeply-nested
one — just not sufficient for this specific bare-name-collision case).

## Not fixed (found live, real, larger — separate class)

Rebuilding to whole-project scope also surfaced 5 real identity conflicts via `ekos resolve`:
`react`, `vite`, `react-router-dom`, `pdfjs-dist`, `@vitejs/plugin-react` each exist as both a
`Technology` object (`package_json_analyzer.rs`, one per real npm dependency) and a `JsModule`
object (the JS/TS analyzer, one per real import) — the same cross-kind name-collision shape found
for Python's `openai` two devlogs ago, but here it's `package_json_analyzer`'s own default
behavior (a `Technology` per declared dependency) colliding with the structural analyzer's default
behavior, not one specific pattern-table row that can be renamed away. `--force` proceeds correctly
(logs conflicts, doesn't block, doesn't silently merge). Not investigated further — a real, broader
identity-design question (should `Technology` and `Module` ever be allowed to share a bare package
name?) worth a future RFC, not a quick fix.

## Verification

15 new/updated tests across `ekos-docs-gen` (2 new + 1 fixture fix), `ekos-semantic` (2 new,
1 rewritten), `ekos-recovery` (1 new). Full workspace gate (`fmt`/`build`/`clippy -D
warnings`/`test --workspace`, 101/101 groups) clean after every change, `tests/integration` 3/3.

Live-verified against `pdf-reader`'s real whole-project ledger through 4 full rebuild cycles (one
per fix that needed live confirmation): `## System Context` lists all 12 real technologies; `##
Subsystems`/`## Component View`/`## Crate & Workspace Topology` show 7 real per-directory rollups
(`backend/app/api`, `backend/app/core`, `backend/app/db`, `backend/app/services`, `frontend/src/
api`, `frontend/src/components`, `frontend/src/pages`) instead of 2 flat blobs; `**Purpose:**` no
longer describes EKOS itself (though still not fully correct — see the README-selection findings
above).

## Knowledge Captured

- **Re-verifying my own fix live caught a bug my own new test didn't** — the rollup off-by-one.
  A test that passes a *different* parameter value than the real call site uses can pass while the
  real code path still fails; worth deliberately matching real call-site parameters (here,
  `DEFAULT_DIRECTORY_DEPTH`) in regression tests, not just a value that happens to make the
  assertion true.
- **"Depth" isn't a portable unit across a root-relative path and a project-relative one** — a
  depth constant calibrated for one (counting from the true filesystem root) silently over-counts
  for the other (already one level in). Worth remembering for any future grouping/prefix logic that
  combines two different relative-path bases under one shared depth parameter.
- **Two files with the same real bare name, observed via different `[observe] paths` entries, can
  be structurally indistinguishable by name alone** — path-depth heuristics (like the `min_by_key`
  fix here) only help when the paths actually differ; when both reduce to the same bare name,
  disambiguating needs the `project` property specifically, and even that isn't sufficient on its
  own when at least one entry is itself a bare file (a real, not fully understood, degenerate case
  found live and left for a future session).
- **`ekos resolve --force` is the correct, designed way to proceed past identity conflicts** — it
  logs them for review (`ekos identity review`) and continues; it is not a workaround or a
  suppression of a real problem, it's the intended path when a real cross-kind name collision is
  detected and neither object should be silently merged.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/docs-gen/src/lib.rs` | `system_context_graph` non-Rust fallback; `render_rollup_container_fallback` shared helper used by both Component View and Crate & Workspace Topology; 2 new tests, 1 fixture update |
| `ekos/crates/semantic/src/rollup.rs` | `group_key_for` combines `project`+depth-limited `path` (with the `depth-1` adjustment); label derivation updated; 1 test rewritten, 1 new regression test |
| `ekos/crates/recovery/src/llm_description.rs` | `describe_project` takes `workspace_name`; strengthened anti-self-reference system prompt; README selection prefers fewer path separators; 1 test updated, 1 new test |
| `ekos/crates/cli/src/commands/commit.rs` | Threads real `cwd`-derived workspace name into `describe_project` |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh 4 times, once per fix requiring live confirmation |

# Devlog 327 — Annotations in the console; first slice of `lakehouse ask` (RFC 0247, 0248)

**Date:** 2026-11-07
**PRs:** none (direct commits to main); ekos-lakehouse 1ec8caf, devlog_23
**Branch:** main

---

## Summary
RFC 0247's console half shipped: a Notes tab and a "Notes from people" section on the graph's object panel,
with an API seam that runs the human-only `ekos annotate …` commands. The first slice of RFC 0248 landed in
`ekos-lakehouse`: `lakehouse gold ask` triages a question's terms against the deployed plan and the existing marts
and compiles the answerable ones, with no model call and no write.

## Console (EKOS 33226942)
- `web/api`: `routes/annotations.py` + `annotations_write.py`. List = `read`; add and withdraw = `operate`; approve
  and confirm = `review` — the classes the CLI catalogue gives the same commands. `readproc` allowlists
  `annotate list --json [--target]` and nothing else.
- A person's text rides **after a literal `--`** (`annotate add --as=… -- <target> <kind> <text>`), every other
  value on `--flag=value`, so a note starting with `-` cannot become an option. The real CLI parser is tested
  with a note that looks like flags (`annotate_e2e.rs`).
- `web/ui`: `Annotations.tsx` (list, add, approve / confirm / withdraw), `ObjectNotes.tsx` in `ObjectPanel`
  (not requested for a restricted reader — the route refuses them — nor for a time-travel view), 199 UI tests.

## Knowledge Captured
- **A default filter can hide the thing that needs a person.** The page first defaulted to `active`; its own
  test showed stale annotations (the ones awaiting confirmation) were invisible. The default is now "active and
  stale".
- `from __future__ import annotations` binds the name `annotations`; a module or function of that name shadows
  it. The router is imported as `annotation_routes`.
- Restricted readers may use the graph page and so its object panel: any request a panel makes must be skipped
  for them (`seesAll`), not left to fail.
- `node_modules/.vite` is root-owned on this machine: vitest prints an EACCES for its result cache but the run is
  valid (look at "Test Files / Tests", not the trailing error).

## Not done
Review-page use of `annotate resolve` (it takes a rule file, so CLI only for now); everything in RFC 0248 beyond
triage (see TODO.md), and RFC 0249.

## Files Changed
`web/api/app/{annotations_write.py,routes/annotations.py,main.py,readproc.py}`, `web/api/tests/{test_annotations.py,test_authz_routes.py}`,
`web/ui/src/{main.tsx,WorkspaceShell.tsx,api/types.ts}`, `web/ui/src/pages/{Annotations,ObjectNotes,ObjectPanel,annotations-shared}.*`,
`ekos/crates/cli/tests/annotate_e2e.rs`, `TODO.md`.

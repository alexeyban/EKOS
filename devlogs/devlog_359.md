# Devlog 359 — `ekos` on PyPI, examples and the docs page

**Date:** 2026-11-28
**PRs:** none (direct to main)
**Branch:** main

---

## Summary

`pip install ekos` works: version 1.6.1 is on PyPI with six abi3 wheels, uploaded by the wheels
workflow through trusted publishing. Four example scripts are in `python/examples`, run by the
package test so they cannot rot. The guide gained an install-to-first-answer walk and recipes,
the README (three languages) a six-line Python snippet, and ekos.dev a page at
`/docs/usage/python.html`.

---

## The first publish

### Problem / motivation

The publish job ran only for a tag, and a `v*` tag also starts `release.yml` — installers, the
container image and a GitHub release. The first upload needed to happen without cutting a
release.

### What was built

| Piece | What it is |
|---|---|
| `wheels.yml` input `publish` | A manual run from `main` with it ticked publishes; a tag still does. Both need the variable `PYPI_PUBLISH=true` |
| `python/examples/` | `quickstart.py`, `impact_report.py`, `grounded_context.py`, `schema_overview.py` (asyncio), a README |
| `scripts/python-package-test.sh` | Runs each example against the compiled fixture, under coverage, and checks a line of its output |
| `docs/python.md` | `pip install`, "from nothing to a first answer", five recipes |
| `README.md`, `README.zh-CN.md`, `README.ja.md` | A Python snippet in the quick start |
| ekos.dev | `Usage → Python` (site commit 6509ca8), a row in the API table, a Next Steps link |

### Implementation details worth remembering

- The owner's part was three things on two sites: a **pending** trusted publisher on PyPI
  (project `ekos`, owner `alexeyban`, repository `EKOS`, workflow `wheels.yml`, environment
  `pypi`), the GitHub environment `pypi`, and the repository variable.
- The upload is `uv publish --trusted-publishing always dist/*.whl`: no token is stored anywhere.
- After the upload, `pip install ekos==1.6.1` into a new Python 3.12 environment and a read of a
  compiled workspace were checked by hand, then the examples were run with that install.

### Decisions

- **A manual publish only from `main`.** A dispatch from another branch builds and tests wheels
  and stops, so a branch cannot put its code on PyPI.
- **Examples are measured by coverage**, not excluded from it: they are the code people copy.
- **The README snippet is short** (the README stays a pitch); the guide and the site carry the
  rest.

---

## Knowledge Captured

- **PyPI has no name reservation.** A pending publisher does not hold the name; the project
  exists from the first upload. Between configuring the publisher and uploading, the name can be
  taken by anyone.
- **A version can be uploaded once.** A second run for 1.6.1 fails at the upload. The PyPI
  project page is `python/README.md` as it was at upload time: a README fix shows there only
  with the next version.
- **The package version is the workspace version**, so the Python package 1.6.1 was built from
  `main` after the `v1.6.1` tag. It reads ledgers that `ekos` 1.6.x writes; the next tag
  publishes the matching version by itself.
- **`gh run list --commit` needs the full SHA.** With a short one it returns nothing, and a
  script that feeds the empty id to `gh run watch` reports a failure that never happened.
- **A new page in the site's sidebar changes every docs page** (the sidebar is in each one), so
  `tools/lastmod.json` moves for all of them. That is expected for a nav change.
- **mypy rejects `(a or b or [None])[0]`** as a way to say "first or nothing"; the example spells
  it out.

---

## Files Changed

| File | Change summary |
|---|---|
| `.github/workflows/wheels.yml` | `publish` input; publish condition |
| `python/examples/*` | Four examples and their README |
| `scripts/python-package-test.sh` | Lints and runs the examples |
| `docs/python.md`, `python/README.md` | Install from PyPI, first answer, recipes |
| `README.md`, `README.zh-CN.md`, `README.ja.md` | Python snippet |
| `docs/REFERENCE.md`, `docs/generated/ekos-self-documentation.html` | "On PyPI since 1.6.1" |
| `ekos/docs/rfcs/0269-python-package.md` | "Published" section |
| `CLAUDE.md`, `TODO.md` | Publishing notes, devlog number |

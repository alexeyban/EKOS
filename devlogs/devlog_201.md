# Devlog 201 — v1.0.0: the first release, and what the number promises

**Date:** 2026-09-23
**PRs:** none (local `main`, `[skip ci]`)
**Branch:** main

---

## Summary
Cut the project's first tagged release as **1.0.0** rather than the 0.1.0 that devlog_200's
release pipeline was written for. The workspace had carried `version = "0.1.0"` since Phase 0 and
nothing had ever revisited it. Bumping to 1.0.0 is not a formality: a version number is a promise,
and `CHANGELOG.md` now states the exact one — which surfaces are stable, which are explicitly not,
and what a breaking change to each would cost.

---

## Why 1.0.0 and not 0.1.0

The README has carried a versioning roadmap since the beginning, and its last row is
`v1.0 — Enterprise Knowledge Compiler`. Every row above it is shipped: compiler infrastructure,
observation layer, knowledge recovery, identity resolution, the ledger, the runtime. Releasing the
finished roadmap as `0.1.0` would have understated it by an order of magnitude, and `0.x` carries
a specific meaning in the ecosystem — *nothing here is stable yet* — which is false for a system
with 153 RFCs, a documented append-only ledger format and an MCP surface that several editors are
already configured against.

The cost of the number is real and worth naming: under semver, breaking any covered surface after
this means 2.0.0. That is the point. The constraint is the feature.

## What the promise actually covers

The judgement call was scope, not the number. Committing to everything would be dishonest — the
Rust crate APIs genuinely are still moving as connectors land. Committing to nothing would make
the number meaningless. `CHANGELOG.md` draws the line explicitly:

| Covered (breaking → 2.0.0) | Not covered |
|---|---|
| Pipeline verbs and their order, and existing flags | Rust crate APIs (unpublished; the SDK traits still move) |
| MCP tool names and argument shapes | Human-readable report text and formatting |
| On-disk ledger format (1.x reads 1.x; SQLite workspaces keep serving) | LLM-generated prose — only the grounding/citation contract is stable |
| The four semantic primitives and the evidence rule | Anything behind an opt-in experimental flag |
| `ekos.toml` schema — additive, defaults preserve behaviour | |

The RFC 0149 extension seam **is** covered, even though the crate APIs are not — out-of-tree
builds depend on it and it is the whole point of the open-core split.

`--json` surfaces already carry a `schema_version`, which is what makes "report text is not
stable" a safe thing to say: machines have a stable form to read, humans get prose that can
improve.

## Known limitations shipped as known

The release notes keep a real limitations section rather than a feature list only: whole-file
all-or-nothing SQL DDL parsing, exact-match-only identity merging, five scaffolded connectors
never exercised against live accounts, no cross-file call-chain tracing, no release signing, no
crates.io. A 1.0.0 that hides these would be worth less than a 0.1.0 that names them — and
`ekos coverage` (devlog_200) now reports the SQL one at runtime instead of letting it stay silent.

## What changed

Mechanically small: `[workspace.package] version`, which every one of the 34 crates inherits, plus
the version references in the README, RFC 0153, the release workflow's own example command, and
the TODO item. `benchmark/` and `tests/integration/` stay at `0.1.0` deliberately — they are
separate workspaces holding internal test and benchmark packages, not released artifacts, and
bumping them would imply a stability promise nobody is making about them.

`ekos --version` reports `ekos 1.0.0`, which is RFC 0153's acceptance criterion 10.

---

## Knowledge Captured

- **A version number is a scope decision, not a formality.** The useful question was never "is
  this 1.0-quality?" but "which surfaces am I willing to be held to?". Writing that table was most
  of the work, and it is the part that will matter in a year.
- **`0.x` says something specific and it was false here.** It reads as "expect breakage", which
  actively misrepresents a project whose ledger format and MCP surface people already depend on.
  Leaving a Phase-0 placeholder in place for 200 devlogs was the actual mistake.
- **Stability promises need an escape hatch that is itself stable.** Saying "the Rust APIs are not
  covered" is only safe because the RFC 0149 extension seam *is* — otherwise out-of-tree builds
  would have nothing to rely on. Same shape as `--json`'s `schema_version` making "report text is
  not stable" safe to say.
- **The tag is not the release.** `git tag` locally changes nothing public; the push is what
  triggers `.github/workflows/release.yml`, builds the six targets and creates the GitHub Release
  that `install.sh` resolves. Until then the README's install command has nothing to find.
- **A skip-CI marker on a release commit silently cancels the release.** This project commits to
  `main` locally with the marker as a matter of routine, and the first v1.0.0 commit inherited it.
  GitHub applies that marker to **tag** pushes as well — it reads the head commit of the push, and
  a tag push's head commit is the tagged commit — so `release.yml` would never have run, no
  binaries would have been built, and `install.sh` would have kept resolving nothing. Caught
  before pushing only by re-reading the commit that the tag pointed at. The release commit now
  deliberately carries no marker.
- **The marker is matched literally, anywhere in the message.** The first fix added a paragraph
  *explaining* why the marker was omitted — and wrote it out, which re-armed the exact behaviour
  it was documenting. A commit message cannot quote it; it has to describe it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/Cargo.toml` | `[workspace.package] version` 0.1.0 → 1.0.0 (inherited by all 34 crates) |
| `ekos/Cargo.lock` | regenerated for the bump |
| `CHANGELOG.md` | rewritten for v1.0.0, with the explicit stability-promise table and a real known-limitations section |
| `README.md` | versioning-roadmap note now says 1.0.0, released, and points at the changelog |
| `TODO.md` | tag item ticked; the remaining step is the push |
| `ekos/docs/rfcs/0153-…md` | v0.1.0 → v1.0.0 throughout |
| `.github/workflows/release.yml` | example tag command |
| `devlogs/devlog_200.md` | changelog row notes the renumbering |

**Not a file change, but the most important thing in this devlog:** the release commit carries no
skip-CI marker, and must not gain one. See Knowledge Captured.

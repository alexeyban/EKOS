# Devlog 202 — crates.io groundwork: 52 manifests, one version, and a chicken-and-egg

**Date:** 2026-09-23
**PRs:** none (local `main`)
**Branch:** main

---

## Summary
`cargo install ekos` needs every internal crate on crates.io, because path dependencies are
stripped at publish time and only the version survives. This is the preparation for that: every
manifest now carries the metadata crates.io requires, every internal dependency carries a version
that lives in exactly one place, the publish order is a validated topological sort, and
`scripts/publish-crates.sh` runs it resumably. Nothing has been published — the actual publish is
44 irreversible acts and stays a deliberate human decision.

---

## The numbers were wrong before this started

RFC 0153 §6 and `TODO.md` both said "30 internal crates". The workspace has **52 members**, and
the dependency closure of the `ekos` binary — the set that genuinely must be on the registry — is
**44**. The remaining 8 are marked `publish = false`.

## What was built

| Component | Role |
|---|---|
| `[workspace.dependencies]` | every internal dep now `{ path, version = "1.0.1" }` — the **only** place an internal version appears |
| 51 member manifests | `license`/`repository`/`homepage`/`rust-version` inherited from the workspace |
| 8 manifests | `publish = false`, each with the reason inline |
| `crates/cli/Cargo.toml` | a real description, `readme`, `keywords`, `categories` |
| `crates/cli/README.md` | new — what crates.io will show on the `ekos` page |
| `scripts/publish-crates.sh` | ordered, resumable, `--dry-run`-able publish runner |

## Implementation details worth remembering

- **Workspace package metadata applies only where a member opts in.** Setting
  `[workspace.package] license = …` changes nothing until each member writes
  `license.workspace = true`. Before this, `cargo metadata` reported 51 of 52 crates missing both
  `repository` and `license`, and crates.io rejects a publish without them. `cargo metadata` is
  the check that actually tells the truth here, not reading the workspace manifest.
- **One version string, not forty.** Members declaring `ekos-plugin-file = { path = "../../plugins/file" }`
  directly would each have needed their own `version` too. All 16 such declarations (15 in `cli`,
  1 in `simulation`) were converted to `.workspace = true`, so a release bumps one block.
- **The topological order is validated, not assumed.** A DFS post-order over the real
  `cargo metadata` graph, then an explicit assertion that every crate appears after all of its
  dependencies. It also proved the graph is acyclic.
- **Nothing is near the size limit.** All 44 publishable crates together are 4.7 MB of tracked
  content; the largest single crate is `ekos-recovery` at 0.94 MB against a 10 MiB per-crate
  limit. Worth measuring rather than assuming — a fixture-heavy crate is the usual way this bites.
- **All 44 names are free on crates.io**, checked by exact lookup per name rather than by the
  fuzzy search endpoint, which returns near-matches (`eko`, `ekore`, `ekostd`) and would not have
  answered the question.

## Decisions

- **8 crates stay unpublished.** The five scaffolded connectors (Salesforce, SAP, Oracle, Fabric,
  Snowflake) are proofs of concept against mock API shapes that have never run against a live
  account — publishing them under the project's name would misrepresent them. `ekos-demo-server`
  is internal. `ekos-compiler-sdk` and `ekos-scheduler` are a genuine surprise: **nothing in the
  workspace depends on either**, despite `CLAUDE.md` describing them as the public extension API
  and the pass-scheduling primitives. They are marked `publish = false` with a note to confirm
  they are current first, rather than publishing something possibly vestigial — publishing cannot
  be undone, only yanked.
- **No LICENSE file added per crate.** `license = "MIT"` as an SPDX expression is what crates.io
  requires; a file per crate is convention, not a blocker, and 44 copies is churn.
- **No README per crate.** Only `ekos` gets one, because it is the page anyone will actually
  land on. The rest carry a description, which is what shows in listings.

---

## Knowledge Captured

- **A multi-crate publish cannot be fully dry-run.** `cargo package` for any crate whose internal
  dependencies are not yet on the registry fails with `no matching package named 'ekos-common'
  found / location searched: crates.io index` — the path is stripped and the version must already
  exist. Only the 4 crates with zero internal dependencies (`ekos-common`, `ekos-segment-backend`,
  `ekos-cluster`, `ekos-sql-dialect-sdk`) can be verified end to end before the real publish
  starts. That is a property of the registry, not a mistake to fix, and it is the strongest reason
  for the publish script to be resumable rather than all-or-nothing.
- **`cargo publish` already waits for index propagation** (since 1.66) and blocks until the new
  version resolves, so the `sleep 30` between publishes that older guides recommend is
  unnecessary. Adding one would only slow a 44-crate run down.
- **Check a name with `/api/v1/crates/<name>`, not the search endpoint.** Searching `ekos`
  returns `eko`, `ekore`, `ekostd` and 16 others, none of which answer "is this exact name free".
  The exact endpoint returns 404 for free and 200 for taken, and `/<name>/<version>` distinguishes
  "crate exists" from "this version exists" — which is what makes the publish script resumable.
- **`cargo package` strips dev-dependencies that carry no version.** `ekos-distributed` has a
  self-referential dev-dependency (`ekos-distributed = { path = ".", features = ["object-store"] }`)
  to exercise an optional feature in tests. That would be nonsense in a published manifest, and
  cargo removes it — no change needed, but it is worth knowing before it looks like a blocker.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/Cargo.toml` | internal deps carry `version`; 14 plugins moved into `[workspace.dependencies]` |
| 51 member `Cargo.toml` | workspace metadata inheritance; `publish = false` on 8 |
| `ekos/crates/cli/Cargo.toml` | real description, `readme`, workspace deps for 15 plugins |
| `ekos/crates/cli/README.md` | new — the crates.io landing page for `ekos` |
| `ekos/crates/simulation/Cargo.toml` | 1 path dep converted to workspace |
| `scripts/publish-crates.sh` | new — ordered, resumable publish runner |
| `ekos/docs/rfcs/0153-…md` | §6 corrected: 44 crates, not 30 |
| `TODO.md` | crates.io item updated with the real state |

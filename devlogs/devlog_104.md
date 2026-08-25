# Devlog 104 — RFC 0079 gap closed for `crate_topology_analyzer.rs`/`cicd_analyzer.rs`

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

`devlog_101` fixed RFC 0079's project-id qualification bug in `dependency_analyzer.rs`/
`package_json_analyzer.rs` but explicitly deferred the same fix for `crate_topology_analyzer.rs`
(`Cargo.toml`) and `cicd_analyzer.rs` (`.github/workflows/*.yml`), since `pdf-reader` — the only
live test project available that session — has neither Cargo manifests nor GitHub Actions
workflows to exercise the fix against. This entry closes that gap: same root cause, same fix
shape, verified live against a real multi-`[observe]-path` scenario built from EKOS's own
workspace (two real crates + the repo's real `.github/workflows`), since `pdf-reader` still can't
exercise this specific pair of analyzers.

## The fix

Same shape as `devlog_101`'s fix, applied to the two remaining collection loops in `recover.rs`
that still had the old bug:

- `recover.rs`'s `cargo_manifests` and `cicd_workflows` collection loops: `rel` now computed
  relative to `base` (not `cwd`), and a `project_key_for_base(base, cwd)` qualifier threaded
  through as each tuple's third element — both were still bare 2-tuples with no project awareness
  before this fix, unlike the two loops `devlog_101` had already converted.
- `crate_topology_analyzer.rs`: `manifests` widened to a 3-tuple; new `ParsedCrate.project` field;
  a `qualified_dir` closure applied only at the point real `KirId`s are minted
  (`crate_kir_id`/`architecture_gap_kir_id`/the three `add_depends_on_claim` call sites) — every
  internal directory comparison (`workspace_deps` path resolution, `dir_to_id` lookups) stays on
  the raw, unqualified directory, since a real Cargo path dependency only ever resolves within its
  own workspace, never across a project boundary. `technology_kir_id` deliberately left
  unqualified — external crates.io dependencies are global/shared across every observed project,
  not project-scoped the way a crate's own directory is.
- `cicd_analyzer.rs`: `workflows` widened to a 3-tuple; `pipeline_kir_id` now hashes the
  project-qualified path, `path` property and evidence text stay on the bare `rel_path`.
- `architecture_reasoning.rs`'s `seed_crates` test helper (constructs `CrateTopologyAnalyzerPass`
  fixtures for its own cross-pass tests) updated to the new 3-tuple shape.

2 new regression tests, mirroring `devlog_101`'s `dependency_analyzer.rs` precedent exactly: each
computes the real id `ekos_common::project::project_qualify` + the analyzer's own id-minting
function would produce for a qualified path, and asserts the pass's actual output object lands on
that exact id — not just "the pass runs without error."

## Live verification

`pdf-reader` has no Cargo.toml or CI workflows, so this fix needed a different real target. Built
a scratch `ekos.toml` (`[observe] paths` = two real absolute EKOS crate directories,
`crates/common` and `crates/kir`, plus the real `/home/legion/PycharmProjects/EKOS/.github/workflows`)
in a temp workspace outside the repo, ran the full `init`/`build`/`recover`/`resolve`/`compile`/
`commit` pipeline against it, then independently recomputed the expected ids in Python
(`uuid.uuid5`, same algorithm as Rust's `Uuid::new_v5`) from the real qualified-path formula and
confirmed they matched the real ledger objects exactly:

- `ekos-kir` `Crate` object: real id `1e3b2a23-162a-5165-9627-f6f9a5bb5212`, matches
  `uuid5(NAMESPACE_URL, "crate:<abs-path-to-crates/kir>:")` exactly.
- `CI` `Pipeline` object (from the real `ci.yml`): real id `ec56e494-03ad-5cf5-aadf-6dc4e204f351`,
  matches `uuid5(NAMESPACE_URL, "cicd-pipeline:<abs-path-to-.github/workflows>:ci.yml")` exactly.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups), `tests/integration` (3/3).

## Not fixed this session (unrelated, noted for later)

The scratch verification run reproduced the same `SEM002: unknown from-id`/`unknown to-id` warning
volume `devlog_101` flagged and left uninvestigated (1252 warnings on this run, mostly from
`rust_analyzer`'s `Calls` edges referencing symbols outside the deliberately narrow 2-crate observe
scope — expected given the scope, not re-investigated here). Still an open, separate item on the
gap list: "`compile.log`'s `SEM002` warnings firing on ids that actually resolve fine."

## Knowledge Captured

- **A real, addressable-but-inconvenient test scenario is still findable even when the "natural"
  test project doesn't have the right shape.** `pdf-reader` has no Cargo/CI surface, but EKOS's own
  real repo does — pointing a scratch `ekos.toml` at absolute paths outside the scratch workspace's
  own directory tree worked cleanly (`cwd.join(p)` on an already-absolute `p` replaces the whole
  path per `PathBuf::join`'s documented behavior), and deliberately using two *separate* absolute
  paths (rather than one shared parent) forced `project_key_for_base`'s `base != cwd` qualification
  to actually fire for both analyzers being tested, which a single shared-root scope would not have
  exercised.
- **Cross-checking a minted id against an independent implementation of the same hash formula (Python's
  `uuid.uuid5`, not just re-reading the Rust source) is a stronger live-verification signal than
  reading generated Markdown** — confirms the exact byte-for-byte input string the id-minting
  function actually hashed, not just that the pass "looks like it worked."

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/cli/src/commands/recover.rs` | `cargo_manifests`/`cicd_workflows` collection loops: base-relative path + `project_key_for_base` qualifier threaded through (previously bare 2-tuples) |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | `manifests`/`ParsedCrate` widened to carry project; `qualified_dir` closure applied at id-minting call sites only; 1 new regression test |
| `ekos/crates/recovery/src/cicd_analyzer.rs` | `workflows` widened to carry project; `pipeline_kir_id` now qualified; 1 new regression test |
| `ekos/crates/recovery/src/architecture_reasoning.rs` | `seed_crates` test helper updated to the new 3-tuple shape |

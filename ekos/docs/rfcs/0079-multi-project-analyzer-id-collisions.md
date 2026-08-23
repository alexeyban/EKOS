# RFC 0079 — Multi-Project Analyzer Id Collisions

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0044 fixed `build.rs`'s `File`-object id collision risk when `[observe] paths` lists more than
one entry (two projects sharing a same-relative-path file used to silently merge into one
`KirObject`). TODO.md tracked the same risk as still open for every downstream recovery pass that
derives an id from a raw path: `local_docs_analyzer.rs`, `rust_analyzer.rs`/`python_analyzer.rs`,
and `git_analyzer.rs`'s `CoupledWith`. `devlog_65` had already investigated this once and correctly
concluded it wasn't a same-shape copy-paste of `build.rs`'s fix — `project_key` only ever existed
as a transient local inside `build.rs`'s own loop, never persisted onto the `ObservationArtifact`s
these later passes read back with no project context at all.

## Design

Single choke point, `build.rs`, right next to RFC 0043's existing redaction pass (the same place
every connector's artifacts already pass through before persistence): when `[observe] paths` has
more than one entry, write a `"project"` field onto every artifact's `data` object — generic, not
connector-specific, so no per-observer code needed here. Absent entirely for the single-path case
(matching RFC 0044's own "existing single-project ledgers keep byte-identical ids" guarantee).

New `ekos_common::project::project_qualify(path, project) -> String` — the one shared piece every
consumer needs: `"{project}:{path}"` when `Some`, the bare path unchanged otherwise. A recovery pass
reads its own artifact's `data.project` back and calls this only where it computes an **id hash
input** — never where it builds a **displayed** name/label, which must stay the bare path.

Wired into `local_docs_analyzer.rs` (`document_kir_id`/`table_kir_id`/`section_kir_id`),
`rust_analyzer.rs` (`add_symbol`'s id, via `parse_rust_file`'s `path` argument — confirmed by
reading the whole function that `path` there is used *only* for id hashing, this crate emits no
evidence/display text from it at all), `python_analyzer.rs` (same, though here `path` is dual-use:
also `TransformOrigin.source_path`, a real displayed label — passed through qualified anyway, since
distinguishing which project's pipeline a sequence belongs to is a reasonable improvement, not a
regression, and a no-op for the single-project case), and `git_analyzer.rs`'s `CoupledWith` (each
commit artifact's own `data.project`, qualifying `sorted_files` used only for the coupling-pair
hash, not the commit event's own `files_changed` payload already captured for display beforehand).

## A real bug found live while verifying

The first implementation attempt only qualified the **file-level** id (`file_id`) and passed the
*bare* path into `parse_rust_file`/`parse_python_file`, reasoning that the `path` argument was used
for display. Live-verified against a real disposable two-project fixture (`service-a/src/lib.rs`
and `service-b/src/lib.rs`, both defining `fn handler()`) and found only **one** `handler`
`RustSymbol` existed after a full pipeline run — the fix hadn't actually worked. Traced it: `add_symbol`
computes its own id from `path` directly (`"rust-symbol:{path}:{name}"`), independent of `file_id`
— and this crate never uses `path` for anything else. Corrected by passing the qualified path
straight through; re-verified against the same fixture — two distinct `handler` ids, confirmed via
`ekl "FIND Object WHERE name = 'handler'"`.

## Scope — what this does and doesn't close

**Closes**: `local_docs_analyzer.rs`, `rust_analyzer.rs`, `python_analyzer.rs`,
`git_analyzer.rs`'s `CoupledWith` — the four analyzers with a clean one-artifact-to-one-project
relationship.

**Does not close**: `github_analyzer.rs`'s `file_kir_id` (used for `References` edges to files
mentioned in PR/issue bodies). This is a structurally different problem, not the same fix applied
late — a file path parsed out of free-text PR/issue description has no natural single
`[observe] paths` entry it belongs to at all (a GitHub repo isn't scoped to a local directory the
way file observation is); `file_kir_id`'s own doc comment already says it deliberately matches
`build.rs`'s *bare* scheme so `References` edges land on the same object `build.rs` produces — which
means, confirmed while investigating this, it's now silently **wrong** in a multi-project workspace
(the `File` object it's supposed to match is project-qualified; this one still computes the
unqualified id, so the edge points at an id that doesn't exist rather than colliding with the wrong
one). Recorded precisely in TODO.md as a distinct, still-open problem rather than left vague.

## Testing

- `ekos_common::project`: 3 unit tests (no-op without a project; qualifies with one; two different
  projects for the same path qualify differently).
- `local_docs_analyzer.rs`: 1 new test — a `project` field in the artifact data qualifies the
  `Document` object's id while leaving its displayed name bare.
- `rust_analyzer.rs`/`python_analyzer.rs`/`git_analyzer.rs`: existing test suites re-run unchanged
  (all pass) — no new isolated unit test added for these three specifically; correctness instead
  verified live end-to-end (below), which exercises `build.rs`'s injection and every consumer
  together, a stronger signal than an isolated mock for this particular cross-file mechanism.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end**: a disposable two-project fixture (`service-a`, `service-b`, each with
  a same-named, same-content-shaped `src/lib.rs` defining `fn handler()`), full pipeline, confirmed
  two distinct `handler` `RustSymbol` ids via `ekl`. `git_analyzer.rs`'s `CoupledWith` path was
  code-reviewed and unit-tested but not separately live-verified end-to-end (time-boxed) — the
  underlying mechanism (`project_qualify` applied before hashing) is identical to what was proven
  live for the Rust case.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0079-multi-project-analyzer-id-collisions.md` | This RFC |
| `ekos/crates/common/src/project.rs` | New: `project_qualify`; 3 tests |
| `ekos/crates/common/src/lib.rs` | `pub mod project;` |
| `ekos/crates/cli/src/commands/build.rs` | Writes `data.project` at the RFC 0043 choke point when multi-path |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | `DocumentData.project`; id hashing qualified; 1 new test |
| `ekos/crates/recovery/src/rust_analyzer.rs` | `RustArtifactData.project`; qualified path threaded through `parse_rust_file` |
| `ekos/crates/recovery/src/python_analyzer.rs` | `PythonArtifactData.project`; qualified path threaded through `parse_python_file` |
| `ekos/crates/recovery/src/git_analyzer.rs` | `CoupledWith` file-pair hashing qualified per-commit |
| `TODO.md` | Item updated: four analyzers closed; `github_analyzer.rs`'s distinct, now-more-precisely-understood problem still open |
| `devlogs/devlog_82.md` | This increment's devlog |

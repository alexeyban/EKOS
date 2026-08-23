# Devlog 82 — Multi-project analyzer id collisions (RFC 0079)

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Third item of the gap-closing pass. `devlog_65` had already investigated this once and correctly
concluded it needed real cross-cutting plumbing, not a copy-paste of `build.rs`'s existing
`File`-object fix. Built that plumbing: one small shared utility, one choke-point write in
`build.rs`, four analyzers updated to read it back.

## RFC 0079

`project_key` (RFC 0044) only ever lived as a transient local inside `build.rs`'s scan loop — never
persisted onto the artifacts later recovery passes read back from `artifact_store` with zero
project context. Fixed by writing a `"project"` field onto every artifact's `data` object at the
same choke point RFC 0043's redaction already uses, and a tiny shared `project_qualify` helper in
`ekos-common` every consuming pass calls only where it computes an id hash — never where it builds
a displayed name.

**A real bug found live, not just theorized**: the first pass at `rust_analyzer.rs` only qualified
the file-level id and passed the *bare* path into the actual parsing function, assuming `path` was
used for display there. A live two-project fixture test (`service-a`/`service-b`, each with an
identically-shaped `fn handler()`) showed only one `handler` object existed — the fix hadn't worked.
Traced it: the symbol-id function hashes `path` directly, and this crate doesn't use `path` for
display at all. Fixed by passing the qualified path all the way through; re-verified live — two
distinct ids.

**Honestly scoped, not closed**: `github_analyzer.rs`'s `file_kir_id` (for `References` edges to
files mentioned in PR/issue text) turns out to be a structurally different problem — a path parsed
from free text has no single `[observe] paths` entry it naturally belongs to. Investigating it
found it's now *silently wrong*, not just collision-prone, in a multi-project workspace (it still
computes the bare-path id, which no longer matches `build.rs`'s own qualified `File` object) —
recorded precisely as its own distinct open item rather than left as a vague leftover.

## Knowledge Captured

- **A live end-to-end test catches what code review alone won't**: the first `rust_analyzer.rs` fix
  looked correct on inspection (file id qualified, `parse_rust_file` called) — it just qualified
  the wrong thing. Running the real disposable fixture and directly querying the result (not
  trusting "the code looks right") caught this before it shipped.
- **A "display vs. id" split isn't universal across analyzers with the same apparent shape** —
  `rust_analyzer.rs` had a clean split (id-only usage); `python_analyzer.rs`'s `path` is genuinely
  dual-purpose (also a real displayed pipeline label). Treating both the same way without checking
  would have either broken Rust's display (nothing to break, but wasted effort) or missed Python's
  id fix entirely.
- **Investigating a "same fix elsewhere" TODO item can find the target is more broken than
  described** — `github_analyzer.rs`'s `file_kir_id` wasn't just theoretically collision-risky, it
  was already producing dangling references in exactly the multi-project case `build.rs`'s own
  earlier fix created. Worth flagging precisely rather than filing it under the same "still open"
  bucket as the others.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0079-multi-project-analyzer-id-collisions.md` | New RFC |
| `ekos/crates/common/src/project.rs` | New shared helper; 3 tests |
| `ekos/crates/common/src/lib.rs` | Module registration |
| `ekos/crates/cli/src/commands/build.rs` | `data.project` injection at the RFC 0043 choke point |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | Id qualification; 1 new test |
| `ekos/crates/recovery/src/rust_analyzer.rs` | Id qualification (fixed after live-test failure) |
| `ekos/crates/recovery/src/python_analyzer.rs` | Id qualification |
| `ekos/crates/recovery/src/git_analyzer.rs` | `CoupledWith` id qualification |
| `TODO.md` | Item updated: four analyzers closed, `github_analyzer.rs` re-scoped precisely |
| `devlogs/devlog_82.md` | This file |

# Devlog 81 — Identity-resolution over-merge: the `Technology`/`Crate` half (RFC 0078)

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Second item of the gap-closing pass. TODO.md's own instruction was explicit: fix `ripgrep`/`bat`'s
identity conflicts structurally, not with another kind-exclusion entry. Traced the real mechanism
and found the bug wasn't in `identity`'s resolver at all — it was upstream, in
`crate_topology_analyzer.rs` manufacturing a spurious duplicate object.

## RFC 0078

`ripgrep`'s `ignore`/`pcre2` and `bat`'s own `bat` crate are each a real internal workspace member
*and* depended on elsewhere in the same workspace by a bare version string instead of
`path`/`workspace = true`. `crate_topology_analyzer.rs` classified that version-pinned reference
purely from TOML shape and always manufactured a `Technology` object for it — even when a real
`Crate` object with the identical name already existed from the same scan. `identity`'s conflict
detector then correctly reported the collision it was shown; the recovery-side data was wrong, not
the resolver's judgment.

Fixed at the source: check the dependency name against every already-known internal crate name
before falling back to fabricating a `Technology`. Verified with a fixture reproducing the exact
real `ripgrep`/`bat` shape.

**Honestly left open**: the `RustSymbol`/`Crate` half of the same finding (a module/type inside a
crate's own source sharing that crate's name — normal Rust convention, not a bug) has no existing
relationship in the graph connecting a `Crate` to its own source symbols to structurally
distinguish "legitimate self-naming" from "coincidental collision." Building that link is real,
separate work — not guessed at with a name heuristic that could mask an actual conflict.

## Knowledge Captured

- **A resolver correctly reporting a conflict doesn't mean the resolver is broken** — the
  `SameNameDifferentKind` detector did exactly its job; the bug was in what it was asked to
  compare. Worth checking upstream data quality before assuming the comparison logic is wrong.
- **"Structural fix, not another exclusion" is a real, checkable design constraint, not just a
  preference** — the fix that resulted touches zero resolver code and instead corrects the one
  recovery pass that had incomplete information (it never checked its own crate list before
  deciding something was external).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0078-crate-vs-technology-name-collision.md` | New RFC |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | `name_to_crate_id` check before fabricating a `Technology`; 1 new test |
| `TODO.md` | Item updated: half closed, half honestly re-scoped |
| `devlogs/devlog_81.md` | This file |

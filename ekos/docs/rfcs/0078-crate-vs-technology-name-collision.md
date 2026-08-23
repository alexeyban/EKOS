# RFC 0078 — Version-Pinned Internal Dependencies No Longer Fabricate Duplicate Technologies

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

TODO.md carried a real, previously-found (RFC 0045's repo-selection spike, `devlog_45`), still-open
gap: `BurntSushi/ripgrep` and `sharkdp/bat` both hit identity-resolution `SameNameDifferentKind`
conflicts for their own crate names (`pcre2`, `ignore`, `bat` itself) — flagged as
`RustSymbol`/`Technology`/`Crate` simultaneously. Explicitly asked for a structural fix to
`identity`'s resolver, not another one-off kind-exclusion entry.

## Root cause (the `Technology`/`Crate` half of it)

`crate_topology_analyzer.rs`'s `resolve_dep_entry` classifies a `[dependencies]` entry purely by
its TOML shape: a bare version string (`ignore = "0.4"`) becomes `DepResolution::Version`, a table
with `path = ...` becomes `DepResolution::Path`. `Version` always manufactures a `Custom("Technology")`
object for that name; `Path` always resolves to an internal `Custom("Crate")` via `DependsOn`.

Both `ripgrep` and `bat` have a real internal crate (`ignore`, `pcre2` — `pcre2` is `ripgrep`'s own
regex-engine binding crate; `bat`'s own crate is literally named `bat`) that is *also* depended on
elsewhere in the same workspace via a bare version string rather than `path`/`workspace = true` —
so the analyzer manufactured a spurious duplicate `Technology` object sharing the real `Crate`'s
exact name. `identity`'s cross-kind conflict detector then correctly reported what it saw: the same
normalized name really did appear as two different object kinds — it just didn't know that was a
recovery-side classification bug, not two independent real-world facts colliding by coincidence.

## Fix

Structural, at the source, not in the resolver: before creating a `Technology` object for a
version-pinned dependency, check whether its name exactly matches an already-known internal
crate's own name (`name_to_crate_id`, built once from the same workspace scan already collecting
every crate). If it does, treat it as the real internal `DependsOn` edge it actually is — same
`Claim`/evidence construction the `Path` branch already does — instead of fabricating a duplicate.
`identity`'s resolver itself needed no changes: the conflict it was reporting was real given its
input; the input was wrong.

## Scope — what this does and doesn't close

**Closes**: the `Technology`/`Crate` half of the reported conflict — the mechanically
well-understood, concretely reproducible half (verified with a fixture matching `ripgrep`/`bat`'s
exact real shape: an internal crate also depended on by version elsewhere in the workspace).

**Does not close**: the `RustSymbol`/`Crate` half (a module or type inside a crate's own source
sharing that crate's name — e.g. a `pcre2` crate containing a `mod pcre2` or `struct Pcre2Error` —
is a completely normal, legitimate Rust naming convention, not a bug, but the conflict detector
still flags it). Investigated a structural fix here too: the natural signal would be "does this
`RustSymbol`/`RustModule` actually live inside a file this `Crate` owns" — but no relationship in
the graph currently connects a `Crate` to its own source `File`/`RustModule`/`RustSymbol` objects at
all (Component View, RFC 0070, only matches `Crate`↔`Rollup` by path-string equality, not a real
relationship) — there's no existing structural data to hang a precise fix on without first building
that missing link. Left open, honestly, as separate, real, tracked work rather than guessed at with
a name-based heuristic that would risk suppressing a genuine unrelated collision.

## Testing

- New fixture test in `crate_topology_analyzer.rs` reproducing the exact real pattern: an internal
  crate named `ignore`, plus a second crate depending on it via a bare version string. Confirms
  exactly one `ignore` object exists (the real `Crate`, not a duplicate `Technology`), and that the
  version-pinned dependency still resolves to a real `DependsOn` edge against it.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0078-crate-vs-technology-name-collision.md` | This RFC |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | `name_to_crate_id`; version-pinned dependency checked against it before falling back to `Technology`; 1 new test |
| `TODO.md` | Item updated: `Technology`/`Crate` half closed; `RustSymbol`/`Crate` half re-scoped honestly as still-open, blocked on a missing `Crate`→source relationship |
| `devlogs/devlog_81.md` | This increment's devlog |

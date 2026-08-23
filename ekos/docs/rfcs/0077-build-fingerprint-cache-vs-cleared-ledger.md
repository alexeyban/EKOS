# RFC 0077 — `ekos build`'s Fingerprint Cache vs. a Cleared Ledger

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

TODO.md carried a real, previously-found-live, not-yet-root-caused gap: clearing `.ekos/ledger/`
while keeping the artifact cache and `fingerprints.json` intact, then running
`recover → resolve → compile → commit`, reproduced zero `File`-kind `KirObject`s — even though the
same real file paths were searchable before the clear. This RFC root-causes and fixes it.

## Root cause

`ekos build`'s per-observe-path loop (`crates/cli/src/commands/build.rs`) does two things per
path: (1) writes every observer's raw content to the content-addressed `artifact_store`, and (2),
only for the `file` observer specifically, constructs `ObjectKind::File` `KirObject`s inline and
appends them straight to the ledger. Unlike `recover`-stage analyzer output — which is written as a
replayable `KnowledgeArtifact` that `compile` re-derives from `artifact_store` fresh on every
invocation, independent of any cache — this inline `File`-object construction is gated entirely by
a source-fingerprint check:

```rust
if fingerprints.get(&fp_key) == Some(&fp.0) {
    connectors_skipped_cached += observers.len();
    continue;  // skips the whole per-path block, including File-object construction, entirely
}
```

The fingerprint answers "has anything on disk changed since last time" — a real, valid question for
deciding whether to re-invoke `observer.scan()` (I/O-bound, sometimes network-bound for git/GitHub/
Confluence/ClickHouse). It says nothing about whether the *ledger* still has the objects that scan
would have produced. If the ledger gets cleared independently of the source content, the fingerprint
still matches, the whole block still gets skipped, and the ledger stays empty of `File` objects
indefinitely — until the source content itself changes.

## Fix

`ledger.object_count() == 0` is a cheap, always-correct signal that the ledger was just cleared (or
never populated). When true, no fingerprint is trusted for the whole `run()` invocation — every
observe path is force-rescanned once, repopulating the ledger. On every subsequent run (ledger no
longer empty), the fingerprint cache resumes working exactly as before.

**Deliberately not covered**: the far rarer case of *other* object kinds surviving in the ledger
while `File` objects specifically were somehow selectively removed (not a full ledger clear). That
is not the scenario that was found live, and reproducing it would need either a much more precise
per-kind existence check or an artifact-replay mechanism — both real, larger changes not justified
by anything actually observed. The fix's scope matches the reported bug exactly.

## Testing

- Two new `#[tokio::test]`s in `build.rs` (its first test module — none existed before):
  1. Real end-to-end reproduction: `run()`, clear only `.ekos/ledger/`, `run()` again with
     unchanged source content — `File` objects must be non-zero both times.
  2. Regression guard the other direction: `run()` twice against an *intact* ledger with unchanged
     content must not change the total object count — confirms the fix doesn't defeat the cache
     entirely, only bypass it when the ledger looks freshly cleared.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0077-build-fingerprint-cache-vs-cleared-ledger.md` | This RFC |
| `ekos/crates/cli/src/commands/build.rs` | `ledger_is_empty` check gates fingerprint trust; 2 new tests (first test module in this file) |
| `TODO.md` | Item marked done |
| `devlogs/devlog_80.md` | This increment's devlog |

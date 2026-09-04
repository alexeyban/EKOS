# Devlog 158 — RFC 0135 Part A: pipeline logic version in the build fingerprint

**Date:** 2026-09-04
**Branch:** `rfc/0135-core-provenance-determinism` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0135-core-provenance-and-determinism-foundations.md` (Part A of 4)

---

## Summary

`ekos build` skips re-scanning an observe path whose source files are byte-for-byte unchanged
(`.ekos/fingerprints.json`, keyed by observe-path). That fingerprint is `(path, size, mtime)`
only — it never knew when *EKOS's own* redact/analyze code changed. So a fix to
`ekos_common::redaction` or an analyzer had no effect on an unchanged file until a full `.ekos`
wipe — the exact situation that led to the destructive-command incident in `devlog_100`, and the
content-addressing staleness in `devlog_112`.

Part A closes the code-change half **and** picks up the config-change half for free: the
fingerprint **cache key** now folds in `PIPELINE_LOGIC_VERSION` (a hand-bumped `u32` in
`ekos_common`) and an 8-hex hash of the workspace's `RedactionConfig`. A change to either misses
the cache and forces exactly one real re-scan of that path; the re-scan re-derives artifact ids
from post-redaction content (RFC 0072's fix), so genuinely-changed artifacts get persisted.

---

## PR — Part A

### What changed

| File | Change |
|---|---|
| `ekos/crates/common/src/lib.rs` | New `pub const PIPELINE_LOGIC_VERSION: u32 = 1` with the bump-discipline doc comment + changelog block |
| `ekos/crates/cli/src/commands/build.rs` | New `fingerprint_cache_key(base, logic_version, redaction_config)` → `<abs base>@v<n>#<cfg8>`; `run()` uses it in place of the bare `base.display()` key; 2 new tests, 1 existing test simplified |

### Key shape

```
/abs/path/to/observe/base@v1#a1b2c3d4
                      │    │
                      │    └─ first 8 hex of sha256("{RedactionConfig:?}")
                      └────── PIPELINE_LOGIC_VERSION
```

The absolute base path stays in the key (RFC 0044 multi-`[observe]`-path workspaces). Only the
map **key** changed; the map **value** (the `source_fingerprint` hash) is untouched. No migration
— an old bare-path key simply misses on the first run after upgrade (one re-scan), then the file
is rewritten with versioned keys.

### `preview-scan` untouched

`ekos config preview-scan` answers "what would `build` observe" and must stay a pure function of
the source tree + `[observe]` config. The logic version lives *only* in `build.rs`'s cache-key
string, nowhere in `walk_observed` or any output.

---

## Knowledge Captured

- **The redaction config was already knowable at build time** — folding
  `hash("{RedactionConfig:?}")` into the key means a `[security]` `ekos.toml` edit re-scans on its
  own, no `PIPELINE_LOGIC_VERSION` bump needed. The constant is now *only* for changes to EKOS's
  own code. This is strictly better than the RFC's original "just a const" proposal and was nearly
  free to add.
- **`build.rs`'s `a_later_redaction_pattern_addition_actually_re_redacts_unchanged_source` test
  (from `devlog_112`) used to need a `remove_dir_all(ledger)` to force the rescan** — a tell that
  the only re-scan trigger was "ledger looks empty" (RFC 0077). With the config hash in the key
  that crutch is gone; the test now exercises the real path (config change → key miss → rescan)
  with the ledger left intact.
- **`PIPELINE_LOGIC_VERSION` is a manual bump** — deliberately. A content hash of the relevant
  source files would never be forgotten but makes every dev build a cache miss. The doc comment
  enumerates exactly what counts (`redaction`, any `Observer::scan`, `walk_observed`, the inline
  `File`-object construction) and carries a changelog. Revisit only if a bump is missed in
  practice.
- **`fingerprints.json` has exactly one reader/writer** (`build.rs`) — grep-confirmed. The key
  format is fully contained; no other code parses that file.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/common/src/lib.rs` | `PIPELINE_LOGIC_VERSION` const + doc/changelog |
| `ekos/crates/cli/src/commands/build.rs` | `fingerprint_cache_key` helper; `run()` wired to it; `fingerprint_cache_key_changes_with_logic_version_and_redaction_config` + `a_logic_version_bump_forces_a_rescan_of_unchanged_source` tests; `a_later_redaction_pattern_addition…` test dropped its ledger-clear |
| `ekos/docs/rfcs/0135-…md` | Part A marked implemented |
| `TODO.md` | RFC 0135 Part A ticked |

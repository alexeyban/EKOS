# Devlog 243 — Source freshness: does the ledger still match the source? (RFC 0171)

**Date:** 2026-10-09
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

EKOS had no way to say that the ledger was behind the code. `ekos build`'s fingerprint is a cache that skips silently, the MCP server only watches the ledger, and every existing "drift" feature compares ledger against ledger. RFC 0171 records what the ledger was compiled from: a per-file (path, size, mtime) manifest, written by `build` and promoted by a successful `commit`. EKOS then compares it with the disk in `ekos status`, `ekos doctor`, a new `ekos freshness` command, a `FRESH001` warning at compile/commit, and over MCP. On LedgerSMB, touching one file flags the 167 compiled objects that cite it.

---

## What was built

| Component | Change |
|---|---|
| `observation-sdk` | `FileStamp`, `source_manifest(ctx)`, `fingerprint_of(&[FileStamp])`; `source_fingerprint` is now `fingerprint_of(source_manifest(..))`. A test pins the hash to the old tuple-based one |
| `cli/src/freshness.rs` (new) | `SourceManifest` (workspace-relative keys, git HEAD), `diff`, `scan`, `record_build`, `promote_after_commit`, `changed_since_build` (`FRESH001`), `check` → `Freshness { status: fresh / source_changed / unknown, … }`, `citing_objects` |
| `commands/build.rs` | one walk gives fingerprint and manifest; writes `.ekos/source-manifest.json` |
| `commands/commit.rs`, `compile.rs` | `FRESH001` warning; commit promotes to `.ekos/committed-manifest.json` |
| `commands/freshness.rs` (new), `app.rs` | `ekos freshness [--json] [--limit N]` |
| `commands/ledger.rs`, `doctor.rs` | `status` "Source :" line and a `freshness` object in `--json`; doctor "Source freshness" check (always `ok`) |
| `commands/mcp.rs` | `StoreCache::freshness` (TTL memo), `ekos_status` block, `tool_ok_with_freshness` note on read results |
| `compiler-core/src/config.rs` | `[freshness] enabled = true, ttl-seconds = 30` |
| `ekos/docs/rfcs/0171-source-freshness.md` | the RFC |

## Measured

- **Small workspace:** after a full pipeline it is `fresh`. Editing `schema.sql`, adding `new.py` and removing `app.py` gives 1 changed / 1 added / 1 removed. `may be stale` lists `Table customers, Table orders` for the edit and `PythonSymbol total` for the removal. `compile` warns `FRESH001`. A rebuild makes it `fresh` again, and a bare `touch` counts as changed.
- **LedgerSMB** (demo workspace, re-run with this build, pipeline 283 s): `fresh` at git `544bcd9`. After `touch sql/modules/Company.sql`, 167 objects cite it: 110 `ProcedureStatement`, 46 `Procedure`, 5 `RationaleLink`, 3 `EnumMeaning`, 2 `BusinessConcept` and the `File`. `ekos status` takes 2.2 s; `ekos freshness` takes 25.7 s in debug, because `citing_objects` reads ~11k evidence records one by one.

## Decisions

- **Metadata, not content.** Hashing every file on every check would cost more than the check is worth. An mtime-only false positive only suggests a rebuild, and the output states the rule. A hash fallback for same-size, new-mtime files is a TODO.
- **Promote at commit, not build.** A built-but-uncommitted tree is not in the ledger. Until the first commit after RFC 0171 the status is `unknown`, never a guess.
- **Doctor stays `ok`.** A ledger behind the source is not a broken environment; the detail carries the drift and points to `ekos freshness`.
- **The MCP note is a separate content item.** The tool's own result is byte-identical, so clients that parse `content[0]` are unaffected. `ekos_status` gets the structured block instead of a note.
- **No per-object staleness inside every MCP result.** `citing_objects` scans all evidence, which is fine on request (`ekos freshness`) but not on every call.

---

## Knowledge Captured

- **EKOS's own state directory must be excluded explicitly.** `.ekos` is in the default `ignore-patterns`, but a config that overrides them without it would make every commit stale the moment the manifests are written. `freshness::without_own_state` drops `config.ekos_dir()`; a test covers it.
- **Evidence paths are workspace-relative, observe paths may not be.** Manifest keys are built as `prefix_for(base, cwd)` + the file's base-relative path, so they match `KirEvidence.location.path` for `paths = ["."]` and for sub-paths alike.
- **`citing_objects` costs one `get_evidence` per evidence id** (no bulk evidence read on `KnowledgeStore`). That is fine for a command and too slow for per-call MCP use. If it ever moves to a hot path, add a bulk read rather than caching.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/observation-sdk/src/lib.rs` | `FileStamp`, `source_manifest`, `fingerprint_of`; hash-pinning test |
| `ekos/crates/cli/src/freshness.rs` | new: manifest, diff, check, citing objects; 8 tests |
| `ekos/crates/cli/src/commands/freshness.rs` | new: `ekos freshness` |
| `ekos/crates/cli/src/commands/{build,commit,compile,ledger,doctor,mcp}.rs`, `app.rs`, `lib.rs`, `commands/mod.rs` | wiring; MCP test `freshness_block_and_note_follow_the_source` |
| `ekos/crates/compiler-core/src/config.rs` | `[freshness]`; defaults test |
| `ekos/docs/rfcs/0171-source-freshness.md` | new RFC |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | docs |

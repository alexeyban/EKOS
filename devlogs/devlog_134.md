# Devlog 134 — RFC 0113: publishing the partition manifest (self-describing object-storage partitions)

**Date:** 2026-08-30
**PRs:** the commit landed with this devlog
**Branch:** main (direct)

---

## Summary

Devlog 133 routed a partition's **sealed segments** (its bulk) through a `SegmentBackend`, but
`manifest.json` and `dict.bin` still lived only on the local root — so a distributed cluster
needed those metadata dirs on shared storage. This closes that: `manifest.json` and `dict.bin`
now publish through the backend too. **A partition is now self-describing in object storage** — a
reader opens it from its URL alone; only `HEAD` (the active-segment watermark) and the active
unsealed segment stay local to the writer, plus tantivy's `search/` dir (still a follow-on).

---

## What was built

### `SegmentBackend::publish`

`publish_sealed(key, staged_path)` is for write-once immutable segments. The manifest is
rewritten on every seal, so it needs a distinct verb:

```rust
/// Publish (or overwrite) a small mutable metadata object — manifest.json, dict.bin.
fn publish(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError>;  // has a default
```

| impl | `publish` |
|---|---|
| `LocalFsBackend` | atomic write (temp + fsync + rename + dir fsync) to `<root>/<key>` — byte-identical to the old `atomic_write` the manifest used |
| `ObjectStoreBackend` | `PUT` + drop any stale cache copy |
| `MemBackend` | insert into the map |
| default | stage to a temp file, hand to `publish_sealed` (only test doubles hit this) |

### `SegmentStore`

- `save_manifest` / `set_dictionary` → `backend.publish("manifest.json" | "dict.bin", bytes)`
  instead of a local `std::fs::write` / `atomic_write`.
- `open_with_backend` → `load_manifest(backend)` uses `backend.exists("manifest.json")` +
  `backend.get(...)`; `dict.bin` via `backend.get(...)`.
- `HEAD` and the active segment: **unchanged**, always local.
- For the default `LocalFsBackend`, all of this is the same bytes in the same place as before —
  a pure indirection.

### Result

`FactLedger::open_read_only_with_backend` on a workspace whose local `manifest.json` / `dict.bin`
/ `HEAD` / `segments/` have all been deleted still reads every object back — the segment store
reconstructs entirely from the backend. (`search/` still has to be local — `SearchIndex::
open_read_only` errors on a missing `search/` dir; publishing or rebuilding it is the remaining
piece for a fully portable query-worker cache.)

---

## Knowledge Captured

- **`publish_sealed` vs `publish`.** Overloading `publish_sealed` for the mutable manifest would
  have muddied its write-once contract (and `LocalFsBackend::publish_sealed` literally assumes
  "the staged file already lives at the key path"). A separate `publish(key, bytes)` with a
  conservative default keeps both honest.
- **A flaky architecture test is not a regression.** `diff_on_an_unchanged_workspace_reports_empty`
  failed once under full-workspace parallel load, then passed 3× in isolation and 3× in the full
  cli suite. It captures `t1`/`t2` 2 ms apart right after an append; under load the append's
  internal `Utc::now()` tx timestamp can land after `t1`. Pre-existing timing fragility (same
  class as `diff_reports_a_real_technology_added_between_two_commits`), unrelated to this change.
- **`LocalFsBackend::exists` / `get` already do the right thing for the manifest** — rooted at the
  partition dir, `exists("manifest.json")` is exactly the old `path.exists()` check and `get` is
  the old `std::fs::read`. So the default path needed no special-casing.
- **`HEAD` genuinely can't move to the backend.** It's the crash-recovery watermark for the
  *active* (growing, truncatable) segment, which is inherently local. A read-only opener with no
  `HEAD` is fine — `SegmentStore::open_with_backend` skips the `HEAD`↔manifest consistency check
  when `HEAD` is absent.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/segment-backend/src/lib.rs` | `SegmentBackend::publish` (trait + default); `LocalFsBackend::publish` (atomic write); `MemBackend::publish`; doc update |
| `crates/segment-backend/src/object_store_backend.rs` | `ObjectStoreBackend::publish` (PUT + cache invalidate) |
| `crates/ledger/src/segment/mod.rs` | `load_manifest` / `save_manifest` take `&dyn SegmentBackend`; `dict.bin` via backend; `persist_manifest` / `seal_active` / `set_dictionary` route through `self.backend`; module + field docs; `CountingBackend` + `sealed_io_routes…` test updated |
| `crates/ledger/src/fact_ledger.rs` | `sealed_segments_are_served_from_the_backend_not_local_disk` now also wipes `manifest.json`/`dict.bin`/`HEAD` and still reads back |
| `ekos/docs/rfcs/0113-…md`, `0111-…md` | manifest-publishing acceptance; partition self-describing in object storage |
| `TODO.md`, `README.md` | self-describing object-storage partitions |

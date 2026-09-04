# Devlog 162 — RFC 0135 Part B follow-up: per-object provenance through `compile`

**Date:** 2026-09-04
**Branch:** `rfc/0135-provenance-compile` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0135-core-provenance-and-determinism-foundations.md` (Part B follow-up)

---

## Summary

`devlog_160` shipped RFC 0135 Part B — `ekos ledger audit`/`ekos_audit` show the write history of
any object or relationship, but for anything that passed through `compile` the "source artifact"
was the *run-level* CKM content hash, not the actual `KnowledgeArtifact`(s) it was recovered from.
Every object in a `commit` run looked identical: `ckm:<one hash>`. This closes that gap —
`compile` now threads real per-object provenance from `recover` output through identity resolution
into the CKM, and `commit` stamps each ledger write with it.

---

## PR — Part B follow-up

### Problem / motivation

`ckm:<hash>` answered "which compile run produced this" but not "which recovered artifact(s) this
specific object actually came from" — the thing an audit trail exists to answer. A `Table` merged
from two source systems by identity resolution should show both originating artifacts; a
`concentration_risks`-derived `Risk` object (RFC 0094, synthesized inside `compile` itself, no
recovered artifact at all) legitimately has none.

### What was built

| Component | Change |
|---|---|
| `ekos-semantic::CkmObject`/`CkmRelationship` | new `source_artifact_ids: Vec<String>` field (`#[serde(default, skip_serializing_if = "Vec::is_empty")]` — no format bump, old CKMs deserialize with an empty vec) |
| `ekos-semantic::build_ckm_with_provenance` | new fn — `build_ckm` (old signature, still used by every existing caller) is now a thin wrapper passing an empty provenance map |
| `SemanticCompilerPass::run` | builds `KirId → BTreeSet<KnowledgeArtifact id>` while reading each artifact's KIR graph; folds a merged-away object's provenance into its canonical id *before* `apply_merges` runs; passes the (sorted, `Vec`-ified) map to `build_ckm_with_provenance` |
| `commit.rs` | `per_source_ctx(&ckm_obj.source_artifact_ids)` sets a `WriteContext` per object/relationship write — `Some("ka:<id,id,...>")` when non-empty, else the existing run-level `ckm:<hash>` fallback |

### Implementation details worth remembering

- **Fold provenance into the canonical id before, not after, `apply_merges`.** Identity resolution
  can merge two objects (each from a different `KnowledgeArtifact`) into one canonical id; the
  losing id's KIR object disappears in `apply_merges`. Folding provenance first means the surviving
  canonical id inherits the union of both artifacts' ids — a merged `Table` shows every source it
  was compiled from, not just the one whose id happened to win.
- **`BTreeSet` while accumulating, `Vec` at the boundary.** Sorted + deduped for free during the
  per-artifact/per-merge accumulation; converted to a plain sorted `Vec<String>` only once, at the
  `build_ckm_with_provenance` call — keeps `CkmObject`/`CkmRelationship` (the serialized/public
  type) simple.
- **Two-tier fallback, not a hard requirement.** An object with real recovered provenance gets
  `ka:<ids>`; a `compile`-synthesized object (rollup, concentration risk) or an object from a
  pre-0135 CKM with no map entry gets the old run-level `ckm:<hash>` — `commit`'s
  `set_write_context` is called per-write, so this falls out of `per_source_ctx` naturally rather
  than needing a special case.

### Decisions (alternatives considered, why this choice)

- **Field on `CkmObject`/`CkmRelationship`, not a side map threaded separately into `commit`.** The
  CKM (`model.json`) is already the one artifact `compile` writes and `commit` reads back — adding
  the field there means `commit` doesn't need to re-derive or re-read anything from `compile`'s
  intermediate state, and the provenance survives a `compile`-then-inspect-then-`commit` gap on
  disk for free.
- **No `.ekos` wipe, no CKM schema bump.** `skip_serializing_if = "Vec::is_empty")]` on a
  `#[serde(default)]` field is fully backward/forward compatible — an old `model.json` without the
  field deserializes with an empty vec (correctly falls to the run-level hash), and old code
  reading a new `model.json` ignores the unknown field. Same additive-schema pattern Part B itself
  used for the SQLite `entries` columns.

---

## Knowledge Captured

- **A CKM object's "true" provenance is a set-union problem, not a single value**, because identity
  resolution is a many-to-one merge. Any future per-object provenance field on a compiled/resolved
  type should default to "accumulate via the merge path," not "carry forward the winner's value" —
  the latter silently drops information for every merged object, which is exactly the case an audit
  trail most needs to get right.
- **`build_ckm` staying a thin wrapper over `build_ckm_with_provenance`** (rather than changing
  `build_ckm`'s signature and updating every caller) kept this change to two touched files outside
  its own crate. Worth reaching for the "old fn becomes wrapper over new fn with a default" shape
  again when a widely-called pure function needs one new optional input.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/semantic/src/lib.rs` | `source_artifact_ids` field on `CkmObject`/`CkmRelationship`; `build_ckm_with_provenance`; provenance tracking + merge-folding in `SemanticCompilerPass::run`; 2 new tests + existing dangling-relationship tests updated for the new field |
| `ekos/crates/cli/src/commands/commit.rs` | `per_source_ctx` closure; per-object/relationship `set_write_context` calls; 1 new end-to-end test (`commit_stamps_per_object_source_artifact_provenance`) |
| `README.md`, `docs/generated/ekos-self-documentation.html` | `ekos_audit`/`ekos ledger audit` descriptions updated to explain the per-object vs. run-level fallback |
| `TODO.md` | Part B item's follow-up sub-bullet marked done |

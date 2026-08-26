# RFC 0004 — Semantic Knowledge Ledger

**Status:** Accepted (retroactively documented)
**Date:** 2026-08-26 — the design below was actually built starting around 2026-07-04 (RFC 0005,
which already assumes this ledger exists, is dated then); this document was never written at the
time. Found missing during a Phase -1 through 13 audit: every later ledger RFC (0009, 0015, 0016,
0047, 0096, 0097, 0104-0106…) references "RFC 0004's design" as an established foundation that, in
fact, had no RFC document behind it — only the shipped code. Written now to close that gap in the
historical record, describing the design as it was actually built (Phase 9's own real output),
not as a fresh proposal.

---

## Problem

`TODO.md`'s Phase 9 planning (written before this RFC existed) needed a real design decision before
implementation could start: what does "append-only, single source of semantic truth" actually mean
as a storage engine? Snapshot storage vs. event sourcing, which backend, how time-travel queries
work, how integrity is verified.

## Solution — what was actually built

**Backend**: SQLite, behind a small trait seam (later formalized as `KnowledgeStore`, RFC 0009) —
explicitly the "disposable, swappable first backend" Phase 9's own text called for. One physical
table holds the append-only log; two extra tables that shipped later (0015, 0016) point *at* rows
in that log rather than duplicating content.

**The append-only log**: `entries(id, entry_type, payload, content_sig, written_at)` — every write
is a new row, never an update or delete. `entry_type` discriminates the four KIR primitives
(`Object`/`Relationship`/`Event`/`Evidence`) exactly as planned. Idempotence uses a real content
signature (`content_signature`, SHA-256 of the canonical JSON payload, excluding volatile fields
like `created_at`) rather than the id alone — re-appending logically identical content is a no-op,
appending genuinely different content under the same id inserts a new version. This is *stronger*
than the original plan's "entry ids derive from content hashes" — logical identity (the object's
own stable id) and content identity (its version) are tracked separately, which is what makes
real time-travel (below) possible at all.

**Current-state index**: `current_objects(object_id, entry_rowid)` / `current_relationships(...)` —
updated in the same write as the `entries` insert (RFC 0104 later found this wasn't actually
transactional until Phase 1 of the storage-architecture plan fixed it; the *design* always called
for atomicity, the *implementation* had a real gap for a while). `get_object(id)` is one indexed
lookup, never a full-log scan, matching the plan exactly.

**Historical state index**: `object_history(id)` returns every version oldest-to-newest;
`object_at(id, timestamp)` returns the latest version with `written_at <= timestamp`, `None` before
the object's first write. Implemented exactly as Phase 9 specified, and directly reused by every
later time-travel feature this project has shipped (RFC 0047's `all_objects_at`, RFC 0069's
documentation drift, RFC 0106's version-chain checkpoints, RFC 0107/0108's architecture diff).

**Integrity verification**: shipped as `PRAGMA integrity_check` (SQLite's own, real mechanism) plus,
for the fact engine (RFC 0016), `SegmentStore::verify_sealed`/`verify_sealed_report` (RFC 0105) —
both real, both checked — rather than the originally-sketched manual per-row checksum column. A
deliberate, reasonable substitution: SQLite's own page-level integrity check is a stronger,
already-battle-tested guarantee than a hand-rolled checksum field would have been.

## What the original plan called for that was never built, and why

**Full per-entry audit trail with `source_artifact_id` provenance** (`LedgerEntry` linking each
write back to the exact `KnowledgeArtifact` id that produced it, an `audit_trail(id) ->
Vec<AuditRecord>` reader method) — genuinely never implemented. `LedgerEntry` today carries only
`id`/`entry_type`/`payload`/`written_at`; no `source_artifact_id` field exists anywhere in
`crates/ledger`. This is a real, confirmed gap against the original design, not a stale checkbox —
found by grepping the live source, not assumed.

**What shipped instead, covering a related but distinct need**: `KirEvidence` (RFC 0003) —
every semantic conclusion carries a `SourceLocation`/fragment pointing at exactly where in the
original source it came from, cited by id from the object/relationship that used it. This answers
"why do we believe this" (evidence-level provenance) but not "which pipeline run/artifact produced
this exact ledger write" (artifact-level audit trail) — a real, narrower gap than "no audit trail
at all," but still a gap. Not attempted here; tracked in `TODO.md` as a real, scoped follow-on for
whoever picks it up (`LedgerEntry` would need a new field, both backends would need a migration
path, and every append call site would need to thread a real `ArtifactId` through — the exact kind
of write-path change RFC 0104's transaction-wrapping work already touched, so it's a real, findable
extension point, not greenfield).

## Backend evolution since this design (context for later RFCs, not part of this design)

RFC 0015 added zstd-compressed v2 storage on the same SQLite schema; RFC 0016 added the `FactLedger`
fact-segment engine as an alternative, now-default `KnowledgeStore` backend, keeping every method
signature described above (`object_history`, `object_at`, current-state lookups) — the interface
this RFC establishes outlived its first concrete backend, exactly as the original "no code outside
the `ledger` crate may reference SQLite directly" isolation goal intended.

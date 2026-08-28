# RFC 0080 — Storage Architecture Plan: Six Gaps, Grounded and Prioritized

**Status:** Draft (planning RFC — no implementation in this RFC; each phase gets its own
implementation RFC before code, per the Mandatory Development Workflow)
**Author:** EKOS team
**Created:** 2026-08-22

---

## Purpose of this RFC

TODO.md has carried "Storage architecture: six real gaps, none started" since `devlog_65` found
real, physical evidence one of them was already biting (`analytics/`'s real ledger has a corrupted
FTS5 virtual table). Asked to save a real plan for this work before starting it. This RFC is that
plan: each of the six sub-gaps investigated against the actual current implementation (not
TODO.md's one-line summaries), correctly attributed to the backend it actually affects, and
sequenced by real urgency and dependency — not implemented yet. Each phase below gets promoted to
its own dated implementation RFC when work on it actually starts, matching every other increment
this session has shipped.

## Foundation: what already exists

EKOS has two ledger backends behind one `KnowledgeStore` trait (`crates/ledger/src/lib.rs`):

- **SQLite `Ledger`** — the original backend, still serving every pre-existing workspace (this
  repo's own `.ekos/`, `analytics/`) unless explicitly migrated. `PRAGMA journal_mode=WAL` is set,
  but that's SQLite's own internal WAL — unrelated to sub-gap 4 below, which asks for a *ledger-level*
  WAL usable across backends and independent of SQLite's own crash-recovery internals.
- **`FactLedger` (v3, RFC 0016)** — tantivy + mmap fact-segment engine, default for new workspaces
  since 2026-08-21. Already has real compaction machinery for its own index runs
  (`MERGE_RUNS_AT = 8`, `flush_memtable` → `FactIndexes::merge_runs`) and a genuinely
  crash-safe manifest write pattern (write `.tmp` → `fsync` → rename → directory `fsync`).

**RFC 0015** (Accepted, implemented) is byte-level compaction only — zstd on ledger entries,
snapshot/CKM compression, the Pack v1 artifact format. A different axis entirely from sub-gap 1
below (smaller bytes per version, not fewer retained versions) — not a foundation for it.

**RFC 0034** (single-machine vertical partitioning) and **RFC 0110** (horizontal distribution, built
directly on top of RFC 0034) were merged 2026-08-27, per explicit user direction, into **RFC 0111**
(**Under Review, not implemented**) — one conformed design covering the partition model (dimension,
time bucket, tiering, `entity_id → Set<PartitionId>` correctness) plus two deployment modes built on
it: Local (single-machine, default, RFC 0034's original scope) and Distributed (opt-in,
object-storage-backed, RFC 0110's three-service scope). Both source RFCs are now Withdrawn, kept on
disk as historical record. Sub-gap 6 (horizontal distribution) now has a real target: RFC 0111's
Phase B (Distributed mode), which depends only on RFC 0111's own Phase A (Local mode) shipping
first — a within-RFC sequencing dependency, not a cross-RFC one anymore.

**RFC 0047**'s `object_history`/`relationship_history`/`object_at`/`relationships_at` (point-in-time
reconstruction) already give real read primitives any compaction design (sub-gap 1) must preserve
the semantics of for whatever window it keeps uncompacted.

## A real documentation correction found during this investigation

RFC 0016's own "Non-goals" section states multi-writer concurrency is out of scope because *"the
manifest lock enforces"* single-writer exclusion. This doesn't match the code: there is no manifest
lock anywhere in `segment/mod.rs` (that module's own doc comment says single-writer is "the caller
ensures it," not enforced there). What actually enforces single-writer exclusion is **tantivy's own
`IndexWriter` lock** — confirmed independently by reading the code and by `devlog_67`'s live
finding (a second `open_store` handle in the same process hit `LockBusy` immediately at open,
before any write was attempted). This is an incidental side effect of using tantivy, not a designed
mechanism, and RFC 0016's text should be corrected to describe it accurately — filed as part of
Phase 1 below rather than as a separate trivial fix, since the concurrency spec work needs to
settle on the real mechanism first anyway.

## The six sub-gaps, investigated and prioritized

### Phase 1 (highest priority — real, live evidence) — Concurrency: two different real gaps, one per backend

**[x] Done — RFC 0104 / `devlog_121` (2026-08-26).** SQLite `Ledger`'s multi-statement writers
(`append`/`append_object`/`append_relationship`) now run inside real `BEGIN IMMEDIATE`/`COMMIT`
transactions (`in_transaction`), resolved without adding a supplementary explicit lock — SQLite's
own WAL-mode locking under `BEGIN IMMEDIATE` is already real cross-process protection. `FactLedger`
gets a real, designed `write.lock` file (`fs4`, `flock`(2)-backed), acquired first on every writable
open, before `SegmentStore`/`SearchIndex` are touched — a second writable process now fails fast
with a clear `LedgerError::Locked` instead of an eventual tantivy-internal error. RFC 0016's
Non-goals text corrected. The concurrent-read visibility spec item turned out to be a real,
previously-unverified gap, not housekeeping: a `FactLedger` handle's view is frozen as of its own
`open()` call, not automatically refreshed by a separate process's writes — proven with a dedicated
regression test, not just documented as a claim. Live-verified with two real, separate `ekos commit`
processes racing the same real scratch workspace.

**SQLite `Ledger` — the actual, most likely cause of the real corruption found.** `append_object`
(`crates/ledger/src/lib.rs`) executes 3-4 separate statements per write (INSERT into `entries` →
SELECT `current_objects` → INSERT OR REPLACE `current_objects` → FTS index update) with **no
transaction wrapping** — the only `BEGIN`/`COMMIT` in the entire file is in the one-time
`migrate_to_v2` routine. `object_fts` is a real FTS5 contentless virtual table, and it's exactly
this table `devlog_65` found corrupted in `analytics/`'s real ledger (base DB passes
`PRAGMA integrity_check`; the FTS5 virtual table doesn't). An unwrapped multi-statement write
sequence under concurrent `ekos` processes is a concrete, plausible mechanism for exactly that
corruption shape — not the only possible cause, but a real, fixable one sitting in the code today.

*Design direction*: wrap `append_object`/`append_relationship`/other multi-statement writers in a
real SQLite transaction (`BEGIN IMMEDIATE`/`COMMIT`), so a crash or concurrent write mid-sequence
can't leave `current_objects`/`object_fts` inconsistent with `entries`. Whether SQLite's own
`BEGIN IMMEDIATE` lock is sufficient cross-process protection on its own, or whether an explicit
`flock`-based advisory lock is also warranted, is the real open design question for that
implementation RFC — not resolved here.

**`FactLedger` v3 — better, but the safety story needs to be true, not just believed.** Real
in-process safety (`Mutex<Inner>` held for the whole `append_inner` body) and a genuinely
crash-safe manifest write pattern already exist. The real gaps: (1) the RFC 0016 documentation
correction above; (2) an explicit, designed cross-process write lock, rather than incidentally
relying on tantivy's own `IndexWriter` lock (which works today but was never a deliberate design
choice for this purpose, and nothing stops a future tantivy upgrade from changing that behavior);
(3) a written spec for what a concurrent reader observes mid-write — RFC 0016 claims "the same
visibility SQLite WAL gives today" but this hasn't been independently verified against the real
implementation.

**Update:** item (3) was verified by RFC 0104 Phase 1 (`devlog_121`) — the claim was found false and
corrected: a `FactLedger` handle's view is frozen as of its own `open()` call, not automatically
refreshed by a separate process's writes, a real, material difference from SQLite's WAL mode. RFC
0104 shipped that as a documented, accepted limitation rather than closing it. **RFC 0112** (Draft,
2026-08-27) is the follow-on that actually closes it — a lock-free, per-read incremental snapshot
refresh (reusing the existing watermark/`batches_after` recovery-scan logic and tantivy's already-
present-but-unused-on-read `IndexReader::reload()`), replacing RFC 0097's coarser full-tree-mtime-
scan-then-full-reopen `StoreCache` workaround.

### Phase 2 — WAL + repair tool

**[x] Done — RFC 0105 / `devlog_122` (2026-08-26).** Confirmed the "WAL" half needed no new code —
`FactLedger`'s existing segment format already provides real, ledger-level WAL durability; the real
gap was that nothing surfaced it. New `SegmentStore::verify_sealed_report` checks every sealed
segment unconditionally (not just the first failure); `verify_sealed` refactored on top of it so
the two checks can't drift. New `ekos ledger repair` CLI command opens the ledger (free self-heals:
torn active-segment tail truncation, stale index-runs rebuild), then reports one line per sealed
segment. Replaces `TODO.md`'s previously accurate "the only recovery option is a full migration
rollback" with a real, precise diagnostic — no automatic fix for genuine corruption (no redundancy
exists to reconstruct lost bytes), but a human now gets exactly which segment and transaction range
is affected instead of an opaque failure. FactLedger-only, matching every prior phase's precedent
of not doubling scope onto the SQLite backend (its own `PRAGMA integrity_check` already covers the
analogous job).

No ledger-level WAL exists in either backend today (SQLite's own `journal_mode=WAL` is a different,
backend-internal thing, not something a repair tool for the *logical* ledger could use across both
backends). `FactLedger` already has strong crash-recovery primitives to build a repair tool on top
of (checksummed frames, atomic manifest writes, "crash between fsync and watermark publish loses
nothing: recovery scans forward" per RFC 0016) — the real gap is that no *tool* surfaces this
today; TODO.md's characterization ("the only recovery option is a full migration rollback") is
accurate as far as this investigation found. Natural follow-on to Phase 1, since a real
concurrency-safety fix and a real repair tool are answering the same underlying question
("what happens when a write is interrupted or races another one") from two different angles.

### Phase 3 — Snapshot + compaction of the version chain

**[x] Done — RFC 0106 / `devlog_123` (2026-08-26).** Built as a pure, purely-additive acceleration
structure — periodic per-entity checkpoints (`checkpoints.jsonl`) let `state_at` (the shared engine
behind `object_at`/`current_sig`/every point-in-time read) seed its fold from the nearest prior
checkpoint instead of always genesis, provably equivalent to full replay by construction (never
consulted for correctness, only speed — a missing/corrupt checkpoint just means a slower, still
100% correct fold). Honest scope check before shipping: `FactIndexes`' EAVT key order
(entity→attribute→value→tx) means the underlying index scan itself can't be tx-bounded cheaply, so
the real win is in the fold cost, not scan I/O — stated precisely, not oversold.

**A real finding, not resolved here, explicitly flagged**: RFC 0080's own Phase 4
("retention/pruning policy") implies eventually discarding old delta history — in real tension with
`CLAUDE.md`'s own Key Invariant that the ledger is append-only with no object-level delete/tombstone
anywhere (deliberate, not an oversight). Phase 3 needed no resolution of this (checkpoints are
purely additive, nothing discarded), but Phase 4 does — relaxing a reviewed, load-bearing invariant
needs its own explicit conversation with the user before any design work starts, not a reflexive
"next phase in sequence."

Genuinely greenfield — no existing building block does this (the `.ekos/snapshots/*.json.zst`
mechanism in `build.rs` is unrelated: a build-time artifact-index dump for observation-layer
bookkeeping, not KIR/CKM version history; `SNAPSHOT_KEEP = 10` prunes only those, not ledger fact
history). Real design constraint: whatever compaction scheme ships must preserve
`object_history`/`object_at`'s existing semantics for objects still inside the retained window —
this is the concrete acceptance criterion its implementation RFC needs to define precisely
(how far back does "the window" go, and what's kept vs. summarized beyond it).

### Phase 4 — Retention/pruning policy

No retention/pruning of ledger fact history exists anywhere (distinct from `SNAPSHOT_KEEP`, which
only touches the unrelated build-index snapshots — confirmed the same distinction as Phase 3).
RFC 0034 explicitly named this as its own future RFC rather than attempting it.

**Real blocker found during Phase 3 (RFC 0106), not just a sequencing dependency**: this phase, as
named, means discarding old delta history — which directly conflicts with `CLAUDE.md`'s own Key
Invariant that the ledger is append-only with no object-level delete/tombstone mechanism anywhere
(a deliberate, reviewed project decision, not an oversight). Phase 3's checkpoints do *not* remove
this blocker — they were deliberately built as a purely additive acceleration structure specifically
so they wouldn't need to. Before any Phase 4 design work starts, this needs an explicit decision
from the user: relax the append-only/no-delete invariant (a real, load-bearing architectural
change, not a phase-4-shaped feature), or re-scope Phase 4 to something that doesn't require it
(e.g., cold storage/archival to a separate location rather than in-place deletion — not investigated
here). Not scheduled until that conversation happens.

### Phase 5 — Materialized views alongside the EAV fact engine

Not investigated to the same depth this pass (out of this research round's scope) — `FactLedger`'s
existing `FactIndexes` (EAVT/AEVT/AVET, `crates/ledger/src/index.rs`) is the existing derived-index
layer a materialized-view design would extend or sit alongside, rather than build from nothing.
Needs its own scoping pass (which query patterns are actually expensive enough to justify a
materialized view, checked against real EKL/MCP query logs) before an implementation RFC — the
lowest-urgency item with the least grounding work done so far.

### Phase 6 — Horizontal distribution

**Re-scoped 2026-08-27, per the option this RFC itself left open** ("or being explicitly re-scoped
to not require it"), **then merged into RFC 0111 the same day** per explicit user direction: RFC
0034 (single-machine partitioning) and RFC 0110 (horizontal distribution, built on top of RFC 0034)
are now one conformed design — **RFC 0111**, Under Review — rather than two cross-referencing RFCs.
RFC 0111's Phase A is RFC 0034's original single-machine scope; its Phase B is RFC 0110's
storage/compute-separated, three-service architecture (object storage as the ledger's single
durable copy via the `object_store` crate; a distributed MPP compile/ingest cluster; a distributed
MPP query cluster of stateless workers running the existing EAVT/AEVT/AVET fold against
cached-from-object-storage partitions; a single logical, load-balanced query gateway) plus
distributed search with an explicit cross-shard BM25 caveat. It ships no code. Phase B's acceptance
still requires Phase A shipping first and resolving the `entity_id → Set<PartitionId>` correctness
gap — now a within-RFC phase dependency (RFC 0111 §2, §11) rather than a cross-RFC one.

## What this RFC does not do

No code changes. No implementation RFC has been written for Phase 1 yet — that's the literal next
step when this work starts, following the same Design → Interfaces → Tests → Implementation
sequence every other increment this session has used. This RFC's job is to make sure the six items
stay a real, ordered, technically-grounded plan rather than an undifferentiated backlog line.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0080-storage-architecture-plan.md` | This RFC |
| `TODO.md` | Storage architecture item updated to reference this plan and its phase ordering |

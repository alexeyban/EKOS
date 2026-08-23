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

**RFC 0034** (**Draft, not implemented**) is single-machine vertical partitioning
(`PartitionedLedger` composing multiple `FactLedger`s by `(source_scope, time_bucket)`) plus
hot/cold tiering. It explicitly scopes retention/deletion and horizontal distribution out as
"a distinct, larger RFC" — this one. Important correction to how TODO.md's phrasing could be read:
RFC 0034 itself isn't built yet, so sub-gap 6 (horizontal distribution) has no completed
single-machine foundation to build "beyond" — it would need RFC 0034 shipped first, or to be
re-scoped as not depending on it.

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

### Phase 2 — WAL + repair tool

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
RFC 0034 explicitly named this as its own future RFC rather than attempting it. Natural sequel to
Phase 3 — a pruning policy needs the compaction model to exist first, since "prune" without a
compaction/summarization step underneath it means real data loss, not archival.

### Phase 5 — Materialized views alongside the EAV fact engine

Not investigated to the same depth this pass (out of this research round's scope) — `FactLedger`'s
existing `FactIndexes` (EAVT/AEVT/AVET, `crates/ledger/src/index.rs`) is the existing derived-index
layer a materialized-view design would extend or sit alongside, rather than build from nothing.
Needs its own scoping pass (which query patterns are actually expensive enough to justify a
materialized view, checked against real EKL/MCP query logs) before an implementation RFC — the
lowest-urgency item with the least grounding work done so far.

### Phase 6 — Horizontal distribution

Blocked on RFC 0034 (Draft, unimplemented) shipping first, or being explicitly re-scoped to not
require it. No code exists for this today, which is expected — nothing single-machine to
distribute "beyond" yet. Not schedulable with a real design until Phase 1-4's answers (which shape
what "correct" distributed writes/reads even mean for this ledger) are settled.

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

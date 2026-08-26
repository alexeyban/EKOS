# Devlog 123 — RFC 0106: Storage Architecture Phase 3 (Version-Chain Checkpoints)

**Date:** 2026-08-26
**PRs:** RFC 0106
**Branch:** main (direct)

---

## Summary

The third implementation phase of RFC 0080's storage architecture plan — the genuinely greenfield
one. `FactLedger::state_at` (the shared engine behind `object_at`/`current_sig`/every point-in-time
read) always folded an entity's entire history from genesis; `entity_history`'s own code comment
already named this as `O(versions × entries)`, "not optimized for entities with very long
histories." Periodic, purely-additive checkpoints now let `state_at` seed its fold from the nearest
prior checkpoint instead of genesis — a real, tested, live-verified improvement that, by
construction, can never make a read wrong, only occasionally not-yet-accelerated. A real tension
with this project's own append-only/no-delete invariant was found and deliberately *not* resolved —
flagged explicitly rather than built around silently.

---

## RFC 0106 — Version-Chain Checkpoints

### Problem / motivation

RFC 0080 scoped Phase 3 as "snapshot + compaction of the version chain," with one hard constraint:
whatever ships must preserve `object_history`/`object_at`'s existing semantics for the retained
window. The real, concrete problem behind that framing: no acceleration structure exists at all for
folding an entity's history — every point-in-time read is `O(entity's total fact count)`, forever.

### What was built

| Component | Change |
|---|---|
| `checkpoints.jsonl` | New per-workspace file: periodic `(entity, tx, facts)` records |
| `Inner::checkpoint_at` | Looks up the nearest checkpoint at or before a requested cut |
| `Inner::state_at` | Now seeds its fold from the nearest checkpoint instead of always genesis |
| `Inner::maybe_checkpoint` | Writes a new checkpoint once `CHECKPOINT_INTERVAL` (20) versions accumulate since the last one |

### Implementation details worth remembering

- **A real, load-bearing project invariant almost got quietly implicated.** `CLAUDE.md`'s Key
  Invariants state the ledger is append-only with no object-level delete/tombstone anywhere,
  confirmed deliberately, not an oversight. RFC 0080's own Phase 4 ("retention/pruning policy")
  implies eventually discarding old delta history — in real tension with that invariant. Resolved
  for *this* RFC by design, not by ignoring the tension: checkpoints are purely additive (nothing is
  ever discarded), so Phase 3 needed zero resolution of it. Phase 4 is now explicitly flagged, in
  the RFC itself, as needing its own separate conversation with the user before any design work
  starts — relaxing a reviewed invariant is not something to back into as "the next phase in
  sequence."
- **The real win is narrower than the first draft of the design claimed, and it was corrected
  before implementation, not after.** The initial framing was "eliminate O(full history) reads
  entirely." Reading `FactIndexes::scan`'s actual block-skip logic before writing any code showed
  the EAVT index's key order (entity→attribute→value→tx, not entity→tx) means a tx-lower-bound
  can't be pushed into the scan itself — every block containing any of an entity's facts still
  returns all of them regardless of `tx`. The real, honest win is narrower: checkpoints shrink the
  **fold** (`fold_state`'s per-entry merge, `reconstruct`'s canonicalization), not the underlying
  scan/I-O, which stays `O(entity's total fact count)` either way. Stated precisely in the RFC
  rather than left as an overclaim that live testing would have quietly under-delivered on.
- **Checkpoints store already-decomposed `Vec<Fact>`, not raw JSON payloads — specifically to avoid
  a `&mut AttributeRegistry` requirement on the read path.** `decompose` (turning a JSON payload
  back into facts) needs mutable registry access to intern any new attribute path — a real friction
  point, since every existing read method took `&self`, not `&mut self`. Storing checkpoints
  pre-decomposed at *write* time (where `&mut` access already exists, on the `append_inner` path)
  means the *read* path never needs to touch the registry at all — zero signature changes to any
  existing read method, a meaningfully smaller and lower-risk change than the alternative.
- **Correctness rests on one precise, checked property of `fold_state`'s existing (unmodified)
  logic**: a checkpoint's synthetic entries are all stamped at the checkpoint's own `tx`; any real
  entry after that `tx` has a strictly greater `tx`, so `fold_state`'s last-write-wins-by-tx rule
  always lets it win over the synthetic seed for the same `(attr, pos)` — checkpoint-seeded folding
  is provably equivalent to full genesis replay, not just empirically close. A dedicated test
  (`a_checkpoint_is_written_after_crossing_the_interval_and_reads_stay_correct`) verifies this
  directly: 25 real versions of one object, `object_at` checked against every one of 25 captured
  timestamps spanning before/at/after the checkpoint boundary.
- **A torn/corrupt checkpoint line degrades to slow, never wrong — verified, not just designed
  that way.** `checkpoints.jsonl` deliberately gets none of the segment format's fsync/atomic-rename
  rigor, because correctness never depends on it. A dedicated test appends literal garbage as a
  trailing line and confirms the ledger still opens and still reads correctly (using whatever valid
  checkpoints came before the garbage).

### Decisions (alternatives considered, why this choice)

- **No deletion/pruning of old delta history — checkpoints are purely additive.** The direct
  consequence of the invariant tension above; also the only design that satisfies RFC 0080's
  "preserve `object_at` semantics" constraint in its strongest form (identical for *all* history,
  not just a bounded window).
- **`entity_history`/`object_history` deliberately left unaccelerated.** Its own existing code
  comment already named the `O(versions × entries)` cost; making it checkpoint-aware too (each of
  its per-version folds could seed from the nearest preceding checkpoint) is real, valuable,
  cleanly-scoped follow-on work, not bundled into an already-substantial RFC.
- **No SQLite-backend equivalent.** Matches every prior phase's precedent — the SQLite backend's
  flat per-version row store has a materially different cost shape (no EAV fold at all).

---

## Knowledge Captured

- **Reading the actual index/scan implementation before writing a performance RFC's design
  section caught a real overclaim before it shipped.** The initial "eliminates O(full history)
  reads" framing was wrong in a way that would have only surfaced later, as a performance
  measurement that didn't match the RFC's own promise. Checking `FactIndexes::scan`'s literal
  block-skip logic against the EAVT key order caught this at design time, not after users noticed a
  performance claim that didn't hold.
- **A pure-acceleration design (never consulted for correctness, only for speed) is a real,
  general pattern for adding a new subsystem to an append-only ledger without touching its
  invariants at all.** Worth reaching for again the next time a performance gap needs a durable
  structure but the underlying data must stay untouched.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ledger/src/fact_ledger.rs` | `checkpoints.jsonl` read/write; `Inner::checkpoint_at`/`entries_since_checkpoint`/`maybe_checkpoint`; `state_at` checkpoint-aware; wired into `append_inner`; 3 new tests |
| `ekos/docs/rfcs/0106-storage-checkpoints-phase3.md` | New RFC |
| `ekos/docs/rfcs/0080-storage-architecture-plan.md`, `TODO.md` | Phase 3 marked done, points to RFC 0106; Phase 4's invariant tension flagged explicitly |

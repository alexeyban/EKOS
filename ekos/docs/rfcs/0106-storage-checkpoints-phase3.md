# RFC 0106 — Storage Architecture Phase 3: Version-Chain Checkpoints

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0080 scoped Phase 3 as "snapshot + compaction of the version chain," genuinely greenfield, with
one hard constraint: whatever ships must preserve `object_history`/`object_at`'s existing semantics
for the retained window. Investigating the real code found the concrete, already-acknowledged
problem this needs to solve: `FactLedger::state_at` (the shared engine behind `object_at`,
`current_sig`, and anything else that needs an entity's state as of some point in time) always
folds an entity's **entire** history from genesis, every call — there is no acceleration structure
at all. `entity_history`'s own doc comment already names this explicitly: *"O(versions × entries)
— fine for this RFC's scope (a small fixture), not optimized for entities with very long
histories."* For a long-lived, frequently-updated entity, every `object_at`/`get_object`-style read
re-derives the same early history over and over.

## A real constraint found during design: this project has no delete/tombstone mechanism

`CLAUDE.md`'s own Key Invariants state the ledger is **append-only** and that *"there is no way to
un-commit something already ledgered (confirmed: no object-level delete/tombstone exists anywhere
in the codebase)."* RFC 0080's own Phase 4 ("retention/pruning policy") implies eventually
*discarding* old delta history — which would mean modifying or removing already-written ledger
state, directly in tension with that stated, reviewed invariant. That tension is real and
**explicitly not resolved here** — Phase 3, as designed below, needs no resolution of it at all,
because nothing this RFC builds ever discards anything. Phase 4 is a separate, much larger decision
(relaxing a load-bearing project invariant) that needs its own explicit conversation with the user
before any design work starts, not something to build reflexively as "the next phase in sequence."

## Design

### Checkpoints are a pure acceleration structure — zero data loss, by construction

A **checkpoint** is a durable, periodic recording of one entity's fully-folded fact state at one
transaction (`(entity, tx) -> Vec<Fact>`), stored in a new file, `<facts_root>/checkpoints.jsonl`
(one JSON line per checkpoint; deliberately distinct in name from the unrelated, already-existing
`.ekos/snapshots/*.json.zst` build-index mechanism RFC 0080's own investigation flagged as a
different thing entirely). Checkpoints are **never consulted for correctness, only for speed**:
every existing full-history fold path continues to exist and remains correct on its own; a
checkpoint that's missing, stale, or fails to parse (a torn trailing line from an interrupted
write — checkpoints intentionally don't need segment-grade fsync/atomic-rename durability, *because*
correctness never depends on them) just means that one lookup falls back to the exact behavior this
codebase already has today. This is what lets this RFC satisfy RFC 0080's stated constraint in the
strongest possible form: semantics are identical for **all** history, not just "the retained
window," because nothing is ever discarded — checkpoints are purely additive.

### Where checkpoints are stored and looked up

`Inner` gains `checkpoints: HashMap<Uuid, BTreeMap<TxId, Vec<Fact>>>`, loaded once at open by
reading `checkpoints.jsonl` line by line (a parse failure on any one line — most plausibly the
last, torn — is silently skipped, not an open-time error). `state_at(entity, cut)` — the function
behind `object_at`/`reconstruct_at`/`current_sig`, and therefore the shared engine under every
point-in-time read — now: looks up the latest checkpoint at or before the effective cut
(`BTreeMap::range(..=cut).next_back()`, or the latest checkpoint at all when `cut` is `None`,
meaning "current state"); if one exists, seeds `fold_state` with the checkpoint's facts (each
wrapped as a synthetic `IndexEntry` stamped at the checkpoint's own `tx`), filters the entity's
real entries to only those strictly after that `tx`, and folds the combination instead of the
entity's entire history. `fold_state`'s own existing last-write-wins-by-tx logic (unmodified)
guarantees this is exactly equivalent to a full genesis replay: any real entry after the
checkpoint's `tx` has a strictly greater `tx` than the synthetic checkpoint entries for the same
`(attr, pos)`, so it always wins the fold precisely as it would without a checkpoint in the picture
at all.

**Honest scope of the win, checked against the real index format before claiming it**: `EAVT`
scanning (`FactIndexes::scan`/`ScanPrefix::Entity`) skips whole *blocks* that can't contain this
entity at all, but every block that does still returns every one of the entity's facts regardless
of `tx` — there is no tx-lower-bound pushed into the scan itself (`EAVT`'s key order is
entity→attribute→value→tx, not entity→tx, so a cheap tx-bounded range scan isn't available without
restructuring that index — real, separately-scoped work, not attempted here). This RFC's checkpoint
therefore accelerates the **fold** (`fold_state`'s per-entry merge plus `reconstruct`'s
canonicalization over a shrunk fact set) — real, proportional savings for an entity with a long
history — not the underlying scan/deserialization I/O, which stays `O(entity's total fact count)`
either way. Stated precisely here rather than oversold: this is a real, meaningful, but partial
improvement, not an unbounded-history-stays-O(1) guarantee.

### When checkpoints get written

Inside `append_inner` (the single write path every `append_object`/`append_relationship`/
`append_evidence`/`append_event` already funnels through), after a write commits: count real
entries for that entity strictly after its latest checkpoint (itself a checkpoint-accelerated,
therefore cheap, lookup — bounded by the interval below, not by the entity's total history); once
that count reaches `CHECKPOINT_INTERVAL` (20 versions), write a new checkpoint by folding the
entity's full state as of the just-committed `tx` (paying the O(full history) cost exactly once
every 20 versions, not on every read) and appending it to `checkpoints.jsonl`.

### `entity_history` — explicitly not accelerated in this RFC

`entity_history` (behind `object_history`) still walks every distinct historical version, each
folded independently — its own doc comment's `O(versions × entries)` caveat is unchanged by this
RFC. Making it checkpoint-aware too is real, valuable, follow-on work (each of its per-version folds
could seed from the nearest preceding checkpoint the same way `state_at` now does) — deliberately
left for a future increment rather than expanding this RFC's already-substantial surface, since
`object_history` is a comparatively rare audit/debug-style query next to `object_at`/`get_object`'s
hot path.

## Non-goals

- **Any form of deletion, pruning, or discarding old delta history.** See the invariant-tension
  section above — genuinely out of scope for this RFC, not just deferred casually.
- **Accelerating `entity_history`/`object_history`.** Named above as real follow-on work.
- **Segment-grade crash safety for the checkpoint file.** Deliberately unnecessary — see "pure
  acceleration structure" above.
- **A SQLite-backend equivalent.** SQLite's `entries` table is a flat per-version row store already
  indexed by `(id, content_sig)` — a materially different cost shape than the fact engine's EAV
  fold, and out of scope for this phase (matches every prior phase's precedent of not doubling
  scope onto the backend being phased out).

## Verification

New `ekos-ledger` tests: a checkpoint-accelerated `object_at`/`current_sig` result is byte-for-byte
identical to the pre-checkpoint full-replay result across many versions of the same entity (the
core correctness property — checked directly by comparing against a *second*, checkpoint-disabled
ledger built from the identical write sequence); a checkpoint is written exactly once every
`CHECKPOINT_INTERVAL` versions, not more or less; `object_at` for a cut *before* the entity's
earliest checkpoint still falls back to full replay and returns the correct historical version;
`object_at` for a cut between two checkpoints picks the earlier one, not the later; a corrupted
(torn) trailing line in `checkpoints.jsonl` is silently skipped on open, with every other checkpoint
still usable and every read still correct (proving a broken checkpoint degrades to slow-but-correct,
never wrong). Full workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D
warnings`, `test --workspace`), `tests/integration` 3/3.

Live-verified against a real scratch workspace: an entity updated enough times to cross
`CHECKPOINT_INTERVAL` produces a real `checkpoints.jsonl` with real entries; `ekos query find`/an
EKL `AS OF` query against a timestamp after the checkpoint returns the identical result before and
after this RFC's change (confirmed by running the same query against the pre-checkpoint code path
on an equivalent fixture).

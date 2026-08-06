# RFC 0005 — Knowledge Runtime

**Status:** Accepted  
**Date:** 2026-07-04

---

## Problem

The Semantic Knowledge Ledger is append-only and stores raw `KirObject`, `KirRelationship`, and
`KirEvidence` entries. AI agents and CLI users need a *view* over that raw storage: a way to ask
"what is this object?", "what is it connected to?", and "what did we know at time T?" without
writing SQL, knowing the ledger schema, or touching the database directly.

---

## Solution

A thin, **read-only** `Runtime` struct that wraps a `&Ledger` reference and exposes four
query methods:

| Method | Description |
|---|---|
| `load_object(id)` | Fetch a single object by ID |
| `load_neighborhood(id, depth)` | BFS graph up to N hops |
| `reconstruct_state(id)` | Object + all direct rels + all evidence |
| `reconstruct_state_at(id, at)` | Same, filtered to entries written ≤ `at` |

`Runtime` holds only an immutable reference to the ledger. It has no `&mut self` methods and no
write path to the ledger. This upholds the invariant: the Runtime reconstructs state; it never
modifies it.

---

## `ObjectState`

```rust
pub struct ObjectState {
    pub object: KirObject,
    pub relationships: Vec<KirRelationship>,
    pub evidence: Vec<KirEvidence>,
}
```

A fully denormalized snapshot. All three collections are populated in a single
`reconstruct_state()` call — no N+1 queries at the caller.

---

## `load_neighborhood` — BFS algorithm

1. Load the root object from the ledger. If not found, return an empty `KirGraph`.
2. Maintain a `visited: HashSet<KirId>` and a `queue: VecDeque<(KirId, depth)>`.
3. For each node dequeued: if `depth >= max_depth`, skip (do not expand).
4. For each relationship returned by `ledger.relationships_for(current_id)`:
   - Add the relationship to the graph (deduplicated by `rel.id`).
   - Resolve the neighbour ID (whichever of `rel.from` / `rel.to` is not `current_id`).
   - If not visited, load the neighbour, add it to the graph, enqueue at `depth + 1`.
5. Return the accumulated `KirGraph`.

The `visited` set prevents infinite loops through cycles.

**depth=0**: root object only, no relationships.  
**depth=N**: N hops from the root.

---

## Ledger schema additions (this RFC)

Two schema additions needed to support `relationships_for` and `relationships_at`:

```sql
CREATE TABLE IF NOT EXISTS current_relationships (
    rel_id      TEXT PRIMARY KEY,
    from_id     TEXT NOT NULL,
    to_id       TEXT NOT NULL,
    kind        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rel_from ON current_relationships(from_id);
CREATE INDEX IF NOT EXISTS idx_rel_to   ON current_relationships(to_id);
```

The payload (full `KirRelationship` JSON) lives in `entries`; `current_relationships` is the
current-state index. The same pattern as `current_objects`.

---

## Historical queries

`reconstruct_state_at(id, at: DateTime<Utc>)` filters by `entries.written_at <= at`. This is
ledger-commit time, not observation time. Phase 10 v0 explicitly accepts this limitation:
observation time tracking is deferred to a future RFC.

---

## Non-goals

- Streaming or pagination (v0 returns `Vec<T>`)
- Async methods (synchronous ledger reads are fast enough for CLI and single-user tools)
- Caching (the ledger's WAL mode is the performance floor in v0)
- Write access of any kind

---

## Alternatives considered

**Expose `Ledger` directly to callers** — rejected because it leaks the SQL schema and makes it
impossible to swap the ledger backend (e.g. to a remote API) without touching all callers.

**Async `Runtime`** — rejected for v0. The ledger uses synchronous SQLite. The overhead of
`tokio::spawn_blocking` wrappers is not justified by the current use case (CLI + tests).

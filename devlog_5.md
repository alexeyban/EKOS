# Devlog 5 — Phase 10: Runtime

**Date:** 2026-07-04  
**PRs:** —  
**Branch:** main

---

## Summary

Implemented Phase 10 — the read-only Knowledge Runtime. The ledger now stores relationships
alongside objects and evidence. The Runtime wraps the Ledger with four query methods: point
lookup, BFS neighbourhood traversal, full state reconstruction, and historical state-at queries.
`ekos query neighbourhood <id> --depth N` is wired as a new CLI sub-command. All tests pass
(20 new tests: 11 ledger, 9 runtime), clippy clean.

---

## Changes

### Ledger — relationship storage (`crates/ledger/src/lib.rs`)

Added a `current_relationships` table alongside the existing `current_objects`:

```sql
CREATE TABLE IF NOT EXISTS current_relationships (
    rel_id  TEXT PRIMARY KEY,
    from_id TEXT NOT NULL,
    to_id   TEXT NOT NULL,
    kind    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_rel_from ON current_relationships(from_id);
CREATE INDEX IF NOT EXISTS idx_rel_to   ON current_relationships(to_id);
```

The payload (full `KirRelationship` JSON) lives in `entries`, exactly as objects do.
`current_relationships` is a current-state index only.

New ledger methods:

| Method | Purpose |
|---|---|
| `append_relationship(rel)` | Write rel to entries + index; idempotent |
| `get_relationship(id)` | Fetch a single relationship by ID |
| `relationships_for(id)` | All rels where `from_id = id OR to_id = id` |
| `object_at(id, at)` | Object as it existed at commit time `≤ at` |
| `relationships_at(id, at)` | Relationships involving `id`, committed `≤ at` |
| `relationship_count()` | Count of distinct relationships in the index |

The `init_schema()` schema migration uses `CREATE TABLE IF NOT EXISTS`, so existing ledger
files gain the new table automatically when re-opened. Old data is unaffected.

### Runtime (`crates/runtime/src/lib.rs`)

Full replacement of the stub. `Runtime<'a>` holds an immutable `&'a Ledger` — no write path.

| Method | Description |
|---|---|
| `load_object(id)` | Thin wrapper over `ledger.get_object` |
| `load_neighborhood(id, depth)` | BFS: depth=0 → root only; depth=N → N hops |
| `reconstruct_state(id)` | `ObjectState { object, relationships, evidence }` |
| `reconstruct_state_at(id, at)` | Same, filtered to entries written ≤ `at` |

`load_neighborhood` maintains a `visited: HashSet<KirId>` for cycle safety and deduplicates
relationships by `rel.id` before adding them to the output `KirGraph`.

### CLI — commit relationships (`crates/cli/src/commands/commit.rs`)

`ekos commit` now also writes `CkmRelationship → KirRelationship` to the ledger after writing
objects and evidence. Output line added:

```
Relationships written: N
```

### CLI — `ekos query neighbourhood` (`crates/cli/src/commands/query.rs`)

New `neighbourhood(config, cwd, id_str, depth)` function. Outputs:

```
Neighbourhood of <id> (depth 1): 3 objects, 2 relationships

  <uuid>  orders (Table) [root]
  <uuid>  customers (Table)
  <uuid>  order_items (Table)

  ForeignKey  <orders-id> → <customers-id>
  ForeignKey  <orders-id> → <order_items-id>
```

Wired as `ekos query neighbourhood <id> [--depth N]` (default depth=1).

---

## Knowledge Captured

- **`CREATE TABLE IF NOT EXISTS` as a safe migration pattern**: SQLite silently skips the
  statement if the table exists. Since `init_schema()` is called every time `Ledger::open()`
  runs, adding new tables there is safe for existing databases. No separate migration runner
  needed for Phase 10.

- **Historical queries use ledger-commit time, not observation time**: `object_at(id, at)` and
  `relationships_at(id, at)` filter by `entries.written_at` (the time `ekos commit` ran), not
  the original observation timestamp from the source system. This is a known v0 limitation.
  Phase 10 explicitly accepted it; a future RFC will introduce `observed_at` tracking.

- **BFS relationship dedup by `rel.id`**: `relationships_for(id)` can return the same
  relationship twice when both `from` and `to` equal known visited nodes. The BFS deduplicates
  by checking `graph.relationships.iter().any(|r| r.id == rel.id)` before inserting. This is
  O(n) per relationship but acceptable for v0 graph sizes.

- **`Runtime` carries a lifetime `'a`**: `Runtime<'a>` borrows `&'a Ledger`. This means the
  Ledger must outlive the Runtime at the call site, which is natural (CLI commands create both
  in the same scope). If callers need to store both in a struct, they must use `Arc<Ledger>` and
  change the lifetime. That refactor is deferred to Phase 13 (caching).

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/ledger/src/lib.rs` | Added `current_relationships` schema + 6 new methods + 6 new tests |
| `crates/runtime/src/lib.rs` | Full implementation: 4 methods, `ObjectState`, 9 tests |
| `crates/runtime/Cargo.toml` | Added `chrono`, `tempfile` (dev-dep); removed `ekos-common` |
| `crates/cli/src/commands/commit.rs` | Write `CkmRelationship → KirRelationship` to ledger |
| `crates/cli/src/commands/query.rs` | Added `neighbourhood()` function |
| `crates/cli/src/bin/ekos.rs` | Added `QueryCommands::Neighbourhood { id, depth }` |
| `docs/rfcs/0005-runtime.md` | RFC 0005 (accepted) |
| `TODO.md` | Ticked Phase 10 items |

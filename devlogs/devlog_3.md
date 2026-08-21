# Devlog 3 — Phase 7: Identity Resolution

**Date:** 2026-07-03  
**PRs:** —  
**Branch:** main

---

## Summary

Implemented Phase 7 — Identity Resolution. The `ekos-identity` crate is a standalone library
that merges synonymous `KirObject`s discovered across sources into canonical identities.
84 tests pass (19 new in this phase), clippy clean.

---

## Phase 7 — Identity Resolution (`crates/identity/`)

### Problem / motivation

After Phase 6, the knowledge graph contains multiple objects for the same real-world concept:
`Customer` (SQL table) + `customer` (git commit messages) + `customer_table` (legacy naming).
Without resolution, every downstream query returns fragmented results and AI systems see the same
entity multiple times under different names.

### What was built

| Component | Description |
|---|---|
| `similarity::normalize(name)` | Lowercase, replace `_-·` with space, strip table/tbl suffixes |
| `similarity::jaro(s1, s2)` | Standard Jaro similarity algorithm, O(n·m) |
| `similarity::jaro_winkler(s1, s2)` | Jaro + common-prefix bonus (max 4 chars, weight 0.1) |
| `SimilarityScore` | `{ name, structural, combined }` — combined = 0.7·name + 0.3·structural |
| `MergeProposal` | canonical_name, canonical_kind, source_ids, confidence |
| `ConflictReport` | `SameNameDifferentKind` — same normalised name, different ObjectKind |
| `ResolutionResult` | proposals + conflicts + stats; fully serialisable |
| `IdentityResolver` trait | `fn resolve(&self, graph: &KirGraph) -> ResolutionResult` |
| `DefaultResolver` | Blocking + Jaro-Winkler + Union-Find; threshold configurable |
| `ekos resolve` | CLI command: loads all KnowledgeArtifacts, merges graphs, resolves |

### Algorithm (RFC 0007)

1. **Blocking**: partition by `(ObjectKind, first-3-chars-of-normalised-name)` — only pairs in
   the same block are compared (prevents O(n²) growth)
2. **Scoring**: Jaro-Winkler on normalised names, weighted 0.7; structural (same kind) 0.3
3. **Merge**: pairs above threshold (default 0.85) are unioned in a Union-Find
4. **Transitivity**: handled automatically by Union-Find (A≈B and B≈C → all three merge)
5. **Conflicts**: detected independently of blocking — scan all objects for same normalised name
   with different ObjectKind

### Implementation details worth remembering

- **`jaro("", "") = 1.0`**: The jaro function returns 1.0 for identical strings, including
  two empty strings. This is the correct mathematical interpretation (both are "equal") — don't
  change it to 0.0.

- **Blocking prevents cross-kind merges**: Since blocks are keyed by ObjectKind, a Table named
  "customer" and an Entity named "customer" will never be in the same block, so they can't merge
  — but the conflict detection pass catches them separately.

- **Union-Find root determines canonical**: The canonical object in a merge group is whatever
  index is the UF root. This is deterministic within a single run but not across runs (object
  ordering in the graph varies). Phase 9+ should pin canonical selection by heuristic (e.g.,
  prefer SQL-derived names or highest-evidence objects).

- **`crates/identity` has no async, no compiler-core dep**: It only depends on `ekos-kir`,
  `serde`, and `serde_json`. This makes it reusable as a standalone library for other tools.

### Decisions

- **Jaro-Winkler over Levenshtein**: rewards common prefixes (e.g., `tbl_customer` vs `customer`
  after normalisation → `customer` vs `customer` → 1.0). Edit distance would penalise the
  prefix uniformly.

- **No write-back in Phase 7**: `ekos resolve` is read-only. The canonical identity store is Phase
  9 (Ledger). Writing resolutions before the ledger exists would require a second migration.

- **Conflict = non-zero exit**: `ekos resolve` exits non-zero when conflicts are found. This makes
  it CI-friendly: pipe `ekos resolve` into a gate that blocks the build when naming inconsistencies
  are detected.

---

## RFC Written

- **RFC 0007** (`docs/rfcs/0007-identity-resolution.md`) — algorithm, blocking strategy, JW
  rationale, Phase 7 limitations

---

## Knowledge Captured

- **Blocking key must include ObjectKind**: If you only block by name prefix, a Table and an
  Entity with similar names end up in the same block and get a merged proposal — which is wrong
  (cross-kind merging requires semantic understanding, deferred to Phase 10+).

- **Transitivity via Union-Find is essential**: Naive pairwise output misses groups of three or
  more related objects. With `customer` → `customers` → `customer_table` as three separate
  proposals, the downstream consumer would need to join them. Union-Find collapses them to one.

- **Jaro match window is `max(len1, len2)/2 - 1`**: Use integer division (floor), not ceiling.
  Off-by-one here produces wrong transposition counts on short strings.

---

## Files Changed

| File | Change summary |
|---|---|
| `crates/identity/src/lib.rs` | Full implementation replacing stub |
| `crates/identity/src/similarity.rs` | New: normalize + jaro + jaro_winkler (8 tests) |
| `crates/identity/Cargo.toml` | Added serde, serde_json |
| `crates/cli/src/commands/resolve.rs` | New: ekos resolve command |
| `crates/cli/src/commands/mod.rs` | pub mod resolve |
| `crates/cli/src/bin/ekos.rs` | Resolve subcommand |
| `crates/cli/Cargo.toml` | ekos-identity dependency |
| `docs/rfcs/0007-identity-resolution.md` | RFC 0007 — accepted |

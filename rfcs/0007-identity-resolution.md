# RFC 0007 — Identity Resolution

**Status:** Accepted  
**Phase:** 7  
**Crate:** `ekos-identity`

---

## Problem

After Phase 6, the knowledge graph contains multiple `KirObject`s that refer to the same
real-world concept but were named differently across sources:

- `Customer` (SQL table) vs `client` (git commit messages) vs `Buyer` (Confluence)
- `orders` (SQL) vs `order` (API endpoint name)
- `PRODUCT` (legacy system) vs `Product` (new microservice)

Without identity resolution every downstream query returns fragmented, duplicated results.

---

## Algorithm

### 1. Candidate Extraction

All `KirObject`s in the input `KirGraph` become candidates. Candidates are evaluated by
name and structural similarity; events and relationships are not directly resolved but are
updated to reference the canonical ID after merging.

### 2. Name Normalisation

Before comparison, names are normalised:
1. Lowercase
2. Replace `_`, `-`, `.` with a space
3. Strip known table/entity suffixes: `table`, `tbl`, ` dim`, ` fact` (with surrounding spaces)
4. Collapse whitespace; trim

Examples: `CUSTOMER_TABLE` → `customer`, `OrderItems` → `orderitems`, `tbl_product` → `product`

### 3. Blocking

To avoid O(n²) comparisons, candidates are partitioned into **blocks** keyed by
`(ObjectKind, first 3 chars of normalised name)`. Only candidates in the same block
are compared with each other.

This is an over-approximation: some non-matching pairs will be compared, but no true
match will be missed as long as two synonymous concepts share the same `ObjectKind` and
their normalised names agree on the first three characters. The TODO notes that
cross-kind synonym detection (Phase 10+) requires the LLM semantic layer.

### 4. Similarity Scoring

Within each block, all pairs are scored:

```
name_score      = Jaro-Winkler(normalised_a, normalised_b)
structural_score = 1.0 if same ObjectKind else 0.0
combined        = 0.7 * name_score + 0.3 * structural_score
```

Blocking ensures candidates in the same block always share `ObjectKind`, so
`structural_score` is always 1.0 for in-block comparisons.

**Jaro-Winkler** is chosen over Levenshtein because:
- It handles common naming conventions well (prefix matches, short names)
- O(n·m) time with no additional allocation for the Jaro step
- The Winkler prefix bonus rewards naming conventions like `tbl_customer` vs `customer`

### 5. Merge Threshold

Default: **0.85**. Pairs above this threshold are merged. Configurable via `ResolverConfig`.

### 6. Transitivity via Union-Find

If A matches B and B matches C, all three merge into one canonical group. Path-compressed
Union-Find with union-by-rank achieves near-O(n·α(n)) time.

### 7. Conflict Detection

Independent of blocking. Groups objects by normalised name; if two objects share the same
normalised name but different `ObjectKind`, a `ConflictReport(SameNameDifferentKind)` is
emitted. These are reported but do not block the merge of other objects.

### 8. Canonical Selection

Within a merge group, the **canonical object** is the one with the root index in the
Union-Find structure (typically the first object encountered). The canonical name and kind
are taken from this object. Future work (Phase 10+) can apply heuristics such as preferring
the SQL-derived name or the most-evidenced object.

---

## Output

```rust
pub struct ResolutionResult {
    pub proposals: Vec<MergeProposal>,   // one per merged group (size ≥ 2)
    pub conflicts: Vec<ConflictReport>,  // same name, different kind
    pub stats: ResolutionStats,          // counters for observability
}
```

`ResolutionResult` is serialisable. The `ekos resolve` command writes it to stdout as
structured text and exits non-zero if any conflicts are detected.

---

## Limitations in Phase 7

- Synonym detection across different names (`Customer` vs `Buyer`) requires semantic LLM
  enrichment (Phase 10+).
- Cross-kind resolution (a `Table` object and a `Service` object for the same concept) is
  deferred.
- No write-back to the artifact store in Phase 7; the resolver is a pure read + report tool.
  Phase 9 (Ledger) will own the canonical identity store.

---

## Alternatives Considered

- **TF-IDF + cosine similarity**: Good for long text, overkill for short object names.
- **Phonetic encoding (Soundex/Metaphone)**: English-language bias; enterprise names are
  technical identifiers, not natural-language words.
- **Pure Levenshtein**: Does not reward common prefix conventions; O(n·m) with larger
  constants than Jaro in practice for short identifiers.

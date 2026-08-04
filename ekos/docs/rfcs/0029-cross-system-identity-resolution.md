# RFC 0029 — Cross-System Identity Resolution

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-04
**Gating:** Phase 4 of `ekos-transformation-semantics-plan.md` — deliberately last among the
Transformation IR phases, after RFC 0027's IR (Phase 1), both producers (Phases 3/2), and both
consumers (Phase 5's MCP tools, Phase 6's agents) had already shipped and been rehearsed for real.
RFC 0027 explicitly deferred this: *"identity resolution across systems... produces hypotheses,
not facts, and needs its own explicit trust/confidence status in the ledger... this may need its
own follow-up RFC rather than being folded in here."* This is that RFC.

---

## Motivation

Target scenario (`ekos-transformation-semantics-plan.md`'s own framing): the same real-world
customer table is observed under three different names across three different systems — Informix
`cust_mstr` (schema-only, no source repo), Postgres `customers`, Databricks `gold.dim_customer`.
Today, Phase 1–3's Transformation IR keeps each `Source`/`Sink` node's `object_name` as a raw,
unresolved string exactly as written in its source (RFC 0027's own explicit design choice — see
its "Alternatives Considered"). Nothing links these three strings together, so
`ekos_transformation_explain` on a Pentaho job reading `cust_mstr` and `ekos_transformation_diff`
against a new SQL pipeline reading `customers` cannot recognize they touch the same entity — an
agent doing the plan's target migration would have to notice this by eye.

This is architecturally different from every other pass shipped so far. Every Transformation IR
producer (Phases 2/3) and the existing intra-compile `DefaultResolver` (RFC 0007, used by
`ekos resolve`/the semantic compiler's `apply_merges`) either records a deterministic fact or
merges duplicates the resolver is confident enough about to fold silently before the CKM is ever
built. Cross-system name matching is neither: `cust_mstr` = `customers` is a *hypothesis* inferred
from column overlap and naming similarity, not observed. Getting it wrong silently (auto-merging
two genuinely different tables, or two views of the same aliased identifier) corrupts every
downstream MCP tool's answer with unrecoverable confidence. This RFC's central design constraint,
stated in RFC 0027 and reaffirmed here: **a candidate cross-system match must be recorded as an
explicit, reviewable hypothesis — never silently merged, never indistinguishable from a directly
observed fact** — until a human or agent confirms it.

## Design

### Why this cannot reuse `DefaultResolver` (RFC 0007)

`DefaultResolver::resolve` (`crates/identity/src/lib.rs`) already does structural scoring —
including column-name Jaccard overlap (`structural_score`, requiring `a.kind == b.kind`) — but its
whole design is oriented around "confident enough to auto-merge before the CKM exists," consumed
in-memory by `apply_merges` with no persistence and no review step. It also **already deliberately
excludes `Custom("TransformNode")` objects from blocking entirely** (RFC 0027/0028's fix for the
Section-shaped over-merge bug found live in Phase 6's rehearsal) — correct there, because no two
`TransformNode` objects parsed from the same or different files should ever be blindly merged by
name-prefix similarity. Cross-system identity needs the *opposite* posture for a *different*
comparison: not "are these the same parsed node" but "do this `Source`/`Sink`'s `object_name` and
some `Table`'s name plausibly refer to the same real-world entity," scored primarily on evidence
`DefaultResolver` doesn't use for this purpose (naming-pattern normalization across schema
prefixes and ETL affixes) and — critically — never applied automatically. This is a **new,
separate resolver**, not a config change to `DefaultResolver`.

### `CrossSystemScorer` — `crates/identity/src/cross_system.rs`

```rust
pub struct CrossSystemCandidate {
    pub a: KirId,
    pub b: KirId,
    pub confidence: f32,
    pub signals: CrossSystemSignals, // {column_overlap, name_pattern, type_compat: Option<f32>}
}

pub fn find_cross_system_candidates(objects: &[KirObject]) -> Vec<CrossSystemCandidate>
```

Targets exactly the two "table-like" shapes the plan's scenario needs to link: `ObjectKind::Table`
(recovered by `sql_analyzer.rs`'s DDL pass) and `Custom("TransformNode")` objects where
`properties["node_type"]` is `"Source"` or `"Sink"` (RFC 0027) — the only KIR shapes that carry an
`object_name`/`columns` pair meaningful for this comparison. Every candidate **pair** is scored
(this is deliberately not blocked/bucketed the way `DefaultResolver` is — the whole point is
linking objects whose names *don't* share a prefix, so name-prefix blocking would defeat the
feature outright; the target object count for this comparison is small enough in practice — table-
and pipeline-endpoint objects, not every KIR object — that O(n²) is acceptable for v1, flagged as
an Open Question if a real workspace's object count makes it not).

Three signals, weighted, each degrading gracefully when its input is absent rather than penalizing
the pair:

1. **Column-name overlap** — Jaccard of lowercased column-name sets, reusing
   `crates/identity/src/lib.rs`'s existing `jaccard`/column-extraction logic (factored out to a
   small shared helper rather than duplicated). Both `Table` and `TransformNode` `Source`/`Sink`
   objects carry a `columns` property, though `TransformNode` columns are frequently empty in
   today's MVP producers (Phase 2/3 leave `columns: []` in the common case) — an honest, known
   limitation: this signal degrades toward "no evidence" for most Pentaho/SQL-recovered pipeline
   endpoints today, not a bug in this RFC.
2. **Naming-pattern similarity** — each name is normalized (lowercase; strip a schema/catalog
   prefix before the last `.`; strip a small fixed list of common ETL affixes — `mstr`, `tbl`,
   `dim`, `fact`, `stg`, `raw`, as free-standing `_`-delimited tokens) before Jaro-Winkler scoring,
   so `gold.dim_customer` → `customer`, `cust_mstr` → `cust`, `customers` → `customers` are compared
   on their stripped forms rather than raw strings that share little literal overlap. This is
   explicitly a **v1 heuristic list**, not a claim of completeness — real Informix/Pentaho naming
   conventions vary by organization; extending the affix list from real usage is expected future
   tuning, not a design flaw to fix now.
3. **Column-type compatibility** — only computed when both sides carry typed columns (only `Table`
   objects do, via `properties["columns"][].data_type`; `TransformNode` columns are name-only) —
   Jaccard of normalized type-family strings (e.g. `int`/`integer`/`bigint` bucketed to one family)
   for name-matching columns. `None` (excluded from the weighted average, not scored as 0) when
   either side lacks type info — most `Table`-vs-`TransformNode` comparisons, honestly, since only
   `sql_analyzer.rs`'s DDL objects carry types today.

`confidence = weighted average of available signals`, weights `{column_overlap: 0.4, name_pattern:
0.4, type_compat: 0.2}` renormalized over whichever signals are actually available for that pair
(all `TransformNode`-vs-`TransformNode` or `TransformNode`-vs-`Table` pairs without types fall back
to `{column_overlap: 0.5, name_pattern: 0.5}`). A floor (`MIN_CANDIDATE_CONFIDENCE = 0.3`) excludes
obvious non-matches from ever being written — everything at or above the floor is written as a
candidate for review, deliberately including low-confidence ones (a human should see "0.35,
probably not" as much as "0.9, probably yes"; silently dropping borderline candidates would hide
real matches the heuristic under-scores, which is a worse failure mode than a noisy queue).

### Storage: `Relationship` with an explicit status, never a plain fact

Per RFC 0027/the plan's own instruction, each candidate is written as:
```rust
KirRelationship {
    kind: RelationshipKind::Custom("SameAs".to_string()),
    from: candidate.a,
    to: candidate.b,
    properties: {
        "status": "unconfirmed",       // "unconfirmed" | "confirmed" | "rejected"
        "confidence": candidate.confidence,
        "column_overlap": ..., "name_pattern": ..., "type_compat": ...  // per-signal breakdown
    },
    evidence: [ev_id],  // KirEvidence citing the specific signal values that produced the score
    ..
}
```
`KirRelationship` has no dedicated status field (`crates/kir/src/lib.rs`) — `status` lives in
`properties`, the same place RFC 0026 put `source_prompt_version` and RFC 0027 put `node_type`; no
core-type change needed, consistent with `RelationshipKind::Custom(...)`'s established "no
exhaustive match anywhere" safety argument. **This relationship is never consumed by
`DefaultResolver`/`apply_merges`** — those never look at `Custom("SameAs")` at all, so an
unconfirmed (or even confirmed) cross-system match can never silently affect the CKM merge
pipeline; only an agent/human explicitly reading and acting on it (e.g. via
`ekos_transformation_explain` choosing to treat two objects as the same when reasoning) does
anything with it. This is the concrete mechanism that satisfies "hypothesis, not fact, with its
own explicit trust/confidence status."

### New CLI entry point: `ekos identity scan`

`ekos resolve` (RFC 0007) cannot host this: it operates on pre-ledger `KirGraph`s assembled
directly from artifacts, has no ledger write path, and only ever prints proposals — confirmed by
reading `crates/cli/src/commands/resolve.rs` in full. Cross-system candidates need to read
**already-committed ledger objects** (this is explicitly a cross-*system* comparison, meant to run
after normal recovery/compile/commit has populated the ledger from every connector) and need to
**write** to it. New command, `crates/cli/src/commands/identity.rs`, `ekos identity scan`:
1. `ledger.all_objects()`, filter to `Table` + `TransformNode` `Source`/`Sink`.
2. `find_cross_system_candidates(&objects)`.
3. For each candidate at/above the floor, skip if an existing `Custom("SameAs")` relationship
   already connects the same pair (idempotent re-scan — re-running `ekos identity scan` after new
   objects arrive must not spam duplicate unconfirmed candidates for pairs already reviewed or
   already queued).
4. `ledger.append_relationship(...)` + `ledger.append_evidence(...)` per new candidate.
5. Print a summary: candidates found, by confidence band, and how many were skipped as
   already-known.

### `ekos_identity_review(relationship_id, decision)` — the first write-capable MCP tool

Every existing MCP tool (`crates/cli/src/commands/mcp.rs`) is read-only, built over `Runtime`,
consistent with `CLAUDE.md`'s "the Runtime is read-only" invariant. This tool is a deliberate,
explicit exception, not a violation of that invariant: **`Runtime` itself is not touched** — this
tool bypasses `Runtime` entirely and calls `KnowledgeStore::append_relationship`/`append_event`
directly on the opened `ledger`, the exact same interface `ekos commit`/`ekos identity scan` already
write through outside the MCP process. The invariant "AI systems consume knowledge through the
Runtime only" governs *reading*; this is the one place an agent *acts* — explicitly named and
scoped by the plan itself ("lets an agent or human confirm or reject a candidate match"), on one
narrow relationship kind, through the same append-only, evidenced write path every other ledger
mutation already uses. No raw enterprise system is touched, no other object kind is writable
through this tool, and the write is itself append-only (a new version at the same relationship id,
per the ledger's existing versioning contract — never an in-place edit).

```rust
"ekos_identity_review" => {
    let rel_id = required_id_field(args, "relationship_id")?;
    let decision = required_str(args, "decision")?; // "confirmed" | "rejected"
    let mut rel = ledger.get_relationship(&rel_id)?
        .ok_or_else(|| anyhow::anyhow!("relationship not found: {rel_id}"))?;
    if !matches!(rel.kind, RelationshipKind::Custom(ref k) if k == "SameAs") {
        anyhow::bail!("not a SameAs candidate: {rel_id}");
    }
    rel.properties.insert("status".into(), json!(decision));
    rel.properties.insert("reviewed_at".into(), json!(Utc::now().to_rfc3339()));
    ledger.append_relationship(&rel)?;

    let event_kind = if decision == "confirmed" { EventKind::Merged } else { EventKind::Modified };
    let event = KirEvent { id: KirId::new(), kind: event_kind, subject: rel_id,
        payload: json!({"decision": decision, "relationship_id": rel_id.to_string()}),
        evidence: vec![], occurred_at: Utc::now() };
    ledger.append_event(&event)?;

    Ok(json!({ "relationship_id": rel_id.to_string(), "decision": decision, "status": "recorded" }))
}
```
`decision` is validated against exactly `{"confirmed", "rejected"}` — anything else is a tool
error, never silently accepted. Only `Custom("SameAs")` relationships are reviewable through this
tool — attempting to "confirm" an unrelated relationship kind (e.g. a `ForeignKey`) is rejected,
keeping this tool's write surface narrow and auditable.

### New ledger surface: `append_event`/`get_event`

**`EntryType::Event` and `KirEvent`/`EventKind` already exist in the schema but have never been
written anywhere in this codebase** — confirmed by grepping the whole workspace: no
`append_event` method exists on `Ledger`, `FactLedger`, or the `KnowledgeStore` trait, and
`EventKind::Merged` is never constructed outside test fixtures. `FactLedger`'s
`kind_of_payload`/`EntityKind::Event` dispatch (`crates/ledger/src/fact_ledger.rs`) is *already*
fully wired to recognize an event payload (`has("subject")`) — only the public
`append_event`/`get_event` wrapper methods are missing, mirroring `append_evidence`/`get_evidence`
exactly (`Ledger`'s SQLite backend needs the same two methods added, using `EntryType::Event` and
`self.append(&entry)`, the same non-versioned-insert shape `append_evidence` already uses — an
event is an immutable log entry, not a "current state" object needing a version index). Both
methods are added to the `KnowledgeStore` trait and `delegate_store!` macro. This is new surface,
not a reuse of existing machinery — the plan's own language ("confirmation writes a new Event to
the ledger") is what makes this phase the first real consumer of a schema element that has existed
since early phases but sat unused.

### Observation-layer-facts-only invariant — explicitly addressed

`CLAUDE.md`'s architecture states the Observation Layer records facts; `find_cross_system_candidates`
is **not** an observation-layer pass — it runs over already-recovered, already-committed ledger
objects, is not registered as a `CompilerPass`/`Observer`, and produces a candidate relationship
explicitly tagged `status: "unconfirmed"`, structurally distinguishable from every fact-bearing
relationship in the ledger (`ForeignKey`, `FeedsInto`, `Contains`, etc., which carry no `status`
property at all and are never subject to review). An agent or downstream tool reading the ledger
can trivially tell a `Custom("SameAs")` relationship apart from an observed fact — by kind, and
additionally by the presence of `properties["status"]` — and must treat an `unconfirmed` one as
exactly what it is: a hypothesis, not knowledge.

## Alternatives Considered

- **Extending `DefaultResolver`/`ResolverConfig` with a "cross-system" mode** — rejected: RFC
  0027/0028 already relies on `DefaultResolver` correctly *excluding* `Custom("TransformNode")`
  from blocking to avoid the Section-shaped over-merge; folding cross-system matching into the same
  resolver would require it to simultaneously exclude TransformNode from same-file blocking and
  include it for cross-system comparison — two incompatible postures in one resolver. A separate
  module with its own entry point is simpler and keeps `DefaultResolver`'s existing, already-fixed
  behavior untouched.
- **Auto-confirming above a high-confidence threshold** — rejected, explicitly, per the plan's own
  instruction ("never as a plain fact indistinguishable from directly observed relationships") and
  RFC 0027's framing ("hypotheses, not facts"). Even a 0.95-confidence naming-pattern match can be
  a false positive (two genuinely different "customer" tables in unrelated business units); the
  cost of a silent wrong merge (corrupting every downstream tool's answer) outweighs the convenience
  of skipping review for the highest-confidence tier. `identity-reviewer` (Phase 6, already written)
  already anticipates a human/agent batch-reviewing high-confidence candidates quickly — that is the
  intended fast path, not automatic confirmation.
- **A dedicated `SameAsCandidate` KIR primitive instead of `RelationshipKind::Custom("SameAs")`** —
  rejected: would require touching `KirRelationship`'s core shape or adding a fifth ledger primitive
  beyond Object/Relationship/Event/Evidence, for no benefit `Custom(...)` + `properties["status"]`
  doesn't already provide, and inconsistent with every prior RFC's (0024/0026/0027) established use
  of `Custom(...)` for new semantic concepts.
- **A separate, dedicated `Ledger` primitive for "candidate matches" instead of reusing
  `Relationship`** — rejected per the plan's own explicit instruction: "Store each candidate match
  as a Relationship in the ledger with an explicit status field."

## Testing

- `crates/identity/src/cross_system.rs`: unit tests per signal (column-overlap Jaccard, name-
  pattern normalization+Jaro-Winkler on the `cust_mstr`/`customers`/`gold.dim_customer` scenario
  itself, type-compat degrading to `None` when absent), plus a full
  `find_cross_system_candidates` test proving the three-system scenario produces at least one
  candidate pair above the floor, and a negative test proving two genuinely unrelated tables (no
  column overlap, dissimilar names) score below the floor and produce no candidate.
- `crates/ledger/src/lib.rs` + `fact_ledger.rs`: `append_event`/`get_event` round-trip tests on
  both backends (this is new ledger surface with zero prior test coverage — mandatory before
  relying on it).
- `crates/cli/src/commands/identity.rs`: `ekos identity scan` writes exactly one `unconfirmed`
  `Custom("SameAs")` relationship per candidate above the floor; re-running the scan does not
  duplicate an already-written candidate for the same pair (idempotency).
- `crates/cli/src/commands/mcp.rs`: `ekos_identity_review` confirm/reject round-trip (status
  updated, `KirEvent` written with the right `EventKind`), rejecting an invalid `decision` value as
  a tool error, rejecting a non-`SameAs` relationship id as a tool error, unknown relationship id
  as a tool error.

## Acceptance Criteria

- [x] `find_cross_system_candidates` implemented in `crates/identity/src/cross_system.rs`,
      producing confidence-scored candidates over `Table` and `TransformNode` `Source`/`Sink`
      objects, with graceful signal degradation when column/type data is absent.
- [x] Candidates are written as `RelationshipKind::Custom("SameAs")` with
      `properties["status"] = "unconfirmed"`, never auto-merged, never consumed by
      `DefaultResolver`/`apply_merges`.
- [x] `ekos identity scan` CLI command reads ledger objects, writes new candidates, is idempotent
      against already-known pairs.
- [x] `ekos_identity_review(relationship_id, decision)` MCP tool implemented; the first
      write-capable tool, explicitly scoped to `Custom("SameAs")` relationships only, validated
      `decision` values only, going through the same `KnowledgeStore` write path as every other
      ledger mutation.
- [x] `append_event`/`get_event` added to `KnowledgeStore`, `Ledger`, and `FactLedger` — the first
      real usage of `EntryType::Event`/`KirEvent` in this codebase.
- [x] All new/updated tests pass; `cargo test --workspace`, `cargo clippy --workspace --all-targets`,
      `cargo fmt --check` clean; zero `unsafe` introduced.
- [x] `demo/agents/identity-reviewer.md`'s Status note (Phase 6) is no longer accurate once this
      ships — it must be updated to remove the "not yet wired" caveat as part of this RFC's
      implementation, not left stale.

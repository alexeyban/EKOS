# RFC 0027 — Unified Transformation Semantics

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-04
**Gating:** Phase 1 (Transformation IR), Phase 2 (SQL analysis), Phase 3 (Pentaho plugin). Does
not itself implement anything — this RFC defines the shared target representation and the
compiler-pass contract every format-specific parser compiles into. Cross-system identity
resolution over Transformation-IR objects (matching a Pentaho step's target table to a
SQL-parsed `Table`, etc.) is explicitly **out of scope** — see Open Questions.

---

## Motivation

Target scenario: a company with 1000+ GitHub repos, PostgreSQL, Databricks, Synapse Pipelines
(metadata in Postgres), an Informix DB with schema only (no source repo), an outdated Confluence,
and legacy Pentaho (Kettle) ETL jobs. A developer needs to reproduce the business logic of an
existing Pentaho job in a new pipeline, with a new rule applied. Today that means manually reading
`.ktr`/`.kjb` XML and hunting for tribal knowledge in Confluence — exactly the kind of recovery
work EKOS exists to eliminate.

EKOS has no unified way to represent "a transformation of data from sources to a sink through
filter/join/aggregate/calculate operations" — the concept underlying a Pentaho step, a SQL
`SELECT`, a `CREATE VIEW`, and a stored procedure alike. Without a shared representation, logic
recovered from one format cannot be compared or diffed against logic recovered from another, which
blocks the core use case: comparing legacy Pentaho logic to a newly drafted pipeline to verify the
migration preserves intended behavior modulo the one changed rule.

Building a separate extraction path per format (Pentaho → its own ad hoc KIR shape, SQL → a
different ad hoc shape) would produce N incompatible semantic models that cannot be diffed against
each other. That defeats the point. This RFC introduces one Transformation IR that every
format-specific plugin compiles into, so `ekos_transformation_diff` (a later phase) is comparing
apples to apples regardless of source format.

`Unmapped` is deliberate, not a placeholder: anything that cannot be parsed or classified must
still be recorded as a fact ("something is here, not yet understood") rather than silently
dropped — that is itself Evidence, and an explicit signal for where recovery or a human needs to
step in, rather than a silent gap in coverage.

## Design

### Where this fits in the existing architecture

Every format-specific plugin (a future Pentaho `Observer`, the existing SQL observation path) emits
raw facts as `ObservationArtifact`s, exactly as today. A new `CompilerPass` per format —
`SqlTransformAnalyzerPass`, `PentahoAnalyzerPass` (Phase 2/3, not this RFC) — reads those artifacts,
deterministically parses them into a `TransformGraph` (this RFC's new type), and writes that graph
as `KirObject`/`KirRelationship`/`KirEvidence` into a `KnowledgeArtifact`, following the exact
shape `SqlAnalyzerPass` (`crates/recovery/src/sql_analyzer.rs`) and `DependencyAnalyzerPass`
(`crates/recovery/src/dependency_analyzer.rs`) already use. This RFC defines:

1. The `TransformNode`/`TransformGraph` intermediate representation (a new module — see "Crate
   placement" below).
2. How `TransformNode`s map onto `KirObject`/`KirRelationship`/`KirEvidence` once a pass is ready
   to write to the ledger.
3. The observation/interpretation boundary that governs what a *parsing* pass (this RFC) is
   allowed to do versus what a later *classification* pass (future work) does.

It does **not** define the Pentaho or SQL parsers themselves (Phases 2/3), the two new MCP tools
(`ekos_transformation_explain`/`ekos_transformation_diff`, Phase 5, its own RFC per existing
process), or cross-system identity resolution over Transformation-IR objects (Phase 4, its own
RFC per the existing precedent set by RFC 0026's Concept-merge design).

### The IR

```rust
/// A single node in a Transformation IR graph. Every format-specific parser
/// (SQL, Pentaho, stored-procedure embedded-SQL) compiles into this shared
/// vocabulary so graphs from different source formats can be diffed.
pub enum TransformNode {
    /// A read from a table/view/file. `object` is left unresolved at parse
    /// time (see "Observation vs. interpretation" below) — a `Custom`
    /// ObjectKind name string, not a resolved KirId.
    Source {
        object_name: String,
        columns: Vec<String>,
    },
    /// A row-filtering predicate, kept as parsed AST text, not evaluated.
    Filter {
        condition: String,
    },
    Join {
        left: NodeId,
        right: NodeId,
        keys: Vec<(String, String)>,
        kind: JoinKind, // Inner, Left, Right, Full, Cross
    },
    Aggregate {
        group_by: Vec<String>,
        aggs: Vec<AggExpr>, // { output: String, func: String, arg: String }
    },
    Calculate {
        output: String,
        expr: String,
    },
    /// A write to a table/view/file — the mirror of Source.
    Sink {
        object_name: String,
        columns: Vec<String>,
    },
    /// Deliberate, not a fallback-to-error: anything that could not be
    /// parsed or classified into the above, preserved verbatim as evidence
    /// that something is here and not yet understood.
    Unmapped {
        raw: String,
        reason: String,
    },
}

pub struct NodeId(pub u32); // index into TransformGraph::nodes, graph-local only

pub struct TransformGraph {
    pub nodes: Vec<TransformNode>,
    /// Data-flow edges, source-node-id -> consuming-node-id, in parse order.
    pub edges: Vec<(NodeId, NodeId)>,
    /// Where this graph came from — one Pentaho step file, one SQL object,
    /// one stored-procedure body — carried through to evidence generation.
    pub origin: TransformOrigin,
}

pub struct TransformOrigin {
    pub source_path: String,     // file path or DB object identifier
    pub source_kind: String,     // "pentaho-ktr" | "sql-select" | "sql-view" | "stored-procedure" | ...
    pub extracted_at: DateTime<Utc>,
}
```

Field choices deliberately favor "string, kept close to the source text" over "resolved reference,
requiring a lookup" for anything that would otherwise force premature interpretation:
`Source.object_name`/`Sink.object_name` are raw identifiers as written in the source (e.g.
`dbo.cust_mstr`, not a resolved `KirId` pointing at a specific `Table` object) — resolving that
name to a concrete cross-system `Table` object is Phase 4's job (identity resolution), which
operates on the KIR graph *after* this IR has already been lowered into it, not during parsing.
Likewise `Filter.condition`/`Calculate.expr` keep the parsed expression as text rather than a typed
`Expr` AST — the design-sketch draft in the implementation plan proposed a typed `Expr`, but a
shared cross-format expression AST (reconciling SQL scalar expressions, Pentaho's Janino/JS
calculator syntax, and T-SQL/PL/pgSQL expressions) is a substantial project on its own with no
immediate consumer; `ekos_transformation_diff` (Phase 5) can do useful structural diffing (are the
join keys the same, is there a filter here that wasn't there before) without parsing expression
bodies into a typed tree. Revisiting this as a typed `Expr` is listed under Open Questions,
not decided against permanently.

### Crate placement

New module `crates/semantic/src/transform_ir.rs`, not a new crate. `crates/semantic` is described
in `CLAUDE.md`'s workspace layout as "Semantic compiler: Recovered Knowledge → CKM" — the
Transformation IR is exactly a semantic-layer shared vocabulary sitting between per-format parsing
and the ledger, matching that crate's existing charter. A new crate would need its own
`compiler-sdk` trait surface and dependency wiring for zero additional benefit: `TransformNode`/
`TransformGraph` are plain serializable data types with no trait to implement, unlike
`observation-sdk`'s `Observer` (a real extension point multiple independent plugin crates
implement) — the precedent for a dedicated crate. The format-specific *parsers* (Phase 2's SQL
analyzer, Phase 3's Pentaho plugin) are the actual extension points and follow `observation-sdk`
patterns as `CompilerPass`es in `crates/recovery` (SQL) and a new `plugins/pentaho` `Observer`
(Pentaho), same shape as every existing connector — `crates/semantic` only holds the shared target
type they compile into.

### Lowering into KIR

A `TransformGraph` becomes ledger-writable KIR via a `lower_to_kir(&TransformGraph) -> KirGraph`
function (also in `crates/semantic/src/transform_ir.rs`), called by each format-specific pass after
parsing, mirroring how `sql_analyzer.rs::parse_ddl_structural` returns a `KirGraph` directly today.
Mapping:

| TransformNode | KIR shape |
|---|---|
| `Source` / `Sink` | `KirObject::new(object_name, ObjectKind::Custom("TransformNode"))`, `properties["node_type"] = "Source"` or `"Sink"`, `properties["columns"]` |
| `Filter` / `Calculate` | Same `Custom("TransformNode")` kind; `properties["excerpt"]` set to the condition/expr text — this is deliberate, not incidental: `KirObject::indexed_content()` (`crates/kir/src/lib.rs`) reads `properties["excerpt"]` as the one field FTS indexes, so a filter predicate or calculated-field formula becomes searchable via `ekos_search`/`ekos ask` for free, the same mechanism RFC 0026 relies on for Concept text |
| `Join` / `Aggregate` | Same `Custom("TransformNode")` kind; join keys / group-by+agg list serialized into `properties` |
| `Unmapped` | Same `Custom("TransformNode")` kind, `properties["node_type"] = "Unmapped"`, `properties["raw"]`, `properties["reason"]` — never dropped, always a real KirObject with real Evidence, exactly like every other node |
| graph edges | `KirRelationship::new(RelationshipKind::Custom("FeedsInto"), from_node_id, to_node_id)` per `TransformGraph::edges` entry |
| `TransformOrigin` | One `KirEvidence` per node: `SourceLocation::file(origin.source_path)`, `fragment` = the node's raw text (condition/expr/raw, or object_name for Source/Sink), attached via `KirObject::with_evidence` |

A single new `ObjectKind::Custom("TransformNode")` (not a variant per node type) keeps every
Transformation IR node queryable as one kind via existing generic tools (`ekos_search`,
`ekos_neighborhood`) without new `ObjectKind` variants — `node_type` in `properties` disambiguates,
following exactly the precedent RFC 0026 set for `Custom("Concept")`/`Custom("Section")` rather
than growing the core `ObjectKind` enum, whose doc comment (`crates/kir/src/lib.rs`) already
states new variants are low-risk *because* nothing exhaustively matches on it — but `Custom(...)` is
the established idiom for a new semantic concept and is used here for consistency with the two
most recent precedents (RFC 0024's `Section`, RFC 0026's `Concept`).

### Determinism and content-addressability

Every `TransformNode`/`TransformGraph` field is a plain `String`/`Vec`/enum — no floats, no
`HashMap` (which does not serialize deterministically without explicit key sorting), no
wall-clock-dependent value inside the hashed content. `TransformGraph` derives `Serialize` and is
passed through `ekos_artifact::compute_content_id`, which canonicalizes JSON (recursively sorts
object keys) before hashing — the same mechanism `ObservationArtifact`/`KnowledgeArtifact` already
rely on. `TransformOrigin.extracted_at` is the one timestamp field on the graph; it lives on
`origin`, which is included in the graph's serialized content like everything else, deliberately —
unlike `ArtifactMeta.created_at`, which the artifact layer explicitly excludes from its hash,
`extracted_at` is a meaningful part of *this* content's identity (a re-parse of the same Pentaho
step tomorrow is a new fact, not the same fact re-observed), so it is intentionally hashed, not
stripped. Content addressability holds without needing to special-case any field.

**Required test** (per Phase 1's TDD requirement in the implementation plan): one test per
`TransformNode` variant asserting `compute_content_id` returns byte-identical output across two
independent constructions of the same logical node, and a differing output when any single field
changes.

### Observation vs. interpretation boundary

`CLAUDE.md`'s architecture states AI systems never touch raw enterprise systems and the Observation
Layer's job is fact collection. The implementation plan directly asks where deterministic
SQL/Pentaho parsing sits relative to that line. Answer, argued explicitly:

**Parsing a `.ktr`/`.kjb` file's XML, or a SQL statement's grammar, into `TransformNode`s is still
fact collection**, for the same reason `sql_analyzer.rs`'s existing `parse_ddl_structural` (DDL →
`KirObject`/`KirRelationship`, no LLM) is already accepted as a structural, non-interpretive pass:
the mapping from source syntax to `TransformNode` shape is a pure function of the grammar — the
same `.ktr` XML byte-for-byte always parses to the same `TransformGraph`, with zero judgment calls.
A `FilterRows` step with condition `status = 'active'` becomes exactly one `Filter { condition:
"status = 'active'" }` node every time, regardless of what business rule that condition happens to
encode. This is why `Filter.condition`/`Calculate.expr` are kept as raw text (see "The IR" above)
rather than evaluated or semantically labeled at parse time — evaluating/labeling would be the
interpretive step, and this RFC's passes deliberately stop short of it.

**Classifying *what a step means* for the business — "this `FilterRows` enforces the
active-customers-only business rule" — is recovery-layer interpretation**, not observation, and is
explicitly out of scope for the passes this RFC gates. That labeling is exactly the kind of
LLM-assisted enrichment `SqlAnalyzerPass::apply_llm_enrichment` and RFC 0026's
`DocumentSemanticsAnalyzerPass` already do as a *second*, clearly-separated step on top of a
structural pass's output — never blocking the structural graph, always degrading to a diagnostic
warning on failure. A future `TransformSemanticsAnalyzerPass` doing this for Transformation IR
nodes (the LLM-enrichment analogue of RFC 0026, reading `Custom("TransformNode")` objects and
proposing business-meaning descriptions in `properties["business_meaning"]`, evidenced and
never fabricated as fact) is anticipated future work, explicitly not part of this RFC or Phases
1–3 of the implementation plan — it is a natural Phase 6 (`legacy-logic-recoverer` agent) input
but does not need to exist for Phases 1–3 to be useful: an agent can already read raw
`Filter.condition` text via `ekos_transformation_explain` and reason about business meaning
itself, same as it does today reading raw file content.

### Append-only ledger fit

`Ledger::append_object` (`crates/ledger/src/lib.rs`) identifies an object by its `KirId` (logical
identity) and versions it by `content_signature` (a hash of the payload with `created_at`
stripped). A `TransformNode`'s `KirObject` id must therefore be **deterministic and stable across
re-parses of the same logical source object**, so that a second `ekos recover` run over an
unchanged Pentaho step or SQL view produces the identical `KirId` + identical `content_signature`
(recognized as unchanged, no-op) rather than spuriously appearing as a new object every run. When
the underlying `.ktr`/SQL text genuinely changes, the same `KirId` with different content is
correctly recognized as a new version at that logical id, visible via `object_at`/`diff` as a
change over time — never an in-place mutation, matching `RFC 0026`'s `Concept` id scheme exactly:

```rust
fn transform_node_kir_id(origin: &TransformOrigin, node_index: usize) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL,
        format!("transform:{}:{}:node:{}", origin.source_kind, origin.source_path, node_index)
            .as_bytes()))
}
```
Scoped per `(source_path, node_index)`, not per node content — a node's position within its
source graph is the identity, its content is what versions. This mirrors `local_docs_analyzer.rs`'s
`section_kir_id` and RFC 0026's `concept_kir_id` schemes exactly, for the same reason: stable
identity across re-parses is what lets the ledger's existing diff/version machinery show "what
changed in this Pentaho job between last week and today" for free, with no new ledger mechanism.

### `Unmapped` as first-class evidence, not a parse failure

A parser encountering a Pentaho step type or SQL construct it does not recognize does not fail the
pass (matching `sql_analyzer.rs`'s existing degrade-never-fail contract for LLM calls, extended
here to structural parsing too) — it emits a `TransformNode::Unmapped { raw, reason }` node in the
graph, which lowers to a real `KirObject` with real `KirEvidence` citing the exact source location.
This is the mechanism that makes "Phase 2's stored-procedure MVP scope (control flow → Unmapped,
embedded SQL → real nodes)" and "Phase 3's Pentaho coverage-percentage metric (non-Unmapped nodes ÷
total nodes)" both well-defined: coverage is a ratio over real, queryable ledger facts, not a
number computed off to the side and thrown away.

## Alternatives Considered

- **A typed `Expr` AST shared across SQL/Pentaho/T-SQL/PL-pgSQL expressions**, as sketched in the
  implementation plan's draft `TransformNode`. Rejected for v1: reconciling four expression
  grammars into one typed tree is a large, separate project with no immediate consumer —
  `ekos_transformation_diff` (Phase 5) can do useful structural diffing over raw expression text
  (same join keys? same filter present/absent? same aggregate function?) without it. Kept as an
  Open Question / plausible follow-up RFC once real diffing usage shows text-level comparison is
  insufficient.
- **Per-node-type `ObjectKind` variants** (`ObjectKind::TransformSource`, `::TransformFilter`,
  etc.) instead of one `Custom("TransformNode")` kind with a `node_type` property. Rejected:
  inconsistent with the `Custom(...)` idiom RFC 0024/0026 already established for new semantic
  concepts, and would require touching `ObjectKind`'s definition for every new node type added
  later, when nothing currently exhaustively matches on `ObjectKind` (per its own doc comment) —
  no benefit to justify the churn.
- **A resolved `KirId` reference for `Source.object_name`/`Sink.object_name`** at parse time
  instead of a raw string. Rejected: resolving "which `Table` object does this Pentaho step's
  `TableInput` connection actually point at" requires cross-system identity resolution — exactly
  the interpretive, hypothesis-producing step this RFC's "observation vs. interpretation" section
  argues must stay out of the deterministic parsing layer, and which the implementation plan
  already schedules as its own later, harder phase (Phase 4) with its own RFC.
- **A new crate (`crates/transform-ir`)** instead of a module in `crates/semantic`. Rejected: no
  trait-based extension point exists for the IR types themselves (see "Crate placement"); the real
  extension points are the per-format parser passes, which already have a natural home
  (`crates/recovery`, `plugins/pentaho`) without a new crate for the shared data types they consume.

## Open Questions

- [x] Should `Filter.condition`/`Calculate.expr` graduate from raw text to a typed, shared `Expr`
  once Phase 2/3 parsers exist and real diffing needs surface concrete gaps in text-level
  comparison? **Resolved as deferred**: ship raw text now, revisit only once real
  `ekos_transformation_diff` usage shows a concrete gap — not blocking.
- [x] Exact shape of the future `TransformSemanticsAnalyzerPass` (LLM business-meaning enrichment
  layer analogous to RFC 0026). **Resolved as deferred**: explicitly out of scope for this RFC and
  for Phases 1–3, flagged as anticipated future work only — an agent can already read raw
  `Filter.condition`/`Calculate.expr` text and reason about business meaning itself without it.
- [x] Cross-system identity resolution over `Custom("TransformNode")` `Source`/`Sink` objects
  (matching a Pentaho step's `object_name` string to a SQL-parsed `Table` KirObject, an Informix
  schema-only object, etc.). **Resolved as deferred**: explicitly **not** designed here, per the
  implementation plan's own instruction that Phase 4 "may need its own follow-up RFC rather than
  being folded in here." This RFC only guarantees the `object_name` string is preserved verbatim
  and evidenced, giving Phase 4 a stable, well-formed input to resolve against later.
- [x] `JoinKind`/`AggExpr` field shapes are a reasonable first cut, not frozen. **Resolved as
  deferred**: Phase 2/3 implementers may find real Pentaho/SQL constructs (e.g. Pentaho's
  `MergeJoin` sort-merge semantics vs. `DatabaseJoin` semantics) that need small additions to these
  shapes — not a blocker for accepting the overall IR shape and lowering mechanism now.

## Acceptance Criteria

- [x] All Open Questions either resolved or explicitly deferred as documented future work (not
  silently dropped).
- [x] `TransformNode`/`TransformGraph`/`TransformOrigin` defined in
  `crates/semantic/src/transform_ir.rs` with `Serialize`/`Deserialize`.
- [x] `lower_to_kir(&TransformGraph) -> KirGraph` implemented per the mapping table above.
- [x] One deterministic-serialization test per `TransformNode` variant (TDD — written before the
  variant's lowering logic), asserting identical content-address output (via
  `ekos_artifact::ArtifactId::compute`, the same public entry point
  `compute_content_id` wraps) across repeated construction and differing output on any field
  change.
- [x] `transform_node_kir_id` gives stable ids across repeated lowering of an unchanged
  `TransformGraph`, and the resulting `KirObject`s round-trip through `Ledger::append_object`
  recognizing "no logical change" as a no-op (`transform_nodes_round_trip_through_ledger_versioning`,
  `crates/semantic/src/transform_ir.rs`) — required fixing evidence ids to be just as deterministic
  as object ids (`transform_evidence_kir_id`), since a `KirObject`'s `evidence: Vec<KirId>` is part
  of what `content_signature` hashes and `KirEvidence::new`'s default random id would otherwise make
  every re-lowering look like a content change.
- [x] No format-specific parser (SQL, Pentaho) implemented as part of this RFC — Phase 1 defines
  only the shared target representation, per the implementation plan's explicit scope boundary.
- [x] `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --check` clean
  for all new code; zero `unsafe` introduced.
- [ ] At least one review completed.
- [ ] Design is consistent with `ekos.md` compiler architecture (append-only ledger, evidence-backed
  facts, deterministic passes, Runtime read-only).

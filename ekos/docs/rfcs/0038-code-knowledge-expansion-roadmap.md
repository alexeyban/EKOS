# RFC 0038 — Code Knowledge Expansion Roadmap

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

EKOS's stated goal is to compile enterprise knowledge with real, evidence-backed accuracy — but
today's real code-parsing coverage is narrower than the vision: SQL DDL/DML, Pentaho `.ktr`/`.kjb`
XML, and a handful of document formats. Nothing exists for Python, PySpark, Jupyter notebooks,
Databricks, or Azure Data Factory — five of the most common real-world data-engineering surfaces.
This RFC is a roadmap, not a single feature: it sequences six phases of work so each can get its
own just-in-time RFC per `CLAUDE.md`'s mandatory workflow, rather than trying to design all six at
once.

Investigated directly against the current source before writing this roadmap:

- **Nothing exists today for Python, PySpark, Jupyter notebooks, Databricks, or Azure Data
  Factory.** Exhaustively grepped `crates/recovery/` and `plugins/`. The only near-misses: a
  `dependency_analyzer.rs` pattern-table literal (`"kafka-python"`), `sqlparser`'s built-in
  `DatabricksDialect` selection for parsing Databricks *SQL text* (not a workspace connector), and
  aspirational one-line mentions in `VISION.md` (Phase 4/7 product narrative). `TODO.md`'s only
  relevant line (1436) is an unspecified backlog bucket: "Databricks, dbt, etc."
- **Real analyzer inventory today** (`ekos/crates/recovery/`): `sql_analyzer.rs` (DDL → `Entity` +
  `ForeignKey`, LLM-enriched), `sql_transform_analyzer.rs` (SELECT/VIEW/proc bodies →
  Transformation IR; real `sqlparser` dialects for Postgres/MSSQL/Databricks-SQL-text/Informix),
  `pentaho_analyzer.rs` (`.ktr`/`.kjb` XML → Transformation IR), `dependency_analyzer.rs` (fixed
  5-technology substring table → `DependsOn`), `document_semantics_analyzer.rs` (LLM over
  already-extracted `Section` objects), `git_analyzer.rs`/`github_analyzer.rs`/
  `confluence_analyzer.rs`/`local_docs_analyzer.rs`/`crypto_analyzer.rs`. None parse Python or
  notebook source.
- **RFC 0031 (pluggable SQL dialects, Accepted) already built the extension point Phase 1 needs**:
  `SqlDialectParser` trait (`crates/sql-dialect-sdk/src/lib.rs:18-36` — `name()`,
  `sqlparser_dialect()`, `preprocess()` with an identity default), a compile-time registry, and two
  real dialect plugins (`mysql`, `postgres`). Explicitly out of scope in RFC 0031: deep
  procedural-body parsing (loops/cursors/`IF`) — still open today, and this roadmap's Phase 1.
- **`Observer` trait** (`observation-sdk/src/lib.rs:203-208`) is the connector contract every new
  source implements: `name()`, `async scan(&ScanContext) -> Result<ObservationPackage,
  ObserveError>` — read-only, idempotent.
- **Five plugins are scaffolded-only** (`salesforce`, `sap`, `oracle`, `fabric`, `snowflake`) —
  verified via each plugin's own header comment, all self-disclosing "never run against a live
  account," all following the "Phase 14 — scaffold, RFC 0012" pattern with a `Mock*Client` proving
  the mapping logic without live credentials. This is the pattern a scaffolded Databricks/ADF
  connector should follow if live credentials aren't available when that phase starts.
- **No parameter/variable concept exists anywhere in the KIR or Transformation IR** —
  `ObjectKind` (`kir/src/lib.rs:81-115`) and `RelationshipKind` (`:129-141`) have no
  `Parameter`/`ConfigTable`/`Variable` variant; `TransformNode` (`transform_ir.rs:418-457`) has no
  parameter-substitution construct. This is the real blocker for "metadata-driven pipelines" —
  not a missing connector, a missing piece of the shared IR vocabulary. Per `CLAUDE.md`'s
  "just-in-time" RFC rule, this should be designed against its clearest real consumer (Azure Data
  Factory, where pipelines are parameterized by design), not forward-declared speculatively now.
- **Databricks Jobs API job/task DAGs would be the first source of real `RelationshipKind::Calls`
  data anywhere in the project** — confirmed by grep that `Calls` is never constructed by any
  analyzer today, only present in test fixtures (RFC 0037 Phase 2 finding). A real Databricks
  connector recovering job-task dependencies closes the exact gap `SequenceDiagrams.md` (RFC 0037)
  had to honestly disclose as unfillable.

## Scope

Sequence and scope six phases of code-knowledge expansion. This RFC schedules them; it does not
design each phase's interfaces/data models in full — each phase gets its own RFC when it starts.

## Non-goals

- Not a full design for every phase — deliberately deferred to each phase's own just-in-time RFC.
- Not starting any phase's implementation in this pass.
- Not a promise of live-tested Databricks/ADF connectors — if no sandbox credential exists when
  those phases start, they follow the existing scaffolded-plugin pattern (real mapping logic,
  mock-client-tested, live wiring deferred), same honest disclosure as the five Phase-14 plugins.

## What already exists and is reused

- `SqlDialectParser` trait + registry (RFC 0031) — Phase 1's extension point, zero new
  architecture needed.
- `Observer` trait (`observation-sdk`) — the contract Phase 4/5's new connectors implement.
- The scaffolded-plugin pattern (`salesforce`/`sap`/`oracle`/`fabric`/`snowflake`, Phase 14/RFC
  0012) — the template for Databricks/ADF if live credentials aren't available.
- The Transformation IR (RFC 0027) and its diff/explain MCP tools (RFC 0028) — Phase 2's PySpark
  analyzer lowers into the *same* IR Pentaho/SQL already use, making cross-format diffing free.
- `local_docs_analyzer.rs`'s document-chunking pattern — Phase 3's notebook markdown-cell handling.

## Design — the six phases

**Phase 1 — Close existing SQL/Pentaho gaps.** Deepen `sql_transform_analyzer.rs` beyond
`SELECT`/`VIEW` into stored-procedure control flow (`IF`/`LOOP`/cursors as structural
`Unmapped`-with-reason nodes at minimum — RFC 0031 explicitly deferred this); add `snowflake` and
a real `databricks` SQL dialect plugin alongside `mysql`/`postgres` (same `SqlDialectParser`
trait). Verify `pentaho_analyzer.rs`'s still-unverified `DatabaseJoin` shape against a real sample
if one becomes available. No new connector, no new IR — pure depth, cheapest and most immediately
valuable phase.

**Phase 2 — Python/PySpark analyzer.** New `crates/recovery/src/python_analyzer.rs` (or a new
`plugins/python`, decided in this phase's own RFC). Real AST parsing (`rustpython-parser` or
`tree-sitter-python`, evaluated in the phase's own RFC — chosen over lightweight regex/heuristic
extraction per explicit decision below) extracts real imports (`DependsOn`), function/class defs
(a real upgrade over `plugins/file`'s existing substring-based `harvest_symbols`), and recognizes
PySpark DataFrame call chains (`.read.table(...)`, `.join(...)`, `.groupBy(...).agg(...)`,
`.filter(...)`, `.write.saveAsTable(...)`) lowered into the *same* Transformation IR Pentaho/SQL
use (`Source`/`Join`/`Aggregate`/`Filter`/`Sink`). This means `ekos_transformation_diff` (RFC
0028, already built) can diff a Pentaho job against a PySpark rewrite of it for free — proving or
disproving a migration preserved business logic, this project's stated reason for existing.

**Phase 3 — Jupyter notebooks (depends on Phase 2).** `.ipynb` is JSON with a `cells` array; code
cells get their source handed to Phase 2's analyzer per-cell, markdown cells become
`Custom("Section")` objects via `local_docs_analyzer.rs`'s existing chunking pattern. No new IR
concept needed.

**Phase 4 — Databricks connector (depends on Phase 2/3 for notebook recovery).** New
`plugins/databricks`, following the scaffold pattern the five Phase-14 plugins establish. Recovers
workspace notebooks (via Phase 3) and, the real prize, **Jobs API job/task DAGs** — the first real
`RelationshipKind::Calls` data anywhere in the project. Unity Catalog table lineage, if exposed
without extra entitlements, is a stretch goal, not a blocker.

**Phase 5 — Azure Data Factory connector.** New `plugins/azure-data-factory`, same scaffold
pattern. ADF pipelines are literally JSON (activities + dependency edges + parameters/global
parameters) — the natural home to design the parameter/variable IR concept for real, against
ADF's idiomatic metadata-driven pattern (`Lookup` over a control table → `ForEach` → parameterized
child pipeline/dataset). This phase's own RFC designs the actual data model (a
`TransformNode::Parameter{name, source}` variant, or a new `ObjectKind::Parameter` +
`RelationshipKind::Custom("ParameterizedBy")` edge — left open deliberately).

**Phase 6 — Generalize metadata-driven parameterization (depends on Phase 5).** Once ADF's real
parameter/variable IR construct exists and has a real consumer, retrofit the same vocabulary onto
Pentaho (metadata-driven Kettle jobs using "Copy/Get Rows from Result" + a control table) and
PySpark (job/widget parameters). Mirrors how the Transformation IR itself was only unified (RFC
0027) after two real consumers independently needed the same shape.

## Alternatives Considered

- **Design the parameter/variable IR concept now, up front** — rejected; `CLAUDE.md`'s
  "just-in-time" RFC rule and this project's own precedent both argue for designing it against
  ADF's real use case in Phase 5, not speculatively.
- **Lightweight regex-based Python parsing** — rejected per explicit decision; wouldn't reliably
  recover PySpark transformation chains into the Transformation IR, missing Phase 2's highest-value
  outcome (diffing Pentaho against PySpark rewrites).
- **Databricks/ADF connectors before Python/notebooks** — rejected per explicit decision to keep
  the dependency-ordered sequence (notebooks need Python; Databricks benefits from notebook
  recovery already existing; ADF's parameter design is cleaner to do once, not twice).

## Open Questions (each resolved by its own phase's future RFC)

- [ ] Python parser crate choice (`rustpython-parser` vs `tree-sitter-python`) — Phase 2's RFC.
- [ ] Whether Python source discovery reuses `plugins/file`'s existing generic walk or needs its
      own connector — Phase 2's RFC.
- [ ] Exact parameter/variable IR data model — Phase 5's RFC.
- [ ] Databricks/ADF live-credential availability at implementation time (determines scaffold-only
      vs. live-tested) — Phase 4/5's RFCs, decided when each phase starts.

## Testing

Every phase follows `CLAUDE.md`'s mandatory Tests-before-Implementation workflow, detailed in each
phase's own RFC. Phases 1-3: structural/deterministic parsing tests (same style as
`pentaho_analyzer.rs`'s real-XML-fixture tests). Phases 4-5: the existing scaffolded-plugin
pattern — real trait/mapping-logic unit tests against a `Mock*Client`, live integration deferred
until a real sandbox credential exists.

## Acceptance Criteria

- [x] Every phase's starting assumption verified against current source, not assumed.
- [x] Python parser depth and phase order decided before writing.
- [ ] At least one review completed.
- [ ] `TODO.md`'s Databricks/dbt backlog line updated to point at this RFC.

## Implementation Plan

See "Design — the six phases" above — this RFC's Implementation Plan *is* the phase sequence.
Each phase ships with its own RFC, own tests, own acceptance criteria before the next starts.

## Files Changed (this pass)

| File | Change |
|---|---|
| `ekos/docs/rfcs/0038-code-knowledge-expansion-roadmap.md` | new — this roadmap |
| `TODO.md` | backlog bucket line updated to reference RFC 0038's six phases |

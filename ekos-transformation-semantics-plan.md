# EKOS — Unified Transformation Semantics: Implementation Plan

## Context

Target scenario: a company has 1000+ GitHub repos, PostgreSQL, Databricks, Synapse
Pipelines, Synapse metadata stored in Postgres, an Informix DB (no source repo, schema
only), an outdated Confluence, and legacy Pentaho (Kettle) ETL jobs. A developer needs to
build a new pipeline that reproduces the business logic of an existing Pentaho job, but
with a new rule applied.

EKOS should let the developer ask an agent to explain what the legacy transformation does
(with evidence), see the impact of changing it, and get a drafted new pipeline — instead of
manually reading `.ktr`/`.kjb` XML and tribal knowledge in Confluence.

**Core idea:** a Pentaho step, a SQL `SELECT`, a `VIEW`, a stored procedure, and a function
are all the same underlying concept — a transformation of data from sources to a sink
through operations (filter, join, aggregate, calculate). Building a separate extraction
path per format would produce N incompatible semantic models that can't be diffed against
each other — which defeats the point (comparing old Pentaho logic to a new pipeline draft).
So everything maps into one intermediate representation first:

```
Pentaho .ktr/.kjb  ─┐
SQL SELECT          ─┤
CREATE VIEW          ├──▶  Transformation IR  ──▶  Object/Relationship/Evidence
Stored Procedure     │      (Source, Filter,
Function             ─┘      Join, Aggregate,
                              Calculate, Sink, Unmapped)
```

`Unmapped` is deliberate, not a placeholder: anything that can't be parsed/mapped must
still be recorded as a fact ("something is here, not yet understood") rather than silently
dropped — that's itself Evidence, and an explicit signal for where recovery or a human needs
to step in.

Governing constraint: per `CLAUDE.md`, **no feature is implemented before its RFC is
accepted**. Phase 0 below is therefore an RFC session, not a code session.

---

## Known bugs to fix alongside this plan

These are unrelated to the Transformation IR work above — pre-existing gaps surfaced while
building RFC 0025/0026 (document formats + LLM document-semantics extraction, see
`devlog_28.md`) — but tracked here so they don't get lost. Fix opportunistically, e.g. in the
same session as Phase 0 or Phase 1, not gating any phase above.

### Fix: `ekos ask` ignores `config.llm.provider = "ollama"`

**Problem:** `ekos recover` correctly honors `[llm] provider = "ollama"` (routes through
`build_llm_provider` in `crates/cli/src/commands/recover.rs`, added by RFC 0021). `ekos ask`
does not — `crates/cli/src/commands/ask.rs` hardcodes `AnthropicProvider` construction
directly and fails with `"No LLM provider configured. Set ANTHROPIC_API_KEY..."` even when
Ollama is configured and already working for `ekos recover` in the same workspace.

**Claude Code prompt:**

```
Fix ekos ask so it honors config.llm.provider the same way ekos recover already does.

crates/cli/src/commands/ask.rs currently constructs AnthropicProvider directly, ignoring
config.llm.provider entirely. crates/cli/src/commands/recover.rs's build_llm_provider already
implements the correct provider-selection logic (routes to OllamaProvider when
config.llm.provider == "ollama", otherwise falls through to the existing
Anthropic-or-Mock chain). Make ask.rs call the same build_llm_provider function instead of
constructing AnthropicProvider itself — do not duplicate the provider-selection logic a
second time.

Add a regression test proving ekos ask selects OllamaProvider when config.llm.provider =
"ollama" is set, mirroring the existing build_llm_provider selection tests in recover.rs.

Run cargo test --workspace, cargo clippy --workspace --all-targets, and cargo fmt --check
before considering this done.
```

---

## Phase order for Claude Code sessions

```
0 (RFC) → 3 (Pentaho plugin) → 1 (Transformation IR) → 2 (SQL analysis)
        → 5 (MCP tools) → 6 (agents) → 4 (identity resolution) → 7 (benchmark)
```

Rationale: Pentaho XML is deterministic and low-risk, so building it right after the RFC
gives a fast, real, testable result to validate the IR design against. SQL analysis (stored
procedures especially) is the largest and riskiest chunk, so it comes after the IR has
already proven itself on real Pentaho data. Identity resolution is the most architecturally
uncertain piece (it's the first place EKOS proposes hypotheses rather than just recording
facts), so it's deliberately last, right before the end-to-end benchmark.

---

## Phase 0 — RFC

**Goal:** get an accepted RFC before any implementation.

**Claude Code prompt:**

```
Read the existing RFCs in docs/rfcs/ and the mandatory workflow in CLAUDE.md.

Write a new RFC titled "Unified Transformation Semantics", following the exact same
format and numbering convention as the existing RFCs (continue from the current highest
RFC number).

Problem statement: EKOS currently has no unified way to represent transformation logic
that comes from heterogeneous sources — Pentaho (.ktr/.kjb) jobs, raw SQL (SELECT, VIEW),
and stored procedures/functions (T-SQL, PL/pgSQL). Without a shared representation, logic
recovered from one source cannot be compared or diffed against logic recovered from
another, which blocks the core use case: comparing legacy ETL logic to a newly drafted
pipeline.

Proposed solution: introduce a Transformation IR (intermediate representation) with a
fixed set of node types — Source, Filter, Join, Aggregate, Calculate, Sink, and an explicit
Unmapped node for anything that cannot be parsed or classified. Every format-specific
plugin (Pentaho parser, SQL parser, stored-procedure parser) compiles into this IR; the IR
is then what gets turned into Object/Relationship/Evidence in the ledger.

Explicitly address these constraints in the RFC and get them resolved before
implementation starts:
1. Where exactly is the boundary between "Observation Layer collects facts only" and
   "this is already interpretation"? Argue that deterministic parsing of SQL/XML into an
   AST-level IR is still a fact-collection step (the parse is deterministic and
   reproducible), while classifying *what a step means* for the business (e.g. "this
   FilterRows enforces business rule X") belongs to the recovery layer, not observation.
2. Confirm the IR nodes are content-addressable and serialize deterministically, per the
   existing artifact invariant.
3. Confirm this fits the append-only ledger model — an IR extracted from a later version
   of the same Pentaho job/SQL object becomes a new Event, not a mutation.
4. Flag that identity resolution across systems (Phase 4) produces *hypotheses*, not
   facts, and needs its own explicit trust/confidence status in the ledger — note that this
   may need its own follow-up RFC rather than being folded in here.

Do not write any implementation code in this session. Output only the RFC document.
```

---

## Phase 1 — Transformation IR

**Goal:** define the shared IR that every source format maps into.

**Design sketch to give Claude Code as a starting point:**

```rust
enum TransformNode {
    Source { object: ObjectRef, columns: Vec<ColumnRef> },
    Filter { condition: Expr },
    Join { left: NodeId, right: NodeId, keys: Vec<(ColumnRef, ColumnRef)>, kind: JoinKind },
    Aggregate { group_by: Vec<ColumnRef>, aggs: Vec<AggExpr> },
    Calculate { output: ColumnRef, expr: Expr },
    Sink { object: ObjectRef, columns: Vec<ColumnRef> },
    Unmapped { raw: String, reason: String },
}
```

**Claude Code prompt:**

```
Based on the accepted "Unified Transformation Semantics" RFC, implement the
Transformation IR as a new module (decide whether it belongs in the existing `semantic`
crate or as a new crate, and justify the choice against the current workspace layout).

Requirements:
- Define the TransformNode enum (Source, Filter, Join, Aggregate, Calculate, Sink,
  Unmapped) with fields sufficient to reconstruct a human-readable explanation of the
  transformation later.
- Every variant must serialize deterministically (same input always produces byte-identical
  output), consistent with the existing "every artifact is content-addressable" invariant.
- Write unit tests for deterministic serialization of every variant before writing the
  variants themselves (TDD) — one test per node type asserting identical checksums across
  repeated runs.
- Do not implement any format-specific parser yet (no SQL, no Pentaho). This phase only
  defines the shared target representation.
```

---

## Phase 2 — SQL Analysis (SELECT / VIEW / stored procedures / functions)

**Goal:** parse SQL objects across PostgreSQL, T-SQL (Synapse), Databricks SQL, and
Informix (best-effort) into the Transformation IR.

**Scope split — flag explicitly to Claude Code:**
- `SELECT` / `VIEW`: near-direct AST → IR mapping, deterministic, low risk.
- Stored procedures / functions: **not pure SQL** — they contain control flow (loops,
  cursors, conditionals, variables). Full procedural parsing (T-SQL, PL/pgSQL) is a large
  project on its own. MVP scope: extract embedded SQL statements inside the procedure body
  as individual Transformation IR fragments; represent the surrounding control flow as
  `Unmapped` with evidence "control flow present, not modeled" rather than attempting full
  branch semantics.
- Informix has no dedicated dialect in `sqlparser-rs` — plan to fall back to the generic
  dialect and accept incomplete coverage; do not invest in a custom Informix grammar in this
  phase.

**Claude Code prompt:**

```
Implement SQL analysis that compiles SELECT statements, VIEW definitions, and (with the
reduced scope below) stored procedures/functions into the Transformation IR defined in
Phase 1.

Use the `sqlparser-rs` crate. Support the Postgres and MSSQL (T-SQL) dialects fully; for
Databricks SQL, evaluate sqlparser-rs's coverage of Spark SQL extensions and document gaps;
for Informix, use the Generic dialect and document expected coverage gaps rather than
building a custom grammar.

For stored procedures and functions: extract embedded SQL statements as separate
Transformation IR fragments. Represent surrounding control flow (loops, cursors,
conditionals, variable assignment) as Unmapped nodes with reason "control flow present, not
modeled" — do not attempt to model branching logic in this phase.

Before writing the parser: create a golden-test suite of 10-15 real SQL examples (SELECT,
VIEW, at least 3 stored procedures/functions with embedded SQL) with hand-written expected
Transformation IR output. Write these tests first, then implement the parser against them.
If real anonymized examples aren't available, generate representative synthetic ones
covering: simple SELECT with WHERE, SELECT with JOIN, SELECT with GROUP BY, a VIEW wrapping
a multi-table query, and a stored procedure with an embedded SELECT plus a loop.

Report parser coverage as a percentage of parsed statements successfully mapped to
non-Unmapped IR nodes, per dialect.
```

---

## Phase 3 — Pentaho Plugin (.ktr / .kjb)

**Goal:** parse Pentaho Kettle XML transformations/jobs into the Transformation IR.

**Step-to-IR mapping to give Claude Code:**

| Pentaho step | Transformation IR |
|---|---|
| `TableInput` | `Source` |
| `FilterRows` | `Filter` |
| `Calculator` | `Calculate` |
| `DatabaseJoin` / `MergeJoin` | `Join` |
| `GroupBy` | `Aggregate` |
| `TableOutput` | `Sink` |
| anything else | `Unmapped` |

**Claude Code prompt:**

```
Implement a new observation-sdk plugin for Pentaho Kettle files (.ktr transformations and
.kjb jobs), following the same plugin structure as the existing/planned SQL Server and
PostgreSQL plugins.

Parse the XML and map known step types to the Transformation IR from Phase 1 using this
table: TableInput → Source, FilterRows → Filter, Calculator → Calculate,
DatabaseJoin/MergeJoin → Join, GroupBy → Aggregate, TableOutput → Sink. Map every other step
type to Unmapped with the raw step XML and step type name preserved as evidence.

Write tests against real .ktr/.kjb files if available; otherwise construct synthetic test
files covering each mapped step type plus at least one deliberately unrecognized step type
to verify Unmapped handling.

Report plugin coverage as: percentage of steps across the test files successfully mapped to
a non-Unmapped node. This is the phase's readiness metric — treat it as a concrete,
measurable exit criterion rather than a subjective "looks done".
```

---

## Phase 5 — MCP Tools on Top of Transformation IR

**Goal:** expose the IR through new MCP tools that agents can call.

**Claude Code prompt:**

```
Add two new MCP tools to the existing `ekos mcp serve` server, following the pattern of
the existing tools (ekos_search, ekos_neighborhood, ekos_impact, etc.):

1. `ekos_transformation_explain(object_id)` — walks the chain of Transformation IR nodes
   feeding into the given object and returns a human-readable explanation of what the
   transformation does, with each claim linked to its Evidence (source file, line/step,
   commit if applicable).

2. `ekos_transformation_diff(old_id, new_id)` — compares two Transformation IR graphs
   (e.g. an old Pentaho-derived one and a newly drafted pipeline) and reports differences in
   sources, filters, joins, aggregations, and calculations, in a format an agent can use to
   verify a migration preserves intended logic.

Follow the existing RFC process — write the RFC for these two tools before implementing,
per CLAUDE.md.
```

---

## Phase 6 — Agents / Skills

**Goal:** extend the existing four-agent demo pattern with roles needed for this workflow.

**Claude Code prompt:**

```
Following the existing agent pattern in demo/agents/ (estate-scout, impact-analyst,
memory-keeper, estate-architect — each embodying one capability, MCP-tool-only where
applicable), add two new agent definitions:

1. `legacy-logic-recoverer` (sonnet) — specializes in recovering business logic from
   legacy ETL formats (Pentaho, raw SQL, stored procedures) using the new
   ekos_transformation_explain tool and the underlying Transformation IR. Given a legacy
   object, it should describe what it does, cite evidence for each claim, and explicitly
   flag Unmapped portions it could not resolve.

2. `identity-reviewer` (haiku or sonnet — decide based on how much judgment the review
   task needs) — batches unconfirmed cross-system identity hypotheses (from Phase 4) and
   surfaces them for confirmation via ekos_identity_review, instead of requiring a human to
   review them one at a time.

Reuse impact-analyst and estate-architect as-is: impact-analyst assesses blast radius
before a legacy pipeline is replaced; estate-architect drafts the new pipeline using
ekos_transformation_explain and ekos_transformation_diff against the recovered legacy
logic.

Write each agent definition in the same .md format as the existing demo agents, and add a
new act to demo/DEMO.md walking through: recover Pentaho logic → check impact → draft new
pipeline with a modified rule → diff against the original.
```

---

## Phase 4 — Cross-System Identity Resolution

**Goal:** resolve the same real-world entity appearing under different names across
Informix, Postgres, and Databricks (e.g. `cust_mstr` ↔ `customers` ↔ `gold.dim_customer`).

**Note:** this is the one phase where EKOS moves from recording facts to proposing
hypotheses — treat it as architecturally distinct from the rest of the plan, likely
deserving its own RFC as flagged in Phase 0.

**Claude Code prompt:**

```
Following the RFC process, propose and (once accepted) implement cross-system identity
resolution:

1. A heuristic scorer that compares objects across different source systems (column name
   overlap, column type compatibility, naming pattern similarity) and produces a candidate
   match with a confidence score.

2. Store each candidate match as a Relationship in the ledger with an explicit status field
   (e.g. `unconfirmed`, `confirmed`, `rejected`) — never as a plain fact indistinguishable
   from directly observed relationships.

3. Add a new MCP tool `ekos_identity_review(relationship_id, decision)` that lets an agent
   or human confirm or reject a candidate match; confirmation writes a new Event to the
   ledger.

Write the RFC first, explicitly addressing how this differs from the "Observation Layer
facts only" invariant used everywhere else, and get it reviewed before implementing.
```

---

## Phase 7 — End-to-End Benchmark

**Goal:** validate the whole pipeline against the real target scenario, with a concrete
pass/fail criterion.

**Claude Code prompt:**

```
Set up an end-to-end benchmark scenario: a Pentaho job with a known transformation
(source tables, at least one filter, one join, one calculated field, one sink), and a
requirement to reproduce the same logic in a new pipeline with one modified rule.

Test criterion: give a developer access to Claude Code with the EKOS MCP server connected,
but no direct access to the Pentaho files or the original systems. Success = the developer
(via ekos_search, ekos_neighborhood, ekos_transformation_explain, ekos_transformation_diff,
and the legacy-logic-recoverer/estate-architect agents) can correctly describe the original
logic and produce a correct new pipeline draft, using only what EKOS surfaces — no manual
XML reading required.

Record: which questions were answered correctly from evidence, which required falling back
to Unmapped/uncertain data, and where the agent had to guess. Use this to decide whether
Phase 2 (SQL/stored procedure coverage) or Phase 4 (identity resolution) needs further work
before this is presentable as a real pack.
```

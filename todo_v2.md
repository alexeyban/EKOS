# TODO v2 — Technical Debt and Architecture Roadmap

**Project:** EKOS (Enterprise Knowledge Operating System)
**Status:** Post Phase 14 (partial) + Optimizer + near-real-data integration tests
**Last Updated:** 2026-07-11 (refreshed against current reality — originally written Post Phase 6)

---

# Executive Summary

This document was originally written as a Post-Phase-6 audit. The project has since progressed
through Phases 7–13 (Identity Resolution, Semantic Compiler, Knowledge Ledger, Runtime, AI Runtime,
EKL, Optimizer), a partial Phase 14 (SAP/Salesforce/Oracle/Fabric/Snowflake connectors scaffolded but
not live-verified against real sandboxes), a benchmark suite, and a near-real-data integration test
pass (`devlog_9` through `devlog_12`). **About a third of this document's original items are now
resolved** — each is marked below with what actually closed it out, rather than being silently
deleted, so the historical audit stays legible. Genuinely open items are unchanged in substance.

EKOS now has, beyond the original foundation list:

* Identity resolution (Phase 7) — with a real false-merge bug found and fixed this session
* A Semantic Compiler / CKM (Phase 8)
* An append-only SQLite-backed Ledger with working versioning and time-travel queries (Phase 9,
  fixed for real in Phase 13)
* A read-only Runtime + AI Runtime (Phases 10–11)
* A deterministic query language, EKL (Phase 12)
* Incremental builds, parallel pass execution, cache invalidation, ledger diff/branch (Phase 13,
  the Optimizer)
* Five proprietary-connector scaffolds, mock-tested but not live-verified (Phase 14, partial)
* A `criterion` benchmark suite and a real, near-real-open-source-data integration test harness

Several technical and architectural debts remain and should be addressed as the project moves
toward a genuine v1.0.

---

# 1. Technical Debt

---

## TD-001 — Recovery Layer Reads Files Directly

### Problem

`SqlAnalyzerPass` currently reads SQL files directly from disk instead of consuming immutable ObservationArtifacts.

This violates one of EKOS's core principles:

```text
Enterprise Reality
        ↓
Observation
        ↓
Artifacts
        ↓
Compiler Passes
```

Current implementation:

```text
ObservationArtifacts
        ↓
Recovery
        ↓
Filesystem (side-channel)
```

### Risks

* Non-deterministic builds
* Impossible remote/distributed execution
* Build reproducibility issues
* Harder caching
* Breaks content-addressable architecture

### Required Work

* Store file contents inside artifacts
* Introduce BlobArtifact or ContentArtifact
* Recovery passes must consume artifacts only

Priority: HIGH

---

## TD-002 — Git Observer Performance

### Problem

Current implementation executes multiple shell commands per commit.

Complexity:

```text
O(number_of_commits)
```

Large repositories may become unusable.

### Required Work

Investigate:

* gix crate
* libgit2
* batched parsing using:

```bash
git log --stat
```

Introduce incremental git indexing.

Priority: HIGH

---

## TD-003 — Recovery Crate Becoming Monolithic

Current crate responsibilities:

* SQL parsing
* Git analysis
* LLM providers
* Prompt logic
* Cache
* Semantic enrichment

### Risks

Large compile times.

Difficult ownership boundaries.

Potential circular dependencies.

### Required Refactoring

Split into:

```text
crates/

recovery-core/
recovery-sql/
recovery-git/
recovery-llm/
recovery-cache/
```

Priority: MEDIUM

---

## TD-004 — Missing Incremental Build Engine

**Status: RESOLVED — Phase 13 (Optimizer), see RFC 0011 and `devlog_9.md`.**

`PassManager::should_recompute` skips a pass when its `{version, config_hash, cache_inputs}` are
unchanged (`crates/compiler-core/src/cache.rs`); `ekos build` gates re-scanning per `observe.paths`
entry on a cheap mtime/size `source_fingerprint` (`crates/observation-sdk/src/lib.rs`). Verified with
real numbers: a 50-file warm rebuild ran in ~3.5% of the cold-build time. `PassManager::execution_levels`
also added DAG-level parallel pass execution (not originally asked for by this item, but the same
phase). What's *not* done: `ekos build` itself still isn't wired onto `Compiler`/`PassManager` (it's
still a hand-rolled loop) — only `ekos recover`'s passes get the parallel-execution benefit today.

<details><summary>Original problem statement</summary>

Current compiler rebuilds almost everything.

Artifacts are already content-addressable but scheduler does not fully exploit this.

### Required Work

Implement:

```text
Artifact DAG

Artifact A changed
      ↓
Only dependent nodes rebuild
```

Features:

* dependency graph
* cache invalidation
* incremental execution
* build fingerprints

Priority: HIGH

</details>

---

## TD-005 — Missing Distributed Execution Model

Current architecture assumes local execution.

Future enterprise installations will require:

* distributed scanning
* distributed recovery
* remote artifact stores
* horizontal scaling

### Required Work

Design:

```text
Scheduler
      ↓
Task Queue
      ↓
Workers
```

Potential technologies:

* NATS
* Kafka
* Temporal
* Tokio distributed executors

Priority: LOW

---

# 2. Architecture Debt

---

## AD-001 — KIR Is Too Generic

**Status: IN PROGRESS — this session.** `ObjectKind` expanded from 8 variants
(`File`/`Directory`/`Table`/`Entity`/`Service`/`Api`/`BusinessRule`/`Unknown` + `Custom(String)`) to
include `BusinessConcept`, `Dataset`, `Column`, `Pipeline`, `Dashboard`, `Person`, `Model`, `Prompt`,
`Agent` (`crates/kir/src/lib.rs`). A full codebase survey confirmed this was a low-risk additive
change — no exhaustive `match` exists anywhere, EKL's `WHERE kind = '...'` and CLI display work
automatically via the existing `Display` impl, and no golden JSON fixtures depend on the enum's
shape. One real construction site was migrated as proof: git contributors were reclassified from
`ObjectKind::Entity` + a `role` property to `ObjectKind::Person` (`crates/recovery/src/git_analyzer.rs`).
**What's left:** most of the new variants (`Dataset`, `Column`, `Pipeline`, `Dashboard`, `Model`,
`Prompt`, `Agent`) have no construction site yet — they exist in the type but nothing emits them,
same as `Directory`/`Service`/`Api`/`BusinessRule` before this session. Wiring them up is properly
scoped to whichever future connector/pass actually needs to classify that kind of thing (e.g. a dbt
connector would emit `Pipeline`/`Dataset`, a dashboard connector would emit `Dashboard`) — adding a
variant nobody constructs yet doesn't reduce ambiguity by itself, it just removes friction for the
day something does. `RelationshipKind`'s taxonomy (also flagged as "potentially" needing expansion)
is untouched — not evaluated this session.

<details><summary>Original problem statement</summary>

Current primitives:

* Object
* Relationship
* Event
* Evidence

This is elegant but eventually everything becomes:

```text
Object
```

leading to semantic ambiguity.

### Required Work

Introduce:

```rust
enum ObjectKind {
    Dataset,
    Table,
    Column,
    Service,
    Pipeline,
    Dashboard,
    Person,
    BusinessConcept,
    Api,
    Model,
    Prompt,
    Agent,
}
```

Potentially:

```rust
enum RelationshipKind
```

Priority: VERY HIGH

</details>

---

## AD-002 — Missing Canonical Schema Layer

**Status: PARTIALLY RESOLVED — Phase 8, `crates/semantic/src/lib.rs`.** `CkModel`/`CkmObject`/
`CkmRelationship`/`EvidenceRecord` exist as the explicit CKM contract, with a `CkModel::validate()`
that catches dangling relationships and other structural violations (unit-tested). What's still
missing from the original ask: the CKM's object/relationship *kinds* are just `ObjectKind`/
`RelationshipKind` passed through from KIR (see AD-001) — there's no separate, more business-oriented
vocabulary (`Owner`, `Business Term`, `Process`, `Policy`) at the CKM layer distinct from KIR's. If
that distinction still matters, it's a follow-up on top of AD-001, not a restart of this item.

<details><summary>Original problem statement</summary>

KIR currently mixes:

* structural metadata
* business metadata
* semantic metadata

Need explicit CKM contracts.

### Required Work

Define:

```text
Canonical Knowledge Model (CKM)

Dataset
Column
Owner
Business Term
Service
Process
Policy
```

with formal invariants.

Priority: VERY HIGH

</details>

---

## AD-003 — Identity Resolution Does Not Exist Yet

**Status: RESOLVED — Phase 7, `crates/identity/src/lib.rs` (RFC 0007), `DefaultResolver`.** This
was correctly flagged as the hardest component, and that held up: this session's near-real-data
integration tests (`devlog_12.md`) caught a real false-merge bug in the shipped resolver — it was
collapsing distinct tables that merely share a name prefix (`orders`/`order_items`,
`Employees`/`EmployeeTerritories`) because its "structural similarity" term was a constant, not real
signal. Fixed by scoring real column-name overlap when available. The lesson generalizes: identity
resolution isn't a "build once" component, it's an "ongoing tuning against real data" one — treat
future false-merge/false-split reports as expected maintenance, not surprising bugs. Still missing
from the original ask: embeddings-based similarity and human-approval workflows (see MC-003) —
current matching is lexical (Jaro-Winkler) + structural (column overlap) only.

<details><summary>Original problem statement</summary>

This is likely the hardest component of EKOS.

Examples:

```text
customer
customers
customer_dim
crm_customer
client
```

↓

```text
same business entity
```

### Required Work

Introduce:

```text
Identity Layer
```

Capabilities:

* lexical similarity
* embeddings
* graph similarity
* evidence scoring
* confidence levels
* human approval workflows

Priority: CRITICAL

</details>

---

## AD-004 — Runtime Model Undefined

**Status: RESOLVED — Phases 10–11, `crates/runtime/src/{lib.rs,ai.rs}` (RFC 0005, RFC 0009).**
`Runtime` answers current-state reconstruction (`load_object`, `reconstruct_state`), historical
reconstruction (`object_at`, `reconstruct_state_at` — made genuinely correct by Phase 13's ledger
versioning fix, not just present), and graph projections (`load_neighborhood`). `AiRuntime` is the
AI-serving API: retrieve → expand → ground → ask → parse, with evidence citations. Indexes: FTS5 for
name search, no dedicated graph index beyond SQLite's own indexes on the current-state tables.

<details><summary>Original problem statement</summary>

Runtime currently exists only conceptually.

Questions remain unanswered:

* query execution model
* projections
* temporal reconstruction
* snapshot materialization
* indexes

### Required Work

RFC:

```text
Runtime Architecture
```

Define:

```text
Current state reconstruction
Historical reconstruction
Context projections
AI serving APIs
```

Priority: HIGH

</details>

---

## AD-005 — Ledger Storage Undefined

**Status: RESOLVED (as a documented interim decision) — Phase 9, `crates/ledger/src/lib.rs`
(RFC 0004).** SQLite was chosen explicitly as a disposable v0.x backend behind a storage-agnostic
API — RFC 0004 documents up front what SQLite does *not* solve (concurrent writers, unbounded
append-only growth/compaction, branch-by-file-copy) so the future backend swap is planned, not a
rescue. Temporal queries: real (`object_at`, fixed in Phase 13). Lineage/graph traversal: via
`Runtime::load_neighborhood`. Snapshots/branching: `ekos branch` (Phase 13) copies the ledger file
via `VACUUM INTO`. **Genuinely still open:** none of PostgreSQL/RocksDB/Delta Lake/EventStoreDB/
FoundationDB has been evaluated — that evaluation is deferred until SQLite's documented limits are
actually hit, per RFC 0004's own framing, not because it was overlooked.

<details><summary>Original problem statement</summary>

Append-only ledger exists conceptually only.

Need decisions:

### Storage candidates

1. PostgreSQL
2. RocksDB
3. Delta Lake
4. Event Store DB
5. FoundationDB

### Questions

* temporal queries
* lineage traversals
* graph queries
* snapshots
* compaction

Priority: HIGH

</details>

---

# 3. Missing Capabilities

---

## MC-001 — Knowledge Diff Engine

**Status: RESOLVED (v0 scope) — Phase 13, `ekos diff` / `diff_ledger` (RFC 0011).** Answers "what
changed" between two points in time as a list of new object/relationship *versions* written in that
window, plus an unchanged count. Building this required first fixing a real gap: the ledger's append
was previously idempotent purely on object id, silently dropping content updates — `diff_ledger`
couldn't exist without that fix, so it's part of the same RFC. **Not done:** "why did it change"
(causal/semantic diff beyond "this version replaced that one"), lineage diff, and documentation-drift
detection are all still open — this is a raw version-diff, not the fuller semantic/lineage diff the
original item envisioned.

<details><summary>Original problem statement</summary>

Potential killer feature.

Examples:

```text
What changed?

Why did it change?

What knowledge disappeared?
```

Required features:

* graph diff
* semantic diff
* lineage diff
* documentation drift detection

Priority: HIGH

</details>

---

## MC-002 — Semantic Query Language

**Status: RESOLVED (v0 scope) — Phase 12, EKL (`crates/ekl/`, RFC 0010).** A custom DSL:
`FIND <Object|Relationship> WHERE ... FROM ... RETURN ... ORDER BY ... LIMIT ...`, compiling directly
to `Runtime` calls, no LLM in the loop. Explicitly narrower than the original examples — no
multi-hop path expressions (`EXPLAIN WHY TABLE A DEPENDS ON TABLE B` needs graph-path reasoning EKL
v0 doesn't have), no natural-language-shaped queries (that's `ekos ask`'s job, Phase 11, a different
mechanism entirely — LLM-grounded, not deterministic). RFC 0010 documents this narrowing explicitly
as a deliberate v0 scope decision, not an oversight.

<details><summary>Original problem statement</summary>

Need a query layer.

Examples:

```sql
SHOW DATASETS OWNED BY TEAM X

EXPLAIN WHY TABLE A DEPENDS ON TABLE B

SHOW BUSINESS RULES IMPACTED BY CHANGE Y
```

Possible approaches:

* SQL dialect
* GraphQL
* Cypher
* custom DSL

Priority: HIGH

</details>

---

## MC-003 — Human Feedback Loop

Pure AI extraction will generate errors.

Need:

```text
Human corrections
        ↓
Evidence
        ↓
Compiler feedback
```

Priority: HIGH

---

## MC-004 — Confidence Scores

**Status: PARTIALLY RESOLVED.** Per-evidence confidence exists and is used today:
`KirEvidence.confidence: f32` (`crates/kir/src/lib.rs`), propagated into
`EvidenceRecord`/`CkmObject` in the CKM (Phase 8), sorted descending when a CKM object embeds its
evidence. **Still missing:** an *aggregate* confidence score per fact/object combining source
reliability, evidence count, LLM certainty, and graph consistency into one number — today each piece
of evidence carries its own confidence, but nothing rolls them up.

<details><summary>Original problem statement</summary>

Every recovered fact should have:

```text
confidence: f64
```

Based on:

* source reliability
* number of evidences
* LLM certainty
* graph consistency

Priority: HIGH

</details>

---

## MC-005 — Temporal Knowledge Reconstruction

**Status: RESOLVED (for objects) — `Runtime::object_at`/`reconstruct_state_at`
(`crates/runtime/src/lib.rs`), correct as of Phase 13's ledger versioning fix.** Before that fix,
`object_at` only worked for never-updated objects (it filtered the *current* pointer's timestamp
rather than querying true version history) — a real gap that's now closed for objects specifically.
`relationships_at` still only reflects a relationship's *current* version filtered by timestamp, not
true multi-version history — documented as a known limitation in RFC 0011, not silently absent.

<details><summary>Original problem statement</summary>

One of EKOS's biggest opportunities.

Examples:

```text
Show enterprise state at:

2025-06-01
```

Potentially huge differentiator.

Priority: MEDIUM

</details>

---

# 4. AI Debt

---

## AI-001 — Single LLM Provider

Need provider abstraction beyond Anthropic.

Add:

* Azure OpenAI
* OpenAI
* Ollama
* Gemini
* local models

Priority: MEDIUM

---

## AI-002 — Prompt Management

Need:

```text
Prompt Registry
Prompt Versioning
Prompt Testing
Prompt Evaluation
```

Priority: HIGH

---

## AI-003 — Evaluation Framework

Need reproducible quality measurements.

Metrics:

* extraction precision
* recall
* semantic accuracy
* hallucination rate

Priority: HIGH

---

# 5. Production Readiness Debt

---

## PROD-001 — No Security Model

Need:

* RBAC
* tenant isolation
* secrets management
* plugin sandboxing

Priority: HIGH

---

## PROD-002 — No Observability

Need:

* tracing
* metrics
* compiler diagnostics
* build telemetry

Priority: HIGH

---

## PROD-003 — No Plugin Isolation

Connectors may become unsafe.

Need:

* process isolation
* WASM plugins
* sandbox execution

Priority: MEDIUM

---

# 6. Suggested Near-Term Roadmap

**Status as of 2026-07-11:** most of v0.3–v0.5 below is done — identity resolution, semantic query
engine, CKM, ledger, runtime, temporal reconstruction, and knowledge diff all shipped (see the
RESOLVED items above). Artifact purity (TD-001) and typed-KIR-taxonomy-completion (AD-001, in
progress) are the two v0.3 items still genuinely open. `Cargo.toml` still declares `0.1.0` — there's
been no formal version bump tracking phase completion, so treat "v0.3/v0.4/v0.5" here as a rough
sequencing label from the original audit, not a real release number.

---

## v0.3

* fix artifact purity — **open, see TD-001**
* introduce typed KIR — **in progress, see AD-001**
* implement knowledge diff — **done, see MC-001**
* implement confidence scores — **partially done, see MC-004**

---

## v0.4

* identity resolution — **done, see AD-003** (ongoing tuning, not one-and-done — see AD-003's note)
* semantic query engine — **done (v0 scope), see MC-002**
* CKM specification — **done, see AD-002**

---

## v0.5

* ledger implementation — **done, see AD-005**
* runtime implementation — **done, see AD-004**
* temporal reconstruction — **done for objects, see MC-005**

---

## v0.6+ — the honest frontier

* distributed execution — **not started (TD-005)**
* enterprise connectors — **partially scaffolded, not live-verified** (Phase 14: SAP/Salesforce/
  Oracle/Fabric/Snowflake mock-tested only; Kubernetes not attempted; Postgres/SQL Server connectors
  from the original Phase 4 plan were never built as live-DB connectors either — SQL analysis has
  only ever worked over static DDL text, not a live connection)
* AI agents — **not started**
* autonomous knowledge maintenance — **not started**

---

# Biggest Risks

*(Updated 2026-07-11: risks 2 and 5 have shifted from "unstarted" to "shipped but needs ongoing
attention" — see notes below. 1, 3, 4, 6 are unchanged.)*

1. KIR becoming too generic. *(AD-001 in progress this session — narrowing, not resolved.)*
2. Identity resolution complexity. *(Shipped in Phase 7 — but this session's false-merge bug proves
   the risk was real and hasn't gone away just because code exists; treat it as an ongoing
   tuning/quality risk, not a solved problem.)*
3. Scope explosion.
4. Recovery crate monolith. *(Still true — see TD-003, untouched.)*
5. Runtime/ledger complexity. *(Shipped in Phases 9–11, and Phase 13 found and fixed a real
   correctness gap in the ledger's versioning — same lesson as #2: shipped isn't the same as fully
   correct, keep testing against real data.)*
6. Trying to solve too many enterprise problems simultaneously. *(Phase 14's connector scaffolding
   without live verification is a live instance of this risk — five connectors exist in code but
   none has been run against a real vendor sandbox.)*

---

# Recommendation

Focus MVP around:

```text
Git
+
SQL
+
Documentation
        ↓
Knowledge Graph
        ↓
Query Engine
        ↓
AI Consumption
```

Delay:

* autonomous agents
* distributed execution
* advanced runtime projections
* enterprise orchestration

until the semantic foundation becomes stable.

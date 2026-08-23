# RFC 0065 — EKOS Architecture Knowledge Model & Iterative Intelligence Loop

**Status:** Proposed
**Author:** External contribution, filed into this repo's RFC sequence 2026-08-21
**Created:** date unknown (source document undated); filed 2026-08-21
**Supersedes:** [RFC 0064](0064-architecture-knowledge-model.md)

**Original identifier:** EKOS-ARCHMODEL-002 (external doc's own numbering, not this repo's RFC
sequence — kept below for provenance)
**Original Parent RFC referenced:** EKOS-ARCHDOC-001 — "Architecture Documentation Compiler". This
parent document was not provided alongside this file and does not exist elsewhere in this
repository as of filing — an unresolved reference, kept as-is from the source rather than invented.
**Target:** EKOS Architecture Plugin / Knowledge Compiler
**Audience:** Software Architects, Data Architects, Technical Leads, EKOS maintainers

---

## Phase 1 implemented (2026-08-22)

`Status` above stays `Proposed` — this RFC as a whole (its own §60 "Plugin Architecture" parallel
subsystem, the reasoning layer, the evaluator/feedback loop) is not accepted or built. A narrow,
explicitly-scoped first slice is, decided via two up-front questions to the user before any code:
(1) integrate with EKOS's existing KIR/compiler pipeline rather than build this RFC's literal
parallel Evidence/Entity/Relationship/Claim type system and storage (`ObjectKind::Custom(_)`
already covers the same four primitives CLAUDE.md states as canonical — ObjectKind/Relationship/
Event/Evidence), and (2) ship only "knowledge model + one deterministic extractor + a C4 view" —
no LLM reasoning, no evaluator, no feedback loop, no RFC 0066 agent state machine.

**What shipped:**

- Two new `ObjectKind::Custom` kinds: `"Claim"` (§12-13, Fact type only — "X depends_on Y" derived
  deterministically from `CrateTopologyAnalyzerPass`'s (RFC 0042) already-real `DependsOn` edges)
  and `"ArchitectureGap"` (§17's "Unknown" concept — named differently to avoid colliding with the
  pre-existing built-in `ObjectKind::Unknown` fallback variant's identical `Display` text). Both
  added to `DefaultResolver`'s blanket kind-exclusion list proactively (`crates/identity/src/lib.rs`)
  — the same over-merge failure shape (shared name prefix + same-kind structural-score fallback of
  1.0) already hit by `Section`/`TransformNode`/`RustSymbol`/.../`Crate`.
- `crates/recovery/src/crate_topology_analyzer.rs`: emits one `Claim` per `DependsOn` edge it
  already derives; a previously *silently dropped* case (`DepResolution::Unresolved` — a
  `{ workspace = true }` entry with no matching `[workspace.dependencies]` key, or an
  unmodeled-in-v1 dependency shape) now becomes a real, evidence-backed `ArchitectureGap` instead
  of a no-op — matching this project's own "Unmapped is deliberate, not a gap swept under the rug"
  philosophy (Transformation IR, RFC 0027).
- `crates/docs-gen/src/lib.rs`'s `render_architecture`: the existing "## Crate & Workspace
  Topology"/"## Technologies" sections (which already rendered almost exactly what a from-scratch
  "C4 Container view" would have — found during implementation, avoided duplicating it) gained a
  one-paragraph C4 mapping note (§23: crate → Container, technology → External System); a genuinely
  new "## Open Questions" section lists `ArchitectureGap` objects, unless resolved (§17).

**Deferred, real follow-on RFCs, not started:** the reasoning layer (§14-15, local/cloud LLM),
`Inference`/`Assumption`/`Recommendation`-type claims (require that reasoning layer), the
evaluator/feedback/re-collection loop (§32-39), RFC 0066's agent state machine in full, Phase 2's
Terraform/Kubernetes/OpenAPI/SQL extractors (§68).

See `devlogs/devlog_70.md` for the full writeup, including why the originally-planned standalone
C4 diagram function was dropped mid-implementation in favor of annotating what already existed.

## Phase 2/3 implemented (2026-08-22) — reasoning + evaluation, RFC 0067

Closes two of Phase 1's deferred items: the reasoning layer (§14-15, §41-49) and the evaluator +
targeted re-collection (§32-39), scoped to the real MVP both this RFC (§67) and RFC 0066 (§64-65)
already define, not their full combined 146-section scope. Full design rationale in RFC 0067; the
short version: `ArchitectureReasoningPass` is modeled directly on the existing
`DocumentSemanticsAnalyzerPass` (RFC 0026) — real LLM classification of each `Crate`'s
architectural role, written as `inference`-type `Claim`s; `evaluate_architecture` is a plain
deterministic function scoring only the two dimensions this phase has real signal for
(`completeness`, `evidence_coverage`); targeted re-collection reads a crate's own leading `//!` doc
comment for any crate the evaluator flagged unclassified. RFC 0066's MVP investigation loop
(`ekos architecture investigate`) orchestrates all of it. Still deferred: `Assumption`/
`Contradiction`-type claims, Phase 2/3 extractors, everything RFC 0066 itself scopes past its own
MVP (checkpointing, concurrency, CI/CD mode). See `devlogs/devlog_71.md`.

---

# 1. Summary

This RFC defines the **Architecture Knowledge Model (AKM)** and the iterative intelligence pipeline that EKOS uses to reconstruct, reason about, evaluate, and document the architecture of an existing software system.

The core principle is:

> **Architecture documentation is a compiled view of an evidence-backed Architecture Knowledge Model.**

EKOS must not behave as a one-shot documentation generator:

```text
Repository → LLM → documentation
```

Instead, it operates as an iterative architecture investigation system:

```text
COLLECT
   ↓
ANALYZE
   ↓
REASON
   ↓
MODEL
   ↓
GENERATE
   ↓
EVALUATE
   ↓
FEEDBACK
   ↓
TARGETED COLLECTION
   ↺
```

The process repeats until the architecture reaches a configurable quality threshold or the system reaches its investigation limits.

The Architecture Knowledge Model is therefore not only a storage format. It is the **persistent architectural memory of the investigation**.

---

# 2. Motivation

Legacy systems frequently have several conflicting sources of architectural information:

```text
Source code
Infrastructure
Database schemas
Configuration
CI/CD
API specifications
Runtime behavior
Old documentation
Human knowledge
```

These sources may disagree.

For example:

```text
Architecture document:
    Oracle

application configuration:
    PostgreSQL

Terraform:
    PostgreSQL

source code:
    PostgreSQL driver
```

A conventional documentation generator may simply choose one answer.

EKOS must instead represent:

```text
Observed facts
    ↓
Evidence
    ↓
Contradiction
    ↓
Reasoning
    ↓
Resolution
```

The goal is not to produce plausible documentation.

The goal is to produce:

> **The most accurate architecture model that can be reconstructed from available evidence, with uncertainty and provenance explicitly represented.**

---

# 3. Core Architecture

The complete system consists of the following layers:

```text
┌──────────────────────────────────────────────────────────────┐
│                  EKOS ARCHITECT AGENT                        │
│                                                              │
│  ┌──────────────┐                                            │
│  │  COLLECTION  │ ←──────────────────────────────┐           │
│  └──────┬───────┘                               │           │
│         ↓                                       │           │
│  ┌──────────────┐                               │           │
│  │   ANALYSIS   │                               │           │
│  └──────┬───────┘                               │           │
│         ↓                                       │           │
│  ┌──────────────┐                               │           │
│  │   REASONING  │                               │           │
│  └──────┬───────┘                               │           │
│         ↓                                       │           │
│  ┌──────────────┐                               │           │
│  │ KNOWLEDGE    │                               │           │
│  │    MODEL     │                               │           │
│  └──────┬───────┘                               │           │
│         ↓                                       │           │
│  ┌──────────────┐                               │           │
│  │  GENERATION  │                               │           │
│  └──────┬───────┘                               │           │
│         ↓                                       │           │
│  ┌──────────────┐                               │           │
│  │  EVALUATION  │                               │           │
│  └──────┬───────┘                               │           │
│         ↓                                       │           │
│  ┌──────────────┐                               │           │
│  │   FEEDBACK   │ ──────────────────────────────┘           │
│  └──────────────┘                                            │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

The feedback loop is a first-class part of the architecture.

---

# 4. Design Principles

## 4.1 Evidence First

Every important architectural claim should have evidence.

If evidence does not exist:

```text
UNKNOWN
```

must be preferred over an unsupported assertion.

---

## 4.2 Documentation Is a Compilation Target

Documentation is not the canonical representation.

The canonical representation is:

```text
Architecture Knowledge Model
```

Documents are projections:

```text
AKM
 ├── C4
 ├── arc42
 ├── deployment architecture
 ├── data architecture
 ├── security architecture
 ├── ADRs
 └── custom views
```

---

## 4.3 Facts, Inferences, Assumptions and Recommendations Are Different

EKOS must distinguish:

```text
FACT
INFERENCE
ASSUMPTION
UNKNOWN
RECOMMENDATION
```

Example:

```yaml
fact:
  OrderService writes to PostgreSQL
```

versus:

```yaml
inference:
  PostgreSQL is the system of record for orders
```

versus:

```yaml
recommendation:
  isolate the shared database
```

These must never be silently merged.

---

# 4.4 Local LLM First

Where an LLM is required, EKOS should prefer a **local LLM by default** whenever the local model can perform the task adequately.

This is especially important because architecture repositories may contain:

- proprietary source code;
- internal APIs;
- infrastructure definitions;
- database schemas;
- security configuration;
- business logic;
- credentials or sensitive metadata.

The default execution model should therefore be:

```text
Source
   ↓
Deterministic extraction
   ↓
Local processing
   ↓
Local LLM reasoning
   ↓
Local Architecture Knowledge Model
```

Cloud LLM use must be explicit.

Supported modes:

```text
local
cloud
hybrid
none
```

---

# 4.5 Deterministic Analysis Before LLM Reasoning

EKOS should never use an LLM to discover information that can be reliably extracted deterministically.

Prefer:

```text
AST
dependency parsers
SQL parsers
Terraform parsers
Kubernetes parsers
OpenAPI parsers
Git history
configuration parsers
```

before:

```text
LLM inference
```

LLMs should primarily handle:

- semantic interpretation;
- classification;
- relationship inference;
- ambiguity resolution;
- summarization;
- architecture reasoning;
- evaluation;
- investigation planning.

---

# 4.6 No Unsupported Precision

EKOS must not invent values such as:

```text
99.99% availability
10,000 requests/sec
RPO = 15 minutes
```

unless evidence supports those claims.

---

# 4.7 Human Review Is Persistent Knowledge

Human corrections must survive regeneration.

Example:

```yaml
review:
  status: confirmed
  reviewer: architect@example.com
  comment: "Confirmed with production team."
```

Human feedback is part of the Architecture Knowledge Model.

---

# 5. Architecture Intelligence Loop

The primary workflow is:

```text
1. COLLECT
2. ANALYZE
3. REASON
4. UPDATE KNOWLEDGE MODEL
5. GENERATE
6. READ & EVALUATE
7. IDENTIFY GAPS
8. CREATE INVESTIGATION PLAN
9. COLLECT TARGETED EVIDENCE
10. REASON AGAIN
11. REGENERATE
12. RE-EVALUATE
```

The loop ends when:

```text
quality threshold reached
```

or:

```text
no meaningful new evidence can be collected
```

or:

```text
maximum iterations reached
```

---

# 6. Collection Layer

The Collection Layer gathers raw evidence.

Potential sources:

```text
Git repository
source code
configuration
dependency files
SQL
database schemas
Terraform
Kubernetes
Docker
Helm
OpenAPI
CI/CD
tests
Git history
existing documentation
logs
metrics
traces
runtime observations
human input
```

The collection process should be incremental.

---

# 7. Collection Must Be Targetable

The system must support both:

```text
initial broad collection
```

and:

```text
targeted collection
```

Example:

```text
Initial analysis:
"PaymentService uses Kafka."

Evaluation:
"Evidence is insufficient."

Investigation plan:
"Inspect PaymentService Kafka consumers."

Targeted collection:
src/payment/kafka/
application.yml
deployment/payment.yaml

New evidence:
PaymentService consumes orders.created
```

This prevents repeatedly scanning the entire repository.

---

# 8. Evidence Model

Evidence is a first-class object.

```yaml
evidence:
  id: evd.source.001

  type: source_code

  source:
    repository: github.com/example/project
    commit: abc123
    path: src/orders/service.py
    line_start: 42
    line_end: 81

  extraction:
    method: static_analysis
    extractor: python.ast

  observed_at:
    timestamp: 2026-08-21T12:00:00Z

  reliability:
    level: high
    score: 0.98
```

Evidence types:

```text
source_code
ast
dependency_file
configuration
environment_configuration
database_schema
sql
terraform
kubernetes
docker
helm
api_specification
ci_cd
git_history
test
log
metric
trace
runtime_observation
existing_documentation
ticket
human_input
manual_annotation
external_reference
```

---

# 9. Evidence Reliability

Evidence sources have different default reliability.

Suggested defaults:

```text
runtime observation        very high
database schema            very high
deployment configuration   very high
source code                high
API specification          high
infrastructure code        high
tests                      medium/high
recent documentation       medium
old documentation          low/medium
LLM inference              low unless supported
```

These are configurable policies, not absolute rules.

---

# 10. Architecture Entity Model

Every architectural entity requires a stable identifier.

```yaml
entity:
  id: service.order
  type: service
  name: Order Service

  description:
    text: "Handles order creation and lifecycle."

  state:
    lifecycle: current
    environment: production

  ownership:
    team: orders

  technology:
    name: Java
    version: "21"

  classification:
    criticality: high

  evidence:
    - evd.source.001

  confidence:
    level: high
    score: 0.95

  review:
    status: unreviewed
```

Recommended entity types:

```text
organization
business_capability
business_process

system
subsystem
application
service
module
component
library

api
interface
endpoint

database
database_schema
table
view
data_store
data_entity
file_store
object_store

queue
topic
event
message

external_system
external_service

actor
user

cloud_account
cloud_resource
network
subnet
cluster
node
container
pod
deployment_unit

environment
technology
framework
runtime

requirement
quality_attribute

architecture_decision
risk
technical_debt
constraint
assumption
question
```

---

# 11. Relationship Model

Relationships are first-class architectural objects.

```yaml
relationship:
  id: rel.order-service.postgres
  type: writes_to

  source:
    entity: service.order

  target:
    entity: database.orders

  properties:
    protocol: JDBC
    purpose: transactional persistence

  state:
    lifecycle: current

  evidence:
    - evd.config.001
    - evd.source.014

  confidence:
    level: high
    score: 0.96
```

Initial relationship vocabulary:

```text
contains
part_of

depends_on
uses
implements
extends

calls
invokes
exposes

reads_from
writes_to
reads_writes

publishes
subscribes
consumes
produces

sends_to
receives_from

deploys_to
runs_on
hosted_on

connects_to
communicates_with

authenticates_with
authorized_by
secured_by

stores
retrieves

transforms
aggregates
replicates
syncs

replaces
replaced_by

migrates_to
depends_on_data

owned_by
managed_by
```

The vocabulary must be extensible.

---

# 12. Claim Model

A claim represents an architectural statement.

```yaml
claim:
  id: claim.001

  statement:
    "OrderService writes orders to PostgreSQL."

  subject:
    entity: service.order

  predicate:
    relationship: writes_to

  object:
    entity: database.orders

  type:
    fact

  evidence:
    - evd.source.001
    - evd.config.003

  confidence:
    level: high

  review:
    status: unreviewed
```

Claim types:

```text
fact
inference
assumption
recommendation
```

---

# 13. Fact Model

Facts are directly supported by evidence.

```yaml
fact:
  subject: service.order
  predicate: uses
  object: technology.postgresql

  evidence:
    - evd.config.001
```

Facts should preferably come from deterministic extraction.

---

# 14. Reasoning / Inference Model

Reasoning creates conclusions from evidence and facts.

```yaml
inference:
  statement:
    "PostgreSQL is likely the primary transactional database."

  based_on:
    - fact.001
    - fact.002
    - fact.003

  confidence:
    level: medium
```

The reasoning engine must preserve supporting evidence.

Reasoning should produce structured claims rather than final prose.

---

# 15. Reasoning Layer

The Reasoning Layer answers questions such as:

```text
What does this component do?

Why does this service depend on Kafka?

Which database appears to be the system of record?

Is this integration synchronous or asynchronous?

Which components form a bounded context?

Which dependencies are architectural boundaries?

Which relationships are strongly supported?

Which relationships are uncertain?

What contradictions exist?

What architecture is implied by the implementation?
```

Reasoning should be performed using:

```text
rules
graph algorithms
deterministic analysis
local LLM
optional cloud LLM
```

The result is a structured knowledge model.

---

# 16. Assumption Model

```yaml
assumption:
  statement:
    "The legacy batch scheduler is still active in production."

  reason:
    "Configuration exists, but no runtime evidence was available."

  confidence:
    level: low
```

Assumptions should appear in:

```text
Open Questions
```

unless confirmed.

---

# 17. Unknown Model

Unknowns are explicit knowledge gaps.

```yaml
unknown:
  question:
    "Who owns the Customer database?"

  affected_entity:
    database.customer

  priority:
    high
```

Unknowns are not errors.

They are useful outputs of architecture discovery.

---

# 18. Contradiction Model

```yaml
contradiction:
  id: contradiction.001

  topic:
    primary_database

  sources:

    - claim:
        "System uses Oracle."
      source:
        old-architecture.md

    - claim:
        "System uses PostgreSQL."
      source:
        application.yaml

  resolution:
    status: unresolved

  confidence:
    level: high
```

Resolution states:

```text
unresolved
resolved
accepted
rejected
obsolete
```

---

# 19. Knowledge Model

The Architecture Knowledge Model is the integrated representation:

```text
Entities
Relationships
Facts
Inferences
Assumptions
Unknowns
Contradictions
Evidence
Reviews
Risks
Decisions
Runtime scenarios
Deployment
Data flows
Security observations
Technology
```

Conceptually:

```text
                ┌──────────────┐
                │   Evidence   │
                └──────┬───────┘
                       ↓
              ┌─────────────────┐
              │      Facts      │
              └────────┬────────┘
                       ↓
              ┌─────────────────┐
              │    Reasoning    │
              └────────┬────────┘
                       ↓
              ┌─────────────────┐
              │  Architecture   │
              │ Knowledge Model │
              └─────────────────┘
```

---

# 20. Investigation Plan

The Evaluation Layer must be able to produce an **Investigation Plan**.

Example:

```yaml
investigation_plan:
  - id: task.001
    priority: high

    question:
      "Is Kafka used by PaymentService?"

    required_evidence:
      - source_code
      - configuration
      - deployment

    target:
      - service.payment

  - id: task.002
    priority: medium

    question:
      "Who owns CustomerDB?"

    required_evidence:
      - repository
      - infrastructure
      - documentation
```

The plan becomes input to the Collection Layer.

---

# 21. Generation Layer

The Generation Layer compiles the AKM into architecture views.

```text
Architecture Knowledge Model
             ↓
       View Selection
             ↓
       View Generator
             ↓
       Documentation Renderer
```

Potential outputs:

```text
C4 Context
C4 Container
C4 Component
Runtime Views
Deployment Architecture
Data Architecture
Security Architecture
Technology Architecture
Integration Architecture
ADR
Risk Report
Technical Debt Report
arc42
Architecture Summary
```

---

# 22. Views Are Projections

A view must not create independent architectural truth.

Example:

```yaml
view:
  id: view.system-context
  type: c4.system-context

  scope:
    root: system.main

  include:
    - system
    - external_system
    - actor

  relationship_types:
    - communicates_with
    - calls
    - sends_to
```

---

# 23. C4 Mapping

Recommended mapping:

```text
AKM Entity                  C4

system                      System
application/service         Container
component/module            Component
code element                Code
external_system             External System
actor                       Person
```

The mapping should remain configurable.

---

# 24. Runtime Scenario Model

```yaml
scenario:
  id: scenario.order-creation

  name: Create Order

  actors:
    - customer

  steps:

    - sequence: 1
      source: customer
      target: order-api
      action: submit order

    - sequence: 2
      source: order-api
      target: order-service
      action: create order

    - sequence: 3
      source: order-service
      target: postgres
      action: persist order
```

---

# 25. Deployment Model

```yaml
deployment:
  id: deployment.production

  environment: production

  nodes:
    - kubernetes.cluster

  deployments:
    - service.order
```

Deployment relationships:

```text
service
    ↓ deployed_to
deployment_unit
    ↓ runs_on
node
    ↓ hosted_in
cloud_resource
```

---

# 26. Data Model

Data architecture is represented independently.

Entities:

```text
DataStore
Schema
Table
Column
DataEntity
DataFlow
Transformation
DataProduct
```

Example:

```yaml
data_flow:
  id: flow.customer.erp-to-lake

  source:
    system: erp

  transformation:
    process: customer-cdc

  target:
    datastore: customer-lake

  method:
    CDC

  evidence:
    - evd.kafka.001
```

---

# 27. Security Model

Security observations:

```text
authentication
authorization
encryption
secret_management
network_boundary
identity_provider
security_control
```

Example:

```yaml
security_observation:
  type: authentication
  subject: api.gateway
  mechanism: OAuth2
  evidence:
    - evd.openapi.001
```

Unknown security properties become investigation questions.

---

# 28. Quality Attribute Model

```yaml
quality_attribute:
  category: availability

  statement:
    "The service is deployed with multiple replicas."

  evidence:
    - evd.kubernetes.001

  status:
    observed
```

Do not infer quantitative guarantees without evidence.

---

# 29. Architecture Decision Model

```yaml
decision:
  id: adr.kafka

  title:
    "Kafka is used for asynchronous integration."

  status:
    observed

  decision:
    "Kafka is currently used for event transport."

  evidence:
    - evd.kafka.config
```

Possible statuses:

```text
observed
confirmed
proposed
superseded
deprecated
unknown
```

---

# 30. Risk Model

```yaml
risk:
  id: risk.shared-database

  title:
    "Multiple services share the same database."

  category:
    architecture

  evidence:
    - evd.service.001
    - evd.service.002

  impact:
    medium

  likelihood:
    medium

  confidence:
    high

  status:
    candidate
```

The plugin generates risk candidates. Business impact should remain explicit unless supported by evidence.

---

# 31. Technical Debt Model

```yaml
technical_debt:
  id: debt.legacy-framework

  subject:
    technology.legacy-framework

  description:
    "Application depends on an old framework version."

  evidence:
    - evd.requirements

  confidence:
    high
```

---

# 32. Evaluation Layer

After generating documentation, EKOS must evaluate the result.

The evaluator should behave as an independent architecture reviewer.

It should check:

```text
Completeness
Consistency
Evidence coverage
Traceability
Contradictions
Unsupported claims
Cross-view consistency
Architecture quality
Documentation quality
```

---

# 33. Evaluation Questions

The evaluator should ask:

```text
Are all major systems represented?

Are all important services represented?

Are external integrations documented?

Are important dependencies supported by evidence?

Are there unsupported architectural claims?

Are there contradictions?

Are critical data flows missing?

Are deployment relationships complete?

Are security boundaries represented?

Does C4 Context agree with C4 Container?

Does the data architecture agree with application architecture?

Does the deployment view agree with infrastructure?

Are important unknowns explicitly identified?

Is the generated documentation internally consistent?
```

---

# 34. Evaluation Result

Example:

```yaml
evaluation:
  score: 0.78

  dimensions:

    completeness:
      score: 0.81

    consistency:
      score: 0.72

    evidence_coverage:
      score: 0.91

    traceability:
      score: 0.95

  issues:

    - id: issue.001
      type: missing_relationship
      severity: high
      description:
        "PaymentService dependency is not documented."

    - id: issue.002
      type: unsupported_claim
      severity: medium
      description:
        "99.9% availability has no evidence."

    - id: issue.003
      type: contradiction
      severity: high
      description:
        "Documentation says Oracle; current configuration says PostgreSQL."
```

---

# 35. Evaluation Must Produce Actionable Feedback

The evaluator must not simply say:

```text
Documentation quality: 78%
```

It must explain:

```text
What is wrong?
Why does it matter?
What evidence is missing?
What should be inspected?
What should be regenerated?
```

Example:

```yaml
feedback:
  issue: issue.001

  action:
    "Inspect PaymentService Kafka consumers."

  target:
    - src/payment/
    - application.yml
    - deployment/payment.yaml

  expected_evidence:
    - source_code
    - configuration
```

---

# 36. Targeted Re-Collection

Feedback creates new collection tasks.

```text
Evaluator
   ↓
Knowledge Gap
   ↓
Investigation Task
   ↓
Targeted Collector
   ↓
New Evidence
   ↓
Reasoning
```

This is the core agentic behavior.

---

# 37. Regeneration Strategy

EKOS should not necessarily regenerate everything.

If only:

```text
PaymentService → Kafka
```

changed, regenerate affected views:

```text
C4 Container
Integration View
Runtime Scenario
Architecture Summary
```

This reduces cost and improves iteration speed.

---

# 38. Evaluation Loop

Complete loop:

```text
                 ┌───────────────┐
                 │    COLLECT    │
                 └───────┬───────┘
                         ↓
                 ┌───────────────┐
                 │    ANALYZE    │
                 └───────┬───────┘
                         ↓
                 ┌───────────────┐
                 │    REASON     │
                 └───────┬───────┘
                         ↓
                 ┌───────────────┐
                 │  UPDATE AKM   │
                 └───────┬───────┘
                         ↓
                 ┌───────────────┐
                 │   GENERATE    │
                 └───────┬───────┘
                         ↓
                 ┌───────────────┐
                 │   EVALUATE    │
                 └───────┬───────┘
                         ↓
                  Quality OK?
                    /       \
                  YES        NO
                   ↓          ↓
                 END      FEEDBACK
                              ↓
                       INVESTIGATION
                              ↓
                         COLLECTION
```

---

# 39. Stopping Criteria

The loop must have explicit stopping conditions.

Success:

```text
quality_score >= configured threshold
AND
no critical contradictions
AND
no high-priority unresolved issues
AND
evidence coverage >= configured threshold
```

Alternative termination:

```text
no meaningful new evidence available
```

or:

```text
maximum iterations reached
```

Example:

```yaml
evaluation_policy:
  quality_threshold: 0.90
  evidence_coverage_threshold: 0.90
  max_iterations: 5
  fail_on_critical_contradiction: true
```

---

# 40. Final Investigation Report

At the end of the process EKOS should report:

```text
Architecture reconstruction completed.

Quality score:              91%
Evidence coverage:          94%
Iterations:                 3

Entities:                   143
Relationships:               387
Claims:                     219
Confirmed claims:           187
Inferences:                  25
Unknowns:                     7
Unresolved contradictions:   2

Critical issues:              0
High-priority issues:         0
Medium-priority issues:       3

LLM:
  Mode: local
  Provider: Ollama
  Model: <configured model>
```

This report is itself generated from the AKM.

---

# 41. Local LLM Architecture

LLM access must be abstracted.

```text
LLMProvider
    |
    +-- LocalOllamaProvider
    +-- LocalLlamaCppProvider
    +-- CloudProvider
```

Example:

```yaml
llm:
  mode: local

  provider: ollama

  model:
    name: qwen3
    endpoint: http://localhost:11434
```

The model remains configurable.

---

# 42. LLM Task Allocation

Not every task requires an LLM.

| Task | Preferred mechanism |
|---|---|
| File discovery | deterministic |
| AST parsing | deterministic |
| Dependency extraction | deterministic |
| Terraform parsing | deterministic |
| Kubernetes parsing | deterministic |
| SQL extraction | deterministic |
| Entity classification | local LLM |
| Semantic naming | local LLM |
| Relationship inference | local LLM + rules |
| Architecture reasoning | local LLM + graph |
| Documentation generation | local LLM |
| Documentation evaluation | local LLM + rules |
| Investigation planning | local LLM + rules |
| Architecture diff | deterministic |
| Evidence validation | deterministic |

---

# 43. Privacy

Default:

```text
No source code leaves the machine.
```

Cloud inference requires explicit configuration.

Example:

```bash
ekos architecture analyze . --llm cloud
```

EKOS should make the data-processing mode visible.

---

# 44. Hybrid Mode

Hybrid mode minimizes transmitted information:

```text
Repository
    ↓
Local static analysis
    ↓
Local evidence extraction
    ↓
Secret detection / redaction
    ↓
Context minimization
    ↓
Optional cloud reasoning
    ↓
Structured result
    ↓
Local AKM
```

---

# 45. Offline Mode

EKOS should support:

```bash
ekos architecture analyze . --offline
```

Guarantee:

```text
No network calls.
```

Possible implementation:

```text
deterministic analyzers
+
local LLM
+
local storage
+
local documentation renderer
```

Offline capability is especially valuable for enterprise and regulated environments.

---

# 46. LLM Output Contract

LLMs must never directly write authoritative architecture state.

Preferred:

```text
LLM
 ↓
Structured JSON
 ↓
Schema validation
 ↓
Evidence linking
 ↓
Confidence validation
 ↓
AKM
 ↓
Documentation
```

Example:

```json
{
  "type": "inference",
  "subject": "service.order",
  "predicate": "depends_on",
  "object": "service.payment",
  "confidence": "medium",
  "reasoning_evidence": [
    "evd.source.021",
    "evd.config.004"
  ]
}
```

---

# 47. Provenance

Every generated claim should retain provenance.

```yaml
provenance:
  generated_by:
    type: local_llm
    provider: ollama
    model: qwen3

  prompt_version:
    architecture-inference-v3

  input:
    - evd.source.001
    - evd.source.002

  generated_at:
    2026-08-21T13:22:00Z
```

---

# 48. Prompt Versioning

Prompts are versioned artifacts.

Examples:

```text
architecture-entity-classification-v1
architecture-relationship-inference-v2
architecture-reasoning-v1
architecture-evaluation-v1
architecture-investigation-v1
architecture-summary-v1
```

Changing a prompt may change the resulting architecture model, so provenance must record the version.

---

# 49. Model Versioning

The AKM schema must be versioned.

```text
AKM v1.0
AKM v1.1
AKM v2.0
```

Migration mechanisms will eventually be required.

---

# 50. Architecture Baseline

A baseline is an immutable snapshot.

```yaml
baseline:
  id: baseline.2026-08-21

  source_commit:
    abc123

  generated_at:
    2026-08-21T13:00:00Z

  model_version:
    2.0

  statistics:
    entities: 143
    relationships: 387
    claims: 219

  evaluation:
    score: 0.91
```

---

# 51. Architecture Diff

Compare structured baselines:

```text
Baseline A
    ↓
Baseline B
```

Example:

```text
Added:
  PaymentService

Removed:
  LegacyPaymentAdapter

Changed:
  OrderService → PaymentService

New dependency:
  PaymentService → PaymentDB
```

Diff must operate on the structured model, not Markdown text.

---

# 52. Architecture Drift

Drift compares:

```text
documented architecture
```

with:

```text
observed architecture
```

Example:

```yaml
drift:
  subject:
    service.order

  documented:
    database: oracle

  observed:
    database: postgresql

  severity:
    high

  evidence:
    - evd.config
    - evd.schema
```

Drift detection can become a continuous operation after initial architecture reconstruction.

---

# 53. Human Review

Human review should operate at claim and relationship level.

Example:

```yaml
review:
  claim_id: claim.001

  status: confirmed

  reviewer:
    user: architect@example.com

  comment:
    "Confirmed with production team."

  timestamp:
    2026-08-21T13:00:00Z
```

The next reasoning cycle must respect confirmed human decisions.

---

# 54. Storage

Initial implementation:

```text
JSON
```

Recommended structure:

```text
.ekos/
└── architecture/
    ├── model.json
    ├── entities.json
    ├── relationships.json
    ├── claims.json
    ├── evidence.json
    ├── investigations.json
    ├── evaluations.json
    ├── reviews.json
    ├── baselines/
    └── provenance.json
```

A graph database or embedded graph engine can be introduced later.

---

# 55. Graph Representation

Conceptually:

```text
(Entity)-[Relationship]->(Entity)
```

Example:

```text
OrderService
   |
   | writes_to
   v
PostgreSQL
```

Evidence attaches to the relationship:

```text
OrderService
   |
   | writes_to
   |
PostgreSQL
   |
   +-- evidence
       ├── application.yml
       ├── repository.py
       └── terraform/
```

---

# 56. Architecture Queries

The model should support:

```text
find all services depending on database.customer

find all external systems connected to system.orders

find all paths between ERP and Power BI

find all services deployed in production

find all undocumented integrations

find all low-confidence relationships

find all architecture drift

find all unresolved high-priority questions
```

---

# 57. MCP Interface

The AKM should eventually be exposed through MCP.

Initial tools:

```text
architecture_search
architecture_get_entity
architecture_get_relationships
architecture_get_dependencies
architecture_get_context
architecture_get_runtime
architecture_get_deployment
architecture_get_data_flow
architecture_get_evidence
architecture_get_drift
architecture_get_risks
architecture_get_decisions
architecture_get_questions
architecture_get_evaluation
architecture_ask
```

This allows Claude Code and other agents to interrogate the architecture model.

---

# 58. Example Architecture Question

User:

> Why does OrderService depend on Kafka?

EKOS:

```text
OrderService publishes OrderCreated events.

Evidence:

1. src/order/events.py:42-71
2. Kafka producer configuration
3. Kafka dependency declaration

Confidence:
HIGH

Relationship:
OrderService --publishes--> orders.created
```

---

# 59. Example Agentic Investigation

User:

> Document this legacy application.

EKOS:

```text
Iteration 1

Collected:
  source code
  configuration
  existing documentation

Found:
  83 entities
  147 relationships

Evaluation:
  68%

Problems:
  Kafka integration uncertain
  database ownership unknown
  deployment topology incomplete
```

EKOS creates:

```text
Investigation Plan

1. Inspect Kafka consumers
2. Inspect deployment manifests
3. Search ownership metadata
```

Then:

```text
Iteration 2

New evidence:
  Kafka consumer configuration
  Kubernetes manifests

Updated:
  112 entities
  231 relationships

Evaluation:
  84%
```

Then:

```text
Iteration 3

Remaining issues:
  2 medium unknowns

Evaluation:
  93%

STOP
```

---

# 60. Plugin Architecture

```text
ekos-architecture
│
├── collection
│   ├── source
│   ├── config
│   ├── terraform
│   ├── kubernetes
│   ├── sql
│   ├── api
│   └── git
│
├── evidence
│
├── analysis
│
├── reasoning
│
├── model
│
├── investigation
│
├── evaluation
│
├── generation
│
├── views
│
├── documentation
│
├── diagrams
│
├── llm
│
└── mcp
```

---

# 61. Extractor Contract

Conceptual Rust interface:

```rust
trait ArchitectureExtractor {
    fn name(&self) -> &str;

    fn supports(&self, input: &Input) -> bool;

    fn extract(
        &self,
        input: &Input,
    ) -> Result<Vec<Evidence>>;
}
```

Extractors produce evidence, not final documentation.

---

# 62. Reasoning Contract

```rust
trait ArchitectureReasoningEngine {
    fn reason(
        &self,
        evidence: &[Evidence],
        model: &ArchitectureModel,
    ) -> Result<Vec<Claim>>;
}
```

Implementations may combine:

```text
rules
graph algorithms
local LLM
cloud LLM
```

---

# 63. Evaluation Contract

```rust
trait ArchitectureEvaluator {
    fn evaluate(
        &self,
        model: &ArchitectureModel,
        documents: &[Document],
    ) -> EvaluationReport;
}
```

The evaluator should be logically separated from the generator.

This separation is important to reduce self-confirming generation.

---

# 64. Investigation Planner Contract

```rust
trait InvestigationPlanner {
    fn plan(
        &self,
        evaluation: &EvaluationReport,
        model: &ArchitectureModel,
    ) -> Result<Vec<InvestigationTask>>;
}
```

---

# 65. Collection Contract

```rust
trait TargetedCollector {
    fn collect(
        &self,
        task: &InvestigationTask,
    ) -> Result<Vec<Evidence>>;
}
```

This enables the feedback loop.

---

# 66. Quality Gates

Before final publication:

```text
[✓] model schema valid
[✓] entity references valid
[✓] relationships valid
[✓] evidence references valid
[✓] confidence present
[✓] LLM claims structured
[✓] no critical contradictions
[✓] diagrams valid
[✓] secrets detected and removed
[✓] provenance recorded
[✓] evaluation threshold reached
```

---

# 67. MVP

The first implementation should support:

```text
Git repository
Markdown documentation
Source code
Configuration
Git metadata

Evidence
Entities
Relationships
Facts
Inferences
Unknowns
Confidence
Provenance

Local LLM

C4 Context
C4 Container
Basic Runtime View
Technology View

Markdown output

Basic evaluator
Basic feedback
Targeted re-collection
Maximum iteration limit
```

Do not attempt full enterprise architecture management in MVP-1.

---

# 68. Phase 2

Add:

```text
Terraform
Kubernetes
OpenAPI
SQL
Data Architecture
Deployment Architecture
Security Architecture
Architecture Drift
Architecture Diff
Baselines
MCP
Human review UI
```

---

# 69. Phase 3

Add:

```text
Runtime telemetry
Logs
Metrics
Traces
Continuous drift detection
Architecture Q&A Agent
Target Architecture
Migration Architecture
ADR generation
Architecture fitness checks
Architecture governance
```

---

# 70. Evaluation Dataset

Maintain representative test repositories:

```text
monolith
microservices
legacy Java
legacy .NET
Python platform
data platform
ETL/DWH
event-driven architecture
Kubernetes
Terraform
hybrid cloud
```

Each should have a manually reviewed expected architecture model.

---

# 71. Evaluation Metrics

Measure:

### Entity Precision

Percentage of discovered entities that are correct.

### Entity Recall

Percentage of important entities discovered.

### Relationship Precision

Percentage of generated relationships that are correct.

### Evidence Coverage

Percentage of important claims supported by evidence.

### Hallucination Rate

Percentage of claims without sufficient evidence.

### Drift Detection Accuracy

Percentage of known documentation discrepancies detected.

### Human Correction Rate

Percentage of generated claims requiring architect correction.

### Iteration Efficiency

How much architecture quality improves per investigation iteration.

### Local-vs-Cloud Quality

Compare local and cloud LLM configurations.

---

# 72. Non-Goals

This RFC does not define:

```text
complete enterprise architecture governance
business process management
full runtime observability
automatic architecture remediation
automatic production changes
```

The initial goal is:

> **reconstruct, reason about, evaluate, and document architecture accurately.**

---

# 73. Product Positioning

The plugin should not be positioned simply as:

> AI documentation generator.

A stronger positioning is:

> **EKOS Architecture Intelligence reconstructs legacy system architecture from code, infrastructure, data and documentation, continuously validates the result, and produces evidence-backed architecture documentation.**

Another concise formulation:

> **Give EKOS a legacy repository. It investigates the system, reconstructs its architecture, finds what the old documentation got wrong, and produces an evidence-backed architecture baseline.**

---

# 74. Final Architecture Principle

The defining EKOS architecture loop is:

```text
                    ┌──────────────┐
                    │    COLLECT   │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │    ANALYZE   │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │    REASON    │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │  BUILD AKM   │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │   GENERATE   │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │   EVALUATE   │
                    └──────┬───────┘
                           ↓
                    ┌──────────────┐
                    │   FEEDBACK   │
                    └──────┬───────┘
                           ↓
                 Investigation Plan
                           ↓
                       COLLECT
                           ↺
```

The most important property is **traceability**:

```text
Documentation statement
        ↓
Architecture claim
        ↓
Architecture relationship
        ↓
Reasoning
        ↓
Evidence
        ↓
Original source
```

The most important operational property is **iterative investigation**:

```text
"I don't know"
      ↓
"What evidence do I need?"
      ↓
"Where can I find it?"
      ↓
"Collect it."
      ↓
"Re-evaluate."
```

The most important privacy property is:

```text
Sensitive repository
      ↓
Local deterministic analysis
      ↓
Local LLM where possible
      ↓
Local Architecture Knowledge Model
      ↓
Local documentation
```

Only explicitly selected data may leave the developer's environment.

Therefore, the Architecture Knowledge Model should be considered the **persistent architectural memory of EKOS**, while the Collect → Analyze → Reason → Generate → Evaluate → Feedback loop is the **architecture intelligence engine that continuously improves that memory**.

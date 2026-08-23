# RFC 0068 — EKOS Architecture Documentation Standard & Deliverable Model

**Status:** Proposed
**Author:** External contribution, filed into this repo's RFC sequence 2026-08-22
**Created:** date unknown (source document undated); filed 2026-08-22
**Parent RFCs:** [RFC 0065](0065-architecture-knowledge-model-v2.md) (EKOS-ARCHMODEL-002),
[RFC 0066](0066-architecture-agent-state-machine.md) (EKOS-ARCHAGENT-003) — both referenced by
their original external ids in the source document (`EKOS-ARCHMODEL-002`, `EKOS-ARCHAGENT-003`),
resolved here to this repo's real RFC numbers. A third parent, `EKOS-ARCHDOC-001` ("Architecture
Documentation Compiler"), is referenced but — as already noted in RFC 0064/0065 — was never
provided and does not exist elsewhere in this repository; kept as an unresolved reference rather
than invented.

**Original identifier:** EKOS-ARCHDOC-004 (external doc's own numbering, not this repo's RFC
sequence — kept for provenance)
**Relationship to shipped work:** RFC 0065 Phase 1-3 + RFC 0066's MVP agent + RFC 0067
(`devlog_70`, `devlog_71`) already implement a real subset of what this RFC specifies — deterministic
crate-topology extraction, LLM-backed role classification, a deterministic evaluator, targeted
re-collection, and the `ekos architecture investigate` orchestrating loop. This RFC is the fuller
target standard those were the first slice of, combining ISO/IEC/IEEE 42010, arc42, C4, and ISO/IEC
25010 into one documentation package spec. Per explicit instruction: **the full feature scope
below is the plan for continued build-out — not to be trimmed to another MVP slice** the way RFC
0065 Phase 1 and RFC 0067 were deliberately scoped down. See TODO.md's "Architecture Documentation
Standard (RFC 0068)" entry for the phased build roadmap (phased for sequencing only, not for
permanently dropping anything described here).

## Increment 1 implemented (2026-08-22) — RFC 0069

System Context view (§15) and real Documentation Drift detection (§31-32) — the first concrete
slice off the §61 MVP list. See `RFC 0069` and `devlog_72.md` for the full design and live
verification (this repo's own real, already-committed ledger already contained 7 genuine drift
findings from earlier real `architecture-reasoning` runs this session). Next queued increment,
already logged in TODO.md: Basic Component View + a dedicated Technology Inventory view.

## Increment 2 implemented (2026-08-22) — RFC 0070

Basic Component View (§18) + Technology Inventory (§61 MVP) — resolved the Crate↔File design
question Increment 1 deferred by reusing RFC 0044's existing `Rollup` grouping (no new extraction
needed). Also found and fixed a real bug live: `KirRelationship`'s non-deterministic ids let
logically-identical relationships accumulate as real duplicates across repeated commits — fixed in
this view's own rendering, tracked as its own separate item (not fully fixed everywhere) in
TODO.md. See `RFC 0070` and `devlog_73.md`.

## Increment 3 implemented (2026-08-22) — RFC 0071

Architecture Summary / Executive Overview (§14) and Basic Runtime View (§20) — the last two §61
MVP view items. Architecture Summary populates only real-evidenced fields, stating explicitly why
`Purpose`/`Architecture style`/`Major risks`/`Architecture confidence` aren't computed rather than
fabricating them. Runtime View links to the already-generated `SequenceDiagrams.md` rather than
duplicating it. Hit the same relationship-duplication bug RFC 0070 found, in a new location — fixed
the same way, with its own regression test. See `RFC 0071` and `devlog_74.md`.

## Root-cause fix (2026-08-22) — RFC 0072

Having hit the same relationship-duplication bug twice independently (RFC 0070, RFC 0071), fixed
the actual, concretely observed instance at its source — `crate_topology_analyzer.rs`'s `DependsOn`
edges now get a deterministic id, matching how `Crate`/`Technology`/`Claim`/`ArchitectureGap`
already do. Deliberately scoped narrow, not a blanket fix across all 136 `KirRelationship::new()`
call sites — a real counter-example (`sql_analyzer.rs`'s `ForeignKey` edges, which can legitimately
repeat between the same two tables via different columns) confirmed a blanket fix would have been
actively wrong. Live-verified end to end against the real default v3 `FactLedger` backend: three
independent commits of the same real dependency graph produced the same 2 relationship ids each
time, not a growing count. See `RFC 0072` and `devlog_75.md`.

## Increment 5 implemented (2026-08-22) — RFC 0073

Closed out the last open RFC 0068 §61 MVP item: `docs-gen` now produces a standalone SVG artifact
(`system-context.svg`), not just Mermaid-in-Markdown, for the System Context diagram, via a new
generic, dependency-free, deterministic `(nodes, edges) -> SVG` renderer (`render_graph_svg`,
`layer_nodes`). Deliberately scoped to one diagram, not all four `graph TD` producers in
`docs-gen` — the primitive itself is generic and ready for the other three as concretely scoped
follow-on work (tracked in TODO.md), not a redesign. Live-verified against this repo's own
already-committed ledger: a real, well-formed 46-node/45-edge SVG. All six RFC 0068 §61 MVP view
items are now shipped. See `RFC 0073` and `devlog_76.md`.

## Increment 6 implemented (2026-08-22) — RFC 0074, opening §62 Phase 2

First §62 Phase 2 increment: a real `## Data Architecture` section (§22) in `Architecture.md`,
reusing already-compiled `Table`/`Dataset` objects (real data stores, with real foreign-key counts)
and the existing Transformation IR (real data flows/lineage, link-through to `SequenceDiagrams.md`'s
Data-Flow Sequences rather than duplicating it). Investigated three candidate Phase 2 starting
points against the real codebase first (Data Architecture vs. Human Review vs. a new
Terraform/Kubernetes/OpenAPI extractor) and picked Data Architecture because real compiled data
already backs half its dimensions; Data Domains/Ownership/Lifecycle/Data Quality each say explicitly
why they're not computed rather than being guessed at. Found and fixed a real, previously-unpinned
stale cross-reference (`## Technologies` → `## Technology Inventory`, missed when RFC 0070 renamed
the section) along the way. Live-verified against this repo's own real `tests/fixtures/ecommerce.sql`
fixture (6 real tables, real FK counts). See `RFC 0074` and `devlog_77.md`.

## Increment 7 implemented (2026-08-22) — RFC 0075

Closed RFC 0074's own follow-on list. Shipped real code for two: `TransformNode` Source/Sink nodes
now link to the real `Table`/`Dataset` object they name (`ReadsFrom`/`WritesTo`, unambiguous exact
match only, deterministic ids from the start), and Data Domains now groups tables by their real
schema qualifier when the source DDL provides one. For the other two, found and corrected a real
factual error in RFC 0074's own Ownership text (`git_analyzer.rs`'s `OwnedBy` edge connects a
commit event to its author, never a `File` object — RFC 0074 had claimed otherwise), replacing a
vague "not yet computed" with a precise, concretely-scoped blocker (a missing `Table`→`File` link,
plus a still-missing per-file ownership derivation in `git_analyzer.rs`). Data Quality confirmed
genuinely out of reach without RFC 0068 §63 Phase 3 runtime telemetry, by checking, not assuming.
Live-verified end to end against a disposable fixture: real `ReadsFrom` edges, idempotent across a
re-commit. See `RFC 0075` and `devlog_78.md`.

## 1. Summary

This RFC defines the target architecture documentation package produced by EKOS after reconstructing an existing software system.

It combines complementary concepts from:

- **ISO/IEC/IEEE 42010:2022** — architecture descriptions, stakeholders, concerns, viewpoints, views, model kinds, and correspondence.
- **arc42** — practical structure for architecture documentation.
- **C4 Model** — hierarchical architecture views: System Context, Container, Component, and optionally Code.
- **ISO/IEC 25010:2023** — terminology for software product quality characteristics.

The standards are not treated as competing formats:

```text
ISO 42010  → architecture description framework
arc42      → documentation structure
C4         → architecture visualization
ISO 25010  → quality terminology
EKOS       → evidence, reasoning, evaluation, traceability and drift
```

The central principle is:

> **EKOS does not merely generate architecture documentation. It reconstructs, validates, explains, and continuously verifies architecture against available evidence.**

---

# 2. Motivation

Legacy systems commonly contain conflicting sources:

```text
Source code
Configuration
Infrastructure as Code
Kubernetes
Database schemas
SQL
API specifications
CI/CD
Git history
Runtime telemetry
Existing documentation
Human knowledge
```

These sources can disagree.

Example:

```text
OLD DOCUMENTATION
Oracle
   │
   │ contradiction
   ▼
CURRENT CODE
PostgreSQL
```

EKOS should not simply choose one source and generate prose. It should produce:

```text
DOCUMENTATION DRIFT DETECTED

Claim:
    System uses Oracle

Current evidence:
    PostgreSQL

Confidence:
    HIGH

Evidence:
    ✓ application configuration
    ✓ PostgreSQL dependency
    ✓ database migrations
    ✓ infrastructure configuration
```

Therefore architecture documentation is a **validated projection of architectural knowledge**, not free-form generated text.

---

# 3. Goals

EKOS Architecture Documentation MUST:

1. provide a complete architecture description;
2. support multiple stakeholder concerns;
3. support multiple architecture viewpoints;
4. provide consistent architecture views;
5. distinguish facts, inferences, assumptions, unknowns and recommendations;
6. preserve evidence and provenance;
7. expose uncertainty;
8. detect contradictions;
9. detect documentation drift;
10. maintain cross-view consistency;
11. support architecture evolution;
12. support quality-related architecture analysis;
13. support human validation;
14. be generated from one Architecture Knowledge Model;
15. support both human and machine consumption.

---

# 4. Non-Goals

This RFC does not attempt to:

- replace ISO/IEC/IEEE 42010;
- reproduce arc42 verbatim;
- require every C4 level for every system;
- infer unsupported business requirements;
- make unsupported quantitative quality claims;
- replace human architecture ownership;
- automatically remediate production architecture.

---

# 5. Core Design Principle

The canonical representation is:

```text
Architecture Knowledge Model (AKM)
```

Documentation is a projection:

```text
AKM
 │
 ├── C4 views
 ├── arc42 documentation
 ├── Deployment views
 ├── Data views
 ├── Security views
 ├── Quality views
 ├── ADRs
 └── Custom architecture views
```

> **Documents are compiled outputs. The Architecture Knowledge Model is the architectural knowledge reconstructed by EKOS.**

---

# 6. Standards Mapping

## 6.1 ISO/IEC/IEEE 42010:2022

EKOS adopts the conceptual chain:

```text
Stakeholders
      ↓
Stakeholder Concerns
      ↓
Architecture Viewpoints
      ↓
Architecture Views
      ↓
Architecture Models
```

The architecture description should explicitly identify relevant stakeholders, concerns, viewpoints, views, model kinds, and correspondence between models.

## 6.2 arc42

arc42 provides the principal narrative structure:

```text
Introduction & Goals
Constraints
Context & Scope
Solution Strategy
Building Block View
Runtime View
Deployment View
Cross-cutting Concepts
Architecture Decisions
Quality Requirements
Risks & Technical Debt
Glossary
```

EKOS extends this with evidence, confidence, drift, traceability, data, security and architecture evolution.

## 6.3 C4 Model

C4 provides:

```text
Level 1 — System Context
Level 2 — Container
Level 3 — Component
Level 4 — Code
```

EKOS should normally generate the first three. Code-level views are generated selectively.

## 6.4 ISO/IEC 25010:2023

EKOS uses ISO/IEC 25010:2023 terminology for quality characteristics.

Quality claims follow:

```text
Evidence
   ↓
Observation
   ↓
Quality characteristic
   ↓
Possible implication
   ↓
Confidence
```

For example:

```text
Observed:
Kubernetes deployment has 3 replicas.

Valid:
The deployment uses multiple replicas.

Invalid:
Availability = 99.99%.
```

---

# 7. Architecture Documentation Package

The target package is:

```text
EKOS Architecture Documentation
│
├── 00. Architecture Description
├── 01. Executive Overview
├── 02. System Context
├── 03. System Landscape
├── 04. Container Architecture
├── 05. Component Architecture
├── 06. Runtime Architecture
├── 07. Deployment Architecture
├── 08. Data Architecture
├── 09. Integration Architecture
├── 10. Security Architecture
├── 11. Technology Architecture
├── 12. Quality Architecture
├── 13. Architecture Decisions
├── 14. Risks & Technical Debt
├── 15. Architecture Evolution
├── 16. Documentation Drift
├── 17. Architecture Traceability
├── 18. Glossary
├── 19. Open Questions
└── 20. Appendices
```

---

# 8. 00. Architecture Description

This section describes the architecture description itself:

```text
Purpose
Scope
System Under Analysis
Stakeholders
Stakeholder Concerns
Architecture Principles
Viewpoints
Model Kinds
Architecture Description Conventions
```

It should identify which system and environments are covered and what evidence was analyzed.

---

# 9. Stakeholders

Potential stakeholders:

```text
Business Owner
Product Owner
Enterprise Architect
Solution Architect
Data Architect
Security Architect
Developer
Operations
Platform Engineer
Compliance
Support
```

Where possible, EKOS should distinguish:

```text
observed
inferred
human-confirmed
unknown
```

---

# 10. Stakeholder Concerns

EKOS should map stakeholders to concerns.

Example:

```text
Data Architect
    ↓
data ownership
data lineage
data quality
data lifecycle
```

```text
Security Architect
    ↓
authentication
authorization
trust boundaries
secrets
encryption
```

This mapping drives viewpoint selection.

---

# 11. Architecture Viewpoints

Potential viewpoints:

```text
System Context Viewpoint
Container Viewpoint
Component Viewpoint
Runtime Viewpoint
Deployment Viewpoint
Data Viewpoint
Integration Viewpoint
Security Viewpoint
Technology Viewpoint
Quality Viewpoint
Evolution Viewpoint
Drift Viewpoint
```

Custom viewpoints should eventually be supported.

---

# 12. Architecture Views

Examples:

```text
C4 System Context View
C4 Container View
C4 Component View
Runtime Sequence View
Deployment View
Data Flow View
Security Trust Boundary View
Technology Inventory View
Architecture Drift View
```

Multiple views may be generated from the same model.

---

# 13. Model Kinds

EKOS should distinguish:

```text
Structural Model
Runtime Model
Deployment Model
Data Model
Integration Model
Security Model
Technology Model
Quality Model
Evolution Model
Evidence Model
Drift Model
```

---

# 14. Executive Overview

A one-page summary answering:

```text
What is the system?
What does it do?
What are its major building blocks?
What technologies does it use?
What are its major dependencies?
What are its major risks?
How confident is EKOS?
```

Example:

```text
System:
    Legacy Order Management Platform

Purpose:
    Process customer orders and fulfilment.

Architecture style:
    Modular monolith with asynchronous integrations.

Primary technologies:
    Java, PostgreSQL, Kafka, Kubernetes.

Major external systems:
    SAP, CRM, Payment Gateway.

Major risks:
    Shared database
    Legacy framework
    Undocumented integration

Architecture confidence:
    87%
```

---

# 15. System Context

C4 Level 1.

It identifies:

```text
System boundary
Users
Actors
External systems
External services
External dependencies
Business context
```

Each relationship should retain:

```text
Purpose
Direction
Protocol where known
Evidence
Confidence
```

---

# 16. System Landscape

The broader environment:

```text
Enterprise
│
├── CRM
├── ERP
├── Order Management
├── Data Platform
├── Analytics
├── Customer Portal
└── Identity Platform
```

Where evidence allows, associate systems with:

```text
Business Domains
Business Capabilities
Organizational Ownership
```

---

# 17. Container Architecture

C4 Level 2.

Example:

```text
Order Management
│
├── Web Application
├── Order API
├── Order Service
├── Notification Service
├── PostgreSQL
├── Kafka
└── Batch Processor
```

Each container should include:

```text
Name
Purpose
Responsibilities
Technology
Owner
Dependencies
Interfaces
Data
Deployment
Evidence
Confidence
```

---

# 18. Component Architecture

C4 Level 3.

Example:

```text
Order Service
│
├── OrderController
├── OrderApplicationService
├── OrderDomain
├── PricingService
├── OrderRepository
└── EventPublisher
```

Generate only architecturally meaningful component views by default.

Do not generate huge class diagrams unless requested.

---

# 19. Code Architecture

C4 Level 4 is optional and on-demand.

Example:

```text
OrderApplicationService
    ↓
OrderDomain
    ↓
OrderRepository
```

---

# 20. Runtime Architecture

Runtime architecture explains behavior:

```text
Runtime Scenarios
Sequence Diagrams
Business Flows
Asynchronous Flows
Failure Scenarios
State Transitions
```

Typical scenarios:

```text
Create Order
Process Payment
Customer Registration
Data Synchronization
Batch Processing
Error Recovery
```

---

# 21. Deployment Architecture

Answers:

> Where does the architecture actually run?

It should include:

```text
Environments
Infrastructure
Compute
Containers
Clusters
Networks
Subnets
Cloud resources
Deployment units
```

Example:

```text
Azure
│
└── AKS
    ├── Order API
    ├── Payment Service
    └── Notification Service

Azure PostgreSQL
Azure Service Bus
Azure Key Vault
```

Cross-check:

```text
Source
  ↓
Container
  ↓
Deployment
  ↓
Infrastructure
```

---

# 22. Data Architecture

A major EKOS capability.

It covers:

```text
Data Domains
Data Stores
Schemas
Tables
Entities
Data Flows
Transformations
Lineage
Ownership
Lifecycle
Data Quality
```

Example:

```text
SAP
 │
 │ CDC
 ▼
Kafka
 │
 ▼
Data Ingestion
 │
 ▼
Raw Storage
 │
 ▼
Transformation
 │
 ▼
Curated Data
 ├── Power BI
 ├── ML
 └── Reporting
```

---

# 23. Integration Architecture

Integration inventory:

```text
REST
SOAP
GraphQL
Kafka
RabbitMQ
Files
SFTP
Database links
Batch
CDC
Events
```

Each integration:

```text
Source
Target
Direction
Protocol
Frequency
Payload
Authentication
Error Handling
Retry
Evidence
Confidence
```

---

# 24. Security Architecture

Includes:

```text
Trust Boundaries
Authentication
Authorization
Identity
Secrets
Encryption
Network Security
Security Controls
External Trust Relationships
Sensitive Data
Security Risks
```

Example:

```text
Internet
    ↓
API Gateway
    │ OAuth2
    ↓
Order API
    ├── PostgreSQL
    └── Kafka
```

Unknown security properties must be explicit.

---

# 25. Technology Architecture

Technology inventory should include:

| Technology | Version | Used by | Lifecycle | Confidence |
|---|---|---|---|---|
| Java | 11 | Order Service | Legacy | High |
| PostgreSQL | 15 | Order Service | Supported | High |
| Kafka | 3.x | Integration | Supported | Medium |
| Spring | 2.x | Order Service | Legacy | High |

The model must distinguish:

```text
Observed technology
Technology version
Lifecycle status
Potential risk
Recommendation
```

---

# 26. Quality Architecture

Use ISO/IEC 25010:2023 terminology.

Applicable characteristics include:

```text
Functional suitability
Performance efficiency
Compatibility
Interaction capability
Reliability
Security
Maintainability
Flexibility
```

For each:

```text
Evidence
Observation
Quality requirement if known
Architectural mechanism
Possible implication
Confidence
```

EKOS must not infer that an architectural mechanism proves a quantitative quality level.

---

# 27. Quality Requirements

Requirements must be separated from observations.

Example:

```yaml
quality_requirement:
  category: performance_efficiency

  statement:
    "Order creation should complete within 500 ms."

  source:
    product-requirements.md

  status:
    documented

  verification:
    unknown
```

---

# 28. Architecture Decisions

Each ADR should contain:

```text
Title
Status
Context
Problem
Decision
Alternatives
Trade-offs
Consequences
Evidence
Confidence
Date
Supersedes
Superseded by
```

Statuses:

```text
Observed
Confirmed
Inferred
Proposed
Superseded
Deprecated
Unknown
```

An inferred decision must not be presented as a human-approved decision.

---

# 29. Risks & Technical Debt

Risks should distinguish:

```text
Observed Risk
Inferred Risk
Potential Risk
Recommendation
```

Example:

```text
Observed:
Seven services share one database.

Inference:
This creates significant service coupling.

Recommendation:
Consider separating persistence boundaries.
```

Technical debt may include:

```text
Legacy technologies
Deprecated frameworks
Duplicated functionality
Tight coupling
Missing tests
Manual processes
Architecture violations
Unsupported components
Obsolete integrations
```

---

# 30. Architecture Evolution

Document:

```text
Historical Architecture
Current Architecture
Target Architecture where known
Migration Roadmap
Architecture Transitions
```

Git history can provide evidence.

Example:

```text
2018
Monolith
   ↓
2021
Modular Monolith
   ↓
2023
Kafka introduced
   ↓
2025
Payment extracted
   ↓
2026
Current Architecture
```

---

# 31. Documentation Drift

Documentation Drift is a first-class EKOS capability.

Definition:

> **Documentation drift is a discrepancy between documented architecture and architecture supported by current evidence.**

Examples:

### Technology drift

```text
Documentation:
Java 8

Current evidence:
Java 21
```

### Database drift

```text
Documentation:
Oracle

Current evidence:
PostgreSQL
```

### Deployment drift

```text
Documentation:
Virtual Machines

Current evidence:
Kubernetes
```

### Integration drift

```text
Documentation:
REST → Payment Gateway

Current evidence:
Kafka → Payment Service
```

### Ownership drift

```text
Documentation:
Team A

Current evidence:
CODEOWNERS → Team B
```

### Component drift

```text
Documentation:
LegacyPaymentService exists

Current evidence:
component no longer deployed
```

---

# 32. Documentation Drift Model

```yaml
drift:
  id: drift.001

  subject:
    database.orders

  documented_claim:
    technology: Oracle

  observed_claim:
    technology: PostgreSQL

  evidence:
    - application.yml
    - terraform/postgres.tf
    - db/migrations

  confidence:
    level: high

  severity:
    high

  status:
    detected
```

Human-readable output:

```text
DOCUMENTATION DRIFT DETECTED

Claim:
    The system uses Oracle.

Current evidence:
    PostgreSQL.

Confidence:
    HIGH

Evidence:
    ✓ application.yml
    ✓ PostgreSQL JDBC dependency
    ✓ database migrations
    ✓ Terraform configuration

Recommendation:
    Update architecture documentation.
```

---

# 33. Architecture Traceability

Every important claim should be traceable:

```text
Claim
  ↓
Architecture Relationship
  ↓
Evidence
  ↓
Source
  ↓
Location
  ↓
Commit / Timestamp
  ↓
Confidence
```

Example:

```text
Claim #124

Order Service writes to PostgreSQL.

Confidence:
HIGH

Evidence:
1. src/order/OrderRepository.java
2. application.yml
3. terraform/postgres.tf
4. production deployment

Last verified:
2026-08-22
```

---

# 34. Evidence Model

Evidence types:

```text
Source Code
Configuration
AST
Dependency Files
Database Schema
SQL
Terraform
Kubernetes
Docker
Helm
OpenAPI
CI/CD
Git History
Tests
Logs
Metrics
Traces
Runtime Observation
Existing Documentation
Human Input
Manual Annotation
```

Each evidence item should contain:

```text
Source
Location
Extraction Method
Timestamp
Commit/Version
Reliability
Provenance
```

---

# 35. Confidence Model

Suggested levels:

```text
Very High
High
Medium
Low
Very Low
Unknown
```

Confidence should reflect evidence quality and source agreement.

Example:

```text
Source code
+
configuration
+
deployment
=
High confidence
```

Old documentation alone should normally produce lower confidence.

---

# 36. Facts, Inferences, Assumptions and Recommendations

These categories must remain separate.

## Fact

```text
PostgreSQL dependency is present.
```

## Inference

```text
PostgreSQL is probably the primary transactional store.
```

## Assumption

```text
The legacy batch scheduler is still active in production.
```

## Recommendation

```text
Consider separating shared database ownership.
```

The UI and generated documentation should visually distinguish them.

---

# 37. Unknowns

Unknowns are valid architecture information.

Example:

```text
UNKNOWN

Who owns CustomerDB?

Priority:
HIGH

Missing evidence:
Ownership metadata

Suggested investigation:
Inspect CODEOWNERS and infrastructure repositories.
```

Unknowns feed the RFC 3 investigation loop.

---

# 38. Open Questions

Each question should include:

```text
Question
Priority
Affected Architecture
Why it matters
Missing Evidence
Suggested Investigation
Status
```

Statuses:

```text
Open
Investigating
Resolved
Accepted as Unknown
Blocked
```

---

# 39. Glossary

Glossary categories:

```text
Business Terms
Technical Terms
Domain Concepts
Acronyms
System-specific terminology
```

EKOS may detect terminology inconsistencies.

Example:

```text
Documentation:
Customer

Code:
Client

Database:
Party
```

Potential result:

```text
Potential terminology inconsistency.
Human validation recommended.
```

---

# 40. Appendices

Machine-oriented inventories:

```text
All Services
All APIs
All Databases
All Tables
All Dependencies
All Technologies
All External Systems
All Repositories
All Infrastructure Resources
All Evidence
```

---

# 41. Cross-View Consistency

All views originate from the same AKM.

Example:

```text
System Context
    ↓
Order Management System

Container View
    ↓
Order Service

Deployment View
    ↓
Order Service → AKS

Data View
    ↓
Order Service → PostgreSQL

Technology View
    ↓
PostgreSQL 15
```

Evaluator example:

```text
CROSS-VIEW INCONSISTENCY

Container View:
    OrderService → PostgreSQL

Deployment View:
    OrderService → Oracle

Severity:
    HIGH
```

---

# 42. Architecture Correspondence

Following ISO 42010 principles, EKOS should represent correspondence between models.

Examples:

```text
Container
    ↕
Component

Component
    ↕
Deployment

Service
    ↕
Database

Runtime Scenario
    ↕
Components

Quality Requirement
    ↕
Architecture Mechanism
```

This supports questions such as:

```text
Which deployment supports this service?

Which components implement this capability?

Which database stores this entity?

Which architecture mechanism supports this quality requirement?
```

---

# 43. Quality-to-Architecture Traceability

Example:

```text
Quality Requirement
    ↓
"Order creation < 500ms"
    ↓
Architecture mechanism
    ↓
Synchronous API
    ↓
Order Service
    ↓
PostgreSQL
```

If no supporting mechanism is identified:

```text
QUALITY REQUIREMENT

No supporting architecture mechanism identified.

Investigation required.
```

---

# 44. Architecture-to-Evidence Traceability

Example:

```text
C4 Relationship
OrderService → Kafka
        ↓
Claim
        ↓
Evidence
        ├── KafkaProducer.java
        ├── application.yml
        └── topic configuration
```

---

# 45. Documentation Generation Pipeline

```text
Architecture Knowledge Model
          ↓
Stakeholder / Concern Analysis
          ↓
Viewpoint Selection
          ↓
Model Selection
          ↓
View Generation
          ↓
Narrative Generation
          ↓
Diagram Generation
          ↓
Consistency Validation
          ↓
Documentation Package
```

---

# 46. Architecture Agent Integration

RFC 3 provides the orchestration:

```text
LEGACY SYSTEM
      ↓
COLLECT
      ↓
EVIDENCE
      ↓
ANALYZE
      ↓
REASON
      ↓
ARCHITECTURE KNOWLEDGE MODEL
      ↓
VIEWPOINTS
      ↓
VIEWS
      ↓
DOCUMENTATION
      ↓
EVALUATION
      ↓
DRIFT / GAPS / CONTRADICTIONS
      ↓
INVESTIGATION PLAN
      ↓
TARGETED COLLECTION
      ↺
```

The agent stops when the quality threshold is reached or further investigation has insufficient value.

---

# 47. Documentation Evaluation

Evaluate:

```text
Completeness
Correctness
Consistency
Evidence Coverage
Traceability
Unsupported Claims
Contradictions
Cross-View Consistency
Architecture Coverage
```

Example:

```yaml
evaluation:
  score: 0.91

  completeness:
    score: 0.92

  evidence_coverage:
    score: 0.94

  consistency:
    score: 0.90

  traceability:
    score: 0.96

  critical_issues: 0
  high_priority_issues: 0
```

---

# 48. Documentation Quality Gate

A package is complete when:

```text
quality_score >= configured threshold

AND

critical contradictions = 0

AND

evidence coverage >= configured threshold

AND

no unresolved high-priority issues remain
```

Otherwise:

```text
Evaluation
    ↓
Feedback
    ↓
Investigation
    ↓
Regeneration
```

---

# 49. Local LLM Principle

Where an LLM is required, EKOS should prefer local inference.

Default:

```text
Repository
    ↓
Deterministic extraction
    ↓
Local evidence processing
    ↓
Local LLM reasoning
    ↓
Local AKM
    ↓
Local documentation
```

Supported modes:

```text
local
cloud
hybrid
none
```

Cloud inference must be explicitly enabled.

---

# 50. Deterministic Analysis First

Prefer:

```text
AST
Dependency parsers
SQL parsers
Terraform parsers
Kubernetes parsers
OpenAPI parsers
Git analysis
Configuration parsing
```

LLMs should primarily handle:

```text
Semantic interpretation
Classification
Inference
Architecture reasoning
Summarization
Evaluation
Investigation planning
```

---

# 51. Privacy

Default:

```text
No source code leaves the machine.
```

Offline mode:

```bash
ekos architecture investigate . --offline
```

must guarantee no network calls.

This is especially important for enterprise, proprietary, security-sensitive and regulated systems.

---

# 52. Architecture Documentation Package

Example:

```text
architecture/
│
├── README.md
├── 00-architecture-description.md
├── 01-executive-overview.md
├── 02-system-context.md
├── 03-system-landscape.md
├── 04-container-architecture.md
├── 05-component-architecture/
├── 06-runtime-architecture/
├── 07-deployment-architecture.md
├── 08-data-architecture.md
├── 09-integration-architecture.md
├── 10-security-architecture.md
├── 11-technology-architecture.md
├── 12-quality-architecture.md
├── 13-architecture-decisions/
├── 14-risks-and-technical-debt.md
├── 15-architecture-evolution.md
├── 16-documentation-drift.md
├── 17-traceability.md
├── 18-glossary.md
├── 19-open-questions.md
│
├── diagrams/
│   ├── system-context.svg
│   ├── containers.svg
│   ├── components/
│   ├── runtime/
│   ├── deployment.svg
│   └── data-flow.svg
│
└── appendices/
    ├── services.md
    ├── apis.md
    ├── databases.md
    ├── technologies.md
    └── evidence.md
```

---

# 53. Machine-Readable Companion

```text
.ekos/
└── architecture/
    ├── model.json
    ├── entities.json
    ├── relationships.json
    ├── claims.json
    ├── evidence.json
    ├── viewpoints.json
    ├── views.json
    ├── evaluations.json
    ├── drift.json
    ├── investigations.json
    ├── reviews.json
    ├── baselines/
    └── provenance.json
```

Markdown is the human-facing projection.

The structured model is the machine-facing representation.

---

# 54. Architecture Baseline

A completed description should create a baseline.

```yaml
baseline:
  id: baseline.2026-08-22

  repository_commit:
    abc123

  akm_version:
    2.0

  generated_at:
    2026-08-22T10:00:00Z

  evaluation:
    score: 0.93

  statistics:
    entities: 147
    relationships: 391
    claims: 219
```

---

# 55. Architecture Diff

Future analyses compare structured baselines.

Example:

```text
Architecture Change

Added:
    PaymentService

Removed:
    LegacyPaymentAdapter

Changed:
    OrderService → PaymentService

New dependency:
    PaymentService → PaymentDB
```

Diff must operate on the AKM, not Markdown.

---

# 56. Continuous Architecture Drift

Once a baseline exists:

```text
Documented Architecture
        ↕
Current Evidence
        ↓
Drift Detection
```

Future workflow:

```text
Code change
    ↓
Architecture analysis
    ↓
Architecture diff
    ↓
Documentation drift
    ↓
Documentation update proposal
```

---

# 57. Architecture Q&A

The AKM should eventually support questions.

Example:

> Why does OrderService depend on Kafka?

```text
OrderService publishes OrderCreated events.

Evidence:
    src/order/events.py
    Kafka configuration
    topic definition

Confidence:
    HIGH
```

Other queries:

```text
Which systems depend on CustomerDB?
Where is Customer data stored?
Which components implement Order Management?
What changed since the last baseline?
Which documentation statements are stale?
```

---

# 58. Documentation Targets

The AKM should support multiple targets:

```text
AKM
 │
 ├── arc42
 ├── C4
 ├── Markdown
 ├── HTML
 ├── diagrams
 ├── ADR
 ├── architecture inventory
 └── MCP knowledge interface
```

Future targets may include:

```text
Enterprise architecture repositories
Architecture portals
Wikis
Confluence
GitHub
```

---

# 59. Product Differentiation

EKOS should not be positioned simply as:

> AI-powered architecture documentation generator.

Preferred positioning:

> **EKOS reconstructs and validates software architecture from evidence.**

Extended:

> **Give EKOS a legacy software project. It investigates how the system actually works, reconstructs its architecture, compares multiple sources, detects contradictions and documentation drift, and produces an evidence-backed architecture description.**

Core capability:

```text
Understand
    ↓
Reconstruct
    ↓
Reason
    ↓
Document
    ↓
Evaluate
    ↓
Detect Drift
    ↓
Investigate
    ↓
Update
```

---

# 60. Key Differentiator: Documentation Drift

The strongest practical differentiator is not document generation.

It is:

> **Documentation verified against reality.**

Instead of:

```text
Code → AI → Documentation
```

EKOS provides:

```text
Code
Infrastructure
Data
Runtime
Existing Documentation
        ↓
     Evidence
        ↓
   Architecture Model
        ↓
      Reasoning
        ↓
 Documentation
        ↓
    Evaluation
        ↓
Documentation Drift
        ↓
 Investigation
```

This turns architecture documentation from a static artifact into a continuously validated architecture description.

---

# 61. Recommended MVP

The first implementation should support:

```text
Repository
Source code
Markdown
YAML
JSON
Configuration
Dependencies
Git history
```

Model:

```text
Entities
Relationships
Facts
Inferences
Unknowns
Evidence
Confidence
Provenance
```

Views:

```text
C4 System Context
C4 Container
Basic Component View
Architecture Summary
Technology Inventory
Basic Runtime View
```

Capabilities:

```text
Basic evaluation
Basic documentation drift
Targeted investigation
Local Ollama
Markdown generation
SVG/diagram generation
```

Maximum investigation iterations:

```text
3
```

---

# 62. Phase 2

Add:

```text
Terraform
Kubernetes
OpenAPI
SQL
Data Architecture
Deployment Architecture
Security Architecture
Quality Architecture
Architecture Diff
Architecture Drift
Human Review
ADR generation
MCP
```

---

# 63. Phase 3

Add:

```text
Runtime telemetry
Logs
Metrics
Traces
Continuous drift detection
Architecture Q&A
Target Architecture
Migration Architecture
Architecture fitness checks
Architecture governance
Architecture evolution analysis
```

---

# 64. Evaluation Metrics

EKOS should measure:

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

### Cross-View Consistency

Percentage of relationships consistent across views.

### Drift Detection Accuracy

Percentage of known documentation discrepancies detected.

### Human Correction Rate

Percentage of claims requiring human correction.

### Iteration Efficiency

Architecture quality improvement per investigation iteration.

---

# 65. Final Architecture

```text
                    LEGACY SYSTEM
                         │
       ┌─────────────────┼──────────────────┐
       ↓                 ↓                  ↓
     CODE             INFRA              DOCS
       │                 │                  │
       └─────────────────┼──────────────────┘
                         ↓
                    COLLECTION
                         ↓
                      EVIDENCE
                         ↓
                      ANALYSIS
                         ↓
                      REASONING
                         ↓
             ARCHITECTURE KNOWLEDGE MODEL
                         │
              ┌──────────┼───────────┐
              ↓          ↓           ↓
            FACTS    INFERENCES    UNKNOWN
              │          │           │
              └──────────┼───────────┘
                         ↓
                   ISO 42010 Concepts
                         │
                  Stakeholders
                         ↓
                     Concerns
                         ↓
                    Viewpoints
                         ↓
                       Views
                         ↓
             ┌───────────┼────────────┐
             ↓           ↓            ↓
            C4          arc42       Custom
             │           │            │
             └───────────┼────────────┘
                         ↓
              ARCHITECTURE DOCUMENTATION
                         ↓
                     EVALUATION
                         ↓
             ┌───────────┴───────────┐
             ↓                       ↓
        GOOD ENOUGH              GAPS / DRIFT
             │                       │
             ↓                       ↓
            END              INVESTIGATION PLAN
                                     │
                                     ↓
                                  COLLECT
                                     ↺
```

---

# 66. Final Principles

### 1. Evidence before prose

Architecture claims originate from evidence.

### 2. Model before documents

The AKM is canonical.

### 3. Standards are complementary

```text
ISO 42010 → architecture description
arc42 → documentation structure
C4 → architecture visualization
ISO 25010 → quality terminology
EKOS → evidence, reasoning, evaluation and drift
```

### 4. Facts are not inferences

The system must explicitly distinguish them.

### 5. Unknown is better than hallucination

Missing information must be represented as unknown.

### 6. Documentation must be traceable

Important statements must link to evidence.

### 7. Views must be consistent

All views originate from the same AKM.

### 8. Architecture must be evaluated

Generation without independent evaluation is insufficient.

### 9. Documentation must be compared with reality

Documentation Drift is a first-class concern.

### 10. Architecture reconstruction is iterative

```text
Collect
→ Analyze
→ Reason
→ Generate
→ Evaluate
→ Investigate
→ Collect
```

### 11. Local AI should be preferred

Sensitive architecture information should remain local whenever possible.

### 12. Human decisions remain authoritative

Human-confirmed decisions must not be silently overwritten.

---

# 67. Final Statement

The goal of EKOS Architecture Intelligence is not to generate a large Markdown file.

The goal is to answer, with evidence:

```text
What is this system?

How is it structured?

How does it behave?

Where does it run?

What data does it use?

How does it integrate with other systems?

How is it secured?

What technologies does it depend on?

What quality characteristics matter?

Why was it designed this way?

What risks and technical debt exist?

What has changed?

What does the existing documentation get wrong?

What do we still not know?

What evidence supports each conclusion?
```

The resulting artifact is:

```text
Evidence-backed Architecture Description
+
Architecture Knowledge Model
+
Architecture Evaluation
+
Documentation Drift Analysis
+
Investigation History
```

The central product proposition is:

> **EKOS reconstructs software architecture from evidence, produces standards-aligned architecture descriptions, and continuously verifies those descriptions against the system itself.**

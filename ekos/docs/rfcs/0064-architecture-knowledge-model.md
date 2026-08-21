# RFC 0064 — EKOS Architecture Knowledge Model

**Status:** Superseded by [RFC 0065](0065-architecture-knowledge-model-v2.md)
**Author:** External contribution, filed into this repo's RFC sequence 2026-08-21
**Created:** date unknown (source document undated); filed 2026-08-21

**Original identifier:** EKOS-ARCHMODEL-001 (external doc's own numbering, not this repo's RFC
sequence — kept below for provenance)
**Original Parent RFC referenced:** EKOS-ARCHDOC-001 — "Architecture Documentation Compiler". This
parent document was not provided alongside this file and does not exist elsewhere in this
repository as of filing — an unresolved reference, kept as-is from the source rather than invented.
**Target:** EKOS Architecture Plugin / Knowledge Compiler
**Audience:** EKOS maintainers, software architects, data architects, technical leads

---

# 1. Summary

This RFC defines the **Architecture Knowledge Model (AKM)** that EKOS should build before generating architecture documentation.

The model is the canonical intermediate representation between:

```text
Legacy System
    ↓
Source / Infrastructure / Documentation Analysis
    ↓
Evidence
    ↓
Architecture Knowledge Model
    ↓
Architecture Views
    ↓
Documentation
```

The fundamental principle is:

> **Documentation is a compiled view of an architecture knowledge model, not the primary data structure.**

The model must represent:

- architecture entities;
- relationships;
- systems;
- applications;
- services;
- components;
- infrastructure;
- data;
- integrations;
- runtime behavior;
- deployment;
- technology;
- security observations;
- quality attributes;
- architecture decisions;
- risks;
- technical debt;
- assumptions;
- unknowns;
- contradictions;
- evidence;
- confidence.

The model must also preserve the distinction between:

```text
OBSERVED FACT
INFERENCE
ASSUMPTION
UNKNOWN
RECOMMENDATION
```

This distinction is mandatory.

---

# 2. Motivation

A documentation generator can produce attractive architecture documents while being factually wrong.

For example:

```text
LLM sees:
- old README
- source code
- configuration

LLM generates:

"System uses Oracle as its primary database."
```

But the actual system may now use PostgreSQL.

A better architecture pipeline is:

```text
Evidence
   ↓
Facts
   ↓
Relationships
   ↓
Architecture Model
   ↓
Reasoning
   ↓
Documentation
```

This allows EKOS to answer:

> "Why does the generated architecture say PostgreSQL?"

with:

```text
application.yml
database dependency
Terraform resource
SQL migrations
source repository
```

The Architecture Knowledge Model therefore becomes the **source of truth for generated architecture documentation**.

---

# 3. Design Principles

## 3.1 Evidence First

Every important architectural claim should have one or more evidence references.

No evidence:

```text
UNKNOWN
```

not:

```text
FACT
```

---

## 3.2 Current State and Target State Must Be Separate

The model must distinguish:

```text
CURRENT
TARGET
PROPOSED
HISTORICAL
```

A proposed architecture must never silently overwrite the observed current architecture.

---

## 3.3 Facts and Inferences Must Be Separate

Example:

```yaml
fact:
  application: OrderService
  database: PostgreSQL
```

versus:

```yaml
inference:
  statement: PostgreSQL is the system of record for orders
  confidence: medium
```

---

## 3.4 Human Review Is Part of the Model

The model must support:

```text
UNREVIEWED
REVIEWED
CONFIRMED
REJECTED
```

An architect should be able to correct EKOS.

Human corrections become valuable knowledge and must not be lost during regeneration.

---

# 3.5 Local LLM First

Where an LLM is required, EKOS should prefer a **local LLM by default whenever the local model can perform the task adequately**.

This is especially important for architecture analysis because source repositories frequently contain:

- proprietary source code;
- infrastructure definitions;
- internal APIs;
- database schemas;
- security configuration;
- business logic;
- sensitive documentation.

The preferred execution model is:

```text
                 +------------------+
                 | Deterministic    |
                 | Static Analysis  |
                 +--------+---------+
                          |
                          v
                 Evidence Extraction
                          |
                          v
                 +------------------+
                 | Local LLM        |
                 | preferred        |
                 +--------+---------+
                          |
                  Structured inference
                          |
                          v
                 Architecture Model
```

External/cloud LLMs should be an explicit option, not an implicit dependency.

Possible modes:

```text
--llm local
--llm cloud
--llm hybrid
--llm none
```

### Local mode

Use:

- Ollama;
- llama.cpp;
- compatible local inference servers;
- future local inference backends.

### Cloud mode

Use an explicitly configured provider.

### Hybrid mode

Keep sensitive source material local and send only selected normalized context to a cloud model.

Example:

```text
Source code
    ↓
Local extraction
    ↓
Local redaction
    ↓
Structured architecture facts
    ↓
Optional cloud reasoning
```

The user must know when data leaves the local environment.

---

# 3.6 Deterministic Analysis Before LLM Reasoning

EKOS should not ask an LLM to discover facts that can be obtained deterministically.

Prefer:

```text
AST
dependency parser
configuration parser
Terraform parser
Kubernetes parser
SQL parser
OpenAPI parser
Git history
```

before:

```text
LLM inference
```

Example:

Do not ask:

> "Which libraries does this Python application use?"

if `requirements.txt` and imports can answer the question directly.

Use the LLM for:

- semantic classification;
- architectural interpretation;
- summarization;
- relationship inference;
- naming;
- explanation;
- ambiguity resolution;
- architecture questions.

---

# 4. Model Architecture

The AKM consists of several layers.

```text
+--------------------------------------------------+
| Documentation Views                              |
+--------------------------------------------------+
| Architecture Reasoning                           |
+--------------------------------------------------+
| Architecture Knowledge Model                    |
|                                                  |
| Entities | Relationships | Decisions | Risks    |
+--------------------------------------------------+
| Evidence Layer                                   |
+--------------------------------------------------+
| Extraction Layer                                 |
| AST | Config | Infra | DB | API | Git | LLM    |
+--------------------------------------------------+
| Source System                                    |
+--------------------------------------------------+
```

---

# 5. Entity Model

Every architecture entity should have a stable identifier.

Example:

```yaml
id: service.order
type: service
name: Order Service
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

# 6. Entity Schema

Conceptual schema:

```yaml
entity:
  id: string
  type: enum
  name: string

  description:
    text: string

  state:
    lifecycle: current | historical | proposed | deprecated
    environment: dev | test | staging | production | unknown

  ownership:
    team: string | null
    organization: string | null

  technology:
    name: string | null
    version: string | null

  classification:
    domain: string | null
    criticality: low | medium | high | critical | unknown

  evidence:
    - evidence_id

  confidence:
    level: high | medium | low | unknown
    score: float | null

  review:
    status: unreviewed | reviewed | confirmed | rejected
    reviewer: string | null
    comment: string | null
```

---

# 7. Relationship Model

Architecture is primarily about relationships.

A relationship should therefore be a first-class object.

Example:

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
    - evd.terraform.003

  confidence:
    level: high
    score: 0.96
```

---

# 8. Relationship Types

Initial vocabulary:

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

The vocabulary should be extensible.

---

# 9. Evidence Model

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

# 10. Evidence Reliability

Not all evidence is equal.

Suggested defaults:

```text
runtime observation       very high
database schema           very high
deployment configuration  very high
source code               high
API specification         high
infrastructure code       high
tests                     medium/high
recent documentation      medium
old documentation         low/medium
LLM inference             low unless supported
```

These are defaults, not absolute rules.

The system should allow source-specific reliability policies.

---

# 11. Claims

A claim represents an architectural statement.

Example:

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
    fact | inference | assumption | recommendation

  evidence:
    - evd.source.001
    - evd.config.003

  confidence:
    level: high

  review:
    status: unreviewed
```

Claims allow EKOS to separate:

```text
What we know
```

from:

```text
What we think
```

---

# 12. Fact Model

A fact is directly supported by evidence.

Example:

```yaml
fact:
  subject: service.order
  predicate: uses
  object: technology.postgresql

  evidence:
    - evd.config.001
```

Facts should ideally be generated by deterministic extractors.

---

# 13. Inference Model

Inference represents reasoning from evidence.

Example:

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

The inference must preserve its supporting facts.

---

# 14. Assumption Model

Example:

```yaml
assumption:
  statement:
    "The legacy batch scheduler is still active in production."

  reason:
    "Configuration exists, but no runtime evidence was available."

  confidence:
    level: low
```

Assumptions must appear in:

```text
Open Questions
```

and should not be represented as facts.

---

# 15. Unknown Model

Unknown is important.

Example:

```yaml
unknown:
  question:
    "Who owns the Customer database?"

  affected_entity:
    database.customer

  priority:
    high
```

A professional architecture document should expose meaningful unknowns.

---

# 16. Contradiction Model

Example:

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

Possible resolution states:

```text
unresolved
resolved
accepted
rejected
obsolete
```

---

# 17. Architecture Views

Views are projections over the model.

A view must not contain independent architectural truth.

Examples:

```text
System Context View
Container View
Component View
Runtime View
Deployment View
Data View
Security View
Technology View
Integration View
Dependency View
```

Each view is generated from entities and relationships.

---

# 18. View Definition

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

This allows the same architecture model to produce multiple outputs.

---

# 19. C4 Mapping

Mapping:

```text
AKM Entity                    C4

system                        System
application/service            Container
component/module              Component
code element                   Code
external_system               External System
actor                         Person
```

The mapping must be configurable.

---

# 20. Runtime Scenario Model

Runtime behavior needs its own model.

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

Each step should have evidence where available.

---

# 21. Deployment Model

```yaml
deployment:
  id: deployment.production

  environment: production

  nodes:
    - kubernetes.cluster

  deployments:
    - service.order
```

Relationships:

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

# 22. Data Model

Data architecture should be represented independently from application architecture.

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

# 23. Technology Model

```yaml
technology:
  id: tech.postgresql
  name: PostgreSQL
  version: "16"

  used_by:
    - service.order

  lifecycle:
    status: supported | unknown | deprecated | end_of_life

  evidence:
    - evd.docker.001
```

Lifecycle status should only be asserted when reliable information is available.

---

# 24. Security Model

Security observations should be modeled separately.

Examples:

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
    - evd.config.002
```

Unknown security properties should become questions.

---

# 25. Quality Attribute Model

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

Do not convert this into:

```text
Availability = 99.99%
```

unless that requirement or measurement actually exists.

---

# 26. Architecture Decision Model

```yaml
decision:
  id: adr.kafka

  title:
    "Kafka is used for asynchronous integration."

  status:
    observed

  context:
    null

  decision:
    "Kafka is currently used for event transport."

  consequences:
    unknown

  evidence:
    - evd.kafka.config
    - evd.kafka.consumer
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

# 27. Risk Model

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

The plugin should produce **risk candidates**, not pretend to know the business impact.

---

# 28. Technical Debt Model

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

  remediation:
    status: unknown
```

---

# 29. Human Review Model

Human feedback must be stored separately from generated content.

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

The compiler must preserve these decisions across regeneration.

---

# 30. Architecture Baseline

An architecture baseline is an immutable snapshot.

```yaml
baseline:
  id: baseline.2026-08-21

  source_commit:
    abc123

  generated_at:
    2026-08-21T13:00:00Z

  model_version:
    1.0

  statistics:
    entities: 143
    relationships: 387
    claims: 219
```

This enables architecture diffing.

---

# 31. Architecture Diff

Given:

```text
Baseline A
Baseline B
```

EKOS should produce:

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

Diff should operate on the structured model rather than Markdown text.

---

# 32. Architecture Drift

Drift is a comparison between:

```text
documented model
```

and:

```text
observed model
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

---

# 33. Local LLM Architecture

The LLM layer should be abstracted.

```text
LLMProvider
    |
    +-- LocalOllamaProvider
    +-- LocalLlamaCppProvider
    +-- CloudProvider
```

Example configuration:

```yaml
llm:
  mode: local

  provider: ollama

  model:
    name: qwen3
    endpoint: http://localhost:11434

  temperature: 0.1
```

The exact model should remain configurable.

---

# 34. Local LLM Task Classification

Not every task needs an LLM.

Recommended classification:

```text
Task                               Preferred implementation

File discovery                     deterministic
AST parsing                        deterministic
Dependency extraction              deterministic
Terraform parsing                  deterministic
Kubernetes parsing                 deterministic
SQL schema extraction              deterministic

Entity naming                      local LLM
Semantic classification             local LLM
Architecture summarization          local LLM
Relationship inference              local LLM
Documentation generation            local LLM
Risk candidate generation           local LLM + rules
Contradiction explanation           local LLM
Architecture Q&A                    local LLM
```

---

# 35. Privacy Policy

Default:

```text
No source code leaves the machine.
```

Cloud inference must require explicit configuration.

Example:

```bash
ekos architecture analyze . --llm cloud
```

EKOS should display:

```text
WARNING:
Cloud LLM selected.

Repository content may be transmitted
to the configured provider.

Continue? [y/N]
```

For automated environments, this behavior should be configurable.

---

# 36. Hybrid LLM Mode

Hybrid mode should minimize data transmission.

Pipeline:

```text
Repository
    ↓
Local static analysis
    ↓
Local evidence extraction
    ↓
Secret removal
    ↓
Context minimization
    ↓
Cloud LLM
    ↓
Structured inference
    ↓
Local Architecture Model
```

For example, instead of sending:

```text
5000 source files
```

send:

```text
service graph
API metadata
dependency graph
selected source snippets
configuration facts
```

---

# 37. LLM Output Contract

LLM output should never be accepted directly as Markdown.

Preferred:

```text
LLM
 ↓
JSON schema
 ↓
Validation
 ↓
Evidence linking
 ↓
Architecture Model
 ↓
Markdown generator
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

Invalid output must be rejected.

---

# 38. Provenance

Every generated object should retain provenance.

Example:

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

This makes the system auditable.

---

# 39. Prompt Versioning

Prompts must be treated as versioned artifacts.

```text
architecture-entity-classification-v1
architecture-relationship-inference-v2
architecture-summary-v1
architecture-risk-analysis-v1
```

Changing a prompt may change the architecture model.

Therefore the model should record:

```text
extractor version
prompt version
LLM model
```

---

# 40. Model Versioning

The AKM schema itself must be versioned.

Example:

```text
AKM v1.0
AKM v1.1
AKM v2.0
```

Migration mechanisms will eventually be required.

---

# 41. Storage

The first implementation may use:

```text
JSON
```

for portability.

Recommended structure:

```text
.ekos/
└── architecture/
    ├── model.json
    ├── entities.json
    ├── relationships.json
    ├── claims.json
    ├── evidence.json
    ├── reviews.json
    ├── baselines/
    └── provenance.json
```

Future storage could use a graph database or embedded graph engine.

---

# 42. Graph Representation

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

Claims and evidence should be attached to relationships.

```text
OrderService
   |
   | writes_to
   |
PostgreSQL
   |
   +-- Evidence:
       application.yml
       repository.py
       terraform/
```

---

# 43. Why the Graph Matters

A flat document cannot efficiently answer:

```text
What depends on this database?

What will break if this service is removed?

Which systems consume this event?

Which data entities cross the security boundary?

Which services share infrastructure?

Which components have the highest dependency centrality?
```

The architecture graph can.

---

# 44. Query Layer

The model should support semantic architecture queries.

Examples:

```text
find all services depending on database.customer

find all external systems connected to system.orders

find all paths between ERP and Power BI

find components deployed in production

find all undocumented integrations

find all low-confidence relationships

find all architecture drift
```

---

# 45. MCP Interface

The AKM should expose architecture knowledge through MCP.

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
architecture_ask
```

---

# 46. Example MCP Interaction

User:

> Why does OrderService depend on Kafka?

EKOS:

```text
OrderService publishes OrderCreated events.

Evidence:

1. src/order/events.py:42-71
2. kafka configuration
3. Kafka producer dependency

Confidence:
HIGH

Architecture relationship:
OrderService --publishes--> Kafka topic: orders.created
```

---

# 47. Documentation Compilation

The documentation compiler consumes the AKM.

```text
AKM
 |
 +--> arc42
 |
 +--> C4
 |
 +--> Data Architecture
 |
 +--> Deployment Architecture
 |
 +--> Security View
 |
 +--> ADR
 |
 +--> Risk Report
 |
 +--> Drift Report
```

The model is therefore presentation-independent.

---

# 48. Example End-to-End Flow

```text
git clone legacy-project

        ↓

ekos architecture analyze .

        ↓

Deterministic extraction

        ↓

Evidence graph

        ↓

Local LLM semantic analysis

        ↓

Architecture Knowledge Model

        ↓

Validation

        ↓

Human review

        ↓

Architecture baseline

        ↓

ekos architecture generate

        ↓

Professional documentation
```

---

# 49. CLI Proposal

Initial commands:

```bash
ekos architecture analyze <path>

ekos architecture inspect

ekos architecture generate

ekos architecture generate --view context

ekos architecture generate --view deployment

ekos architecture generate --view data

ekos architecture diff <baseline-a> <baseline-b>

ekos architecture drift

ekos architecture questions

ekos architecture evidence

ekos architecture ask "<question>"
```

---

# 50. Plugin Boundary

The plugin should be separated into:

```text
ekos-architecture
│
├── extractors
│   ├── source
│   ├── config
│   ├── terraform
│   ├── kubernetes
│   ├── sql
│   └── api
│
├── evidence
│
├── model
│
├── inference
│
├── validation
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

# 51. Extractor Contract

Every extractor should produce normalized evidence.

Conceptually:

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

Extractors should not directly generate final documentation.

---

# 52. Inference Contract

Inference operates over evidence.

```rust
trait ArchitectureInferenceEngine {
    fn infer(
        &self,
        evidence: &[Evidence],
    ) -> Result<Vec<Claim>>;
}
```

LLM-based and rule-based inference can implement the same interface.

---

# 53. Validation Contract

```rust
trait ArchitectureValidator {
    fn validate(
        &self,
        model: &ArchitectureModel,
    ) -> ValidationReport;
}
```

Validation should detect:

- orphan entities;
- broken relationships;
- contradictory claims;
- missing evidence;
- invalid references;
- unsupported states.

---

# 54. Documentation Contract

```rust
trait ArchitectureViewGenerator {
    fn generate(
        &self,
        model: &ArchitectureModel,
    ) -> Result<ArchitectureView>;
}
```

Then:

```rust
trait DocumentationRenderer {
    fn render(
        &self,
        view: &ArchitectureView,
    ) -> Result<RenderedDocument>;
}
```

---

# 55. Quality Gates

Before generating final documentation:

```text
[✓] model schema valid
[✓] entity references valid
[✓] relationships valid
[✓] evidence references valid
[✓] confidence present
[✓] no unclassified LLM claims
[✓] diagrams valid
[✓] no secrets detected
[✓] provenance recorded
```

If critical validation fails:

```text
Architecture generation blocked.
```

---

# 56. Security Requirements

The plugin must:

- detect secrets before LLM processing;
- avoid transmitting source code by default;
- support fully offline execution;
- make cloud inference explicit;
- record LLM provider and model;
- allow users to disable LLM inference;
- never include secrets in generated documentation.

---

# 57. Offline Mode

EKOS should support:

```bash
ekos architecture analyze . --offline
```

This mode must guarantee:

```text
No network calls.
```

Potential implementation:

```text
static analyzers
+
local model
+
local storage
+
local documentation generator
```

This is potentially a major differentiator for enterprise users.

---

# 58. Evaluation Dataset

The project should maintain representative test repositories.

Categories:

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

Each test repository should have a manually reviewed expected architecture model.

---

# 59. Evaluation Metrics

### Entity precision

How many discovered entities are correct?

### Entity recall

How many important entities were discovered?

### Relationship precision

How many generated relationships are correct?

### Evidence coverage

What percentage of important claims have evidence?

### Hallucination rate

How many claims lack sufficient evidence?

### Drift detection accuracy

How many known documentation discrepancies are detected?

### Human correction rate

How many generated claims require architect correction?

### Local-vs-cloud quality

Compare model quality between local and cloud LLM configurations.

---

# 60. MVP Scope

The first implementation of the Architecture Knowledge Model should support:

```text
Source repository
Markdown documentation
Configuration
Git metadata
Basic dependency extraction
Evidence
Entities
Relationships
Facts
Inferences
Unknowns
Confidence
Human review
JSON storage
C4 Context
C4 Container
Basic Runtime
Technology View
Markdown output
Local LLM
```

Do NOT implement the full enterprise architecture platform in MVP-1.

---

# 61. Phase 2

Add:

```text
Terraform
Kubernetes
OpenAPI
SQL
Data architecture
Deployment model
Security observations
Documentation drift
Architecture diff
Baselines
MCP
```

---

# 62. Phase 3

Add:

```text
Runtime telemetry
Logs
Metrics
Traces
Architecture governance
Continuous drift detection
Architecture Q&A agent
Target architecture
Migration architecture
ADR generation
Architecture fitness checks
```

---

# 63. Example Model

A simplified example:

```yaml
entities:

  - id: system.orders
    type: system
    name: Order Management System

  - id: service.order
    type: service
    name: Order Service

  - id: database.orders
    type: database
    name: PostgreSQL Orders DB

  - id: queue.orders
    type: topic
    name: orders.created

relationships:

  - id: rel.001
    type: contains
    source: system.orders
    target: service.order

  - id: rel.002
    type: writes_to
    source: service.order
    target: database.orders

  - id: rel.003
    type: publishes
    source: service.order
    target: queue.orders

claims:

  - id: claim.001
    type: fact
    relationship: rel.002
    confidence:
      level: high
    evidence:
      - evd.001

  - id: claim.002
    type: inference
    statement:
      "orders.created is a domain event."
    confidence:
      level: medium
    evidence:
      - evd.002
```

---

# 64. What the Model Must Not Become

The AKM must not become:

```text
a giant LLM-generated JSON document
```

Nor:

```text
a copy of the source code AST
```

Nor:

```text
a copy of the Markdown documentation
```

It should represent:

> **Architecture-level knowledge derived from multiple sources.**

---

# 65. Relationship to EKOS Core

The architecture plugin should reuse EKOS core capabilities wherever possible.

Conceptually:

```text
EKOS Core
│
├── Knowledge extraction
├── Knowledge graph
├── Evidence
├── provenance
├── MCP
└── plugin framework
        │
        v
EKOS Architecture
│
├── Architecture entities
├── Architecture relationships
├── Architecture inference
├── C4 views
├── arc42 views
├── Deployment views
├── Data views
└── Architecture documentation
```

Architecture-specific semantics belong in the plugin.

Generic knowledge compilation belongs in EKOS core.

---

# 66. Product Positioning

The plugin should not be positioned as:

> "AI documentation generator."

That is too generic.

Better:

> **EKOS Architecture Intelligence reconstructs the architecture of legacy systems from code, infrastructure and documentation, then compiles an evidence-backed architecture baseline.**

Even stronger:

> **Give EKOS a legacy repository. Get an evidence-backed architecture map, professional documentation, and a list of what the old documentation got wrong.**

---

# 67. Final Principle

The Architecture Knowledge Model is the foundation of the entire feature.

The complete architecture should be:

```text
                  LEGACY SYSTEM
                       |
          +------------+------------+
          |            |            |
        CODE       INFRASTRUCTURE   DOCS
          |            |            |
          +------------+------------+
                       |
                       v
               EVIDENCE GRAPH
                       |
                       v
              ARCHITECTURE MODEL
                       |
          +------------+------------+
          |            |            |
        FACTS      INFERENCES    UNKNOWNS
          |            |            |
          +------------+------------+
                       |
                       v
                HUMAN REVIEW
                       |
                       v
              ARCHITECTURE BASELINE
                       |
       +---------------+---------------+
       |               |               |
       v               v               v
      C4             arc42       Custom Views
       |               |               |
       +---------------+---------------+
                       |
                       v
            PROFESSIONAL DOCUMENTATION
```

The critical property is **traceability**:

```text
Document statement
       ↓
Architecture claim
       ↓
Architecture relationship
       ↓
Evidence
       ↓
Original source
```

And the critical operational property is **privacy**:

```text
Sensitive repository
       ↓
Local deterministic analysis
       ↓
Local LLM where possible
       ↓
Local Architecture Model
       ↓
Local documentation
```

Only explicitly selected data should leave the developer's environment.

This makes the Architecture Knowledge Model not merely an implementation detail, but the **canonical knowledge layer that allows EKOS to evolve from a documentation generator into an Architecture Intelligence platform**.

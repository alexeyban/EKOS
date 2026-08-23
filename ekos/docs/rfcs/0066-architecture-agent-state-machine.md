# RFC 0066 — EKOS Architecture Agent State Machine & Investigation Orchestrator

**Status:** Proposed
**Author:** External contribution, filed into this repo's RFC sequence 2026-08-21
**Created:** date unknown (source document undated); filed 2026-08-21
**Depends on:** [RFC 0065](0065-architecture-knowledge-model-v2.md)

**Original identifier:** EKOS-ARCHAGENT-003 (external doc's own numbering, not this repo's RFC
sequence — kept below for provenance)
**Target:** EKOS Architecture Plugin / Architecture Agent
**Audience:** EKOS maintainers, software architects, data architects, AI/agent engineers

---

## MVP Agent implemented (2026-08-22) — RFC 0067

`Status` above stays `Proposed` — the full state machine (persistent checkpointing §51,
concurrency-safety infrastructure §53-54, CI/CD exit-code matrix + PR-comment workflow §49-50,
human review, MCP additions) is not built. What shipped is exactly this RFC's own §64-65 "MVP
Agent"/"MVP Investigation Loop": `ekos architecture investigate`
(`crates/cli/src/commands/architecture.rs`) — one orchestrating async function, not a generic
state-machine framework (this MVP runs one investigation at a time, nothing to checkpoint or
coordinate concurrently), composing the existing pipeline stages (`build`/`recover`/`compile`/
`commit`/`docs generate`) plus RFC 0065 Phase 2/3's reasoning pass and evaluator around the
12-step, max-3-iteration MVP loop §65 itself defines. Full design rationale in RFC 0067; live
verification (real local Ollama model, this repo's own workspace) in `devlogs/devlog_71.md`.

---

# 1. Summary

This RFC defines the runtime behavior of the **EKOS Architecture Agent**.

The agent is responsible for executing the iterative architecture intelligence loop defined in RFC 2:

```text
COLLECT
   ↓
ANALYZE
   ↓
REASON
   ↓
UPDATE KNOWLEDGE MODEL
   ↓
GENERATE
   ↓
EVALUATE
   ↓
FEEDBACK
   ↓
INVESTIGATE
   ↓
TARGETED COLLECT
   ↺
```

The Architecture Agent is not a conversational chatbot and not simply an LLM wrapper.

It is an **orchestrator for evidence-driven architecture investigation**.

Its primary responsibility is to decide:

1. what is already known;
2. what is unknown;
3. what evidence is required;
4. which tool or extractor can obtain that evidence;
5. when reasoning should occur;
6. when documentation should be generated;
7. when generated architecture should be evaluated;
8. whether additional investigation is justified;
9. when the investigation should stop.

The agent must preserve the Architecture Knowledge Model across iterations.

---

# 2. Motivation

A legacy architecture reconstruction task cannot reliably be solved with one LLM prompt.

A typical repository may contain:

```text
source code
configuration
SQL
Terraform
Kubernetes
CI/CD
API specifications
tests
Git history
old documentation
```

The first analysis will inevitably leave gaps.

For example:

```text
Iteration 1:

"OrderService uses Kafka."

Confidence: Medium
```

The evaluator may determine:

```text
Evidence is insufficient.
```

The agent should then decide:

```text
Inspect:
    src/order/
    application.yml
    deployment/
```

After collecting additional evidence:

```text
Iteration 2:

"OrderService publishes OrderCreated to Kafka."

Confidence: High
```

The architecture model improves through investigation.

This requires an explicit state machine.

---

# 3. Goals

The agent MUST:

- execute the architecture investigation loop;
- maintain persistent investigation state;
- preserve evidence provenance;
- distinguish facts from inferences;
- create and execute investigation tasks;
- select appropriate tools;
- support local LLMs;
- support deterministic tools;
- support iterative evaluation;
- avoid unnecessary repository rescans;
- respect human-confirmed decisions;
- detect stopping conditions;
- prevent infinite loops;
- expose progress and reasoning outcomes;
- produce a final investigation report.

---

# 4. Non-Goals

The agent MUST NOT:

- modify production infrastructure;
- deploy applications;
- automatically change source code;
- silently upload source code to cloud services;
- treat LLM output as authoritative architecture truth;
- generate unsupported quantitative claims;
- overwrite human-confirmed architecture decisions;
- endlessly retry failed investigations.

---

# 5. Agent Architecture

```text
┌──────────────────────────────────────────────────────────┐
│                 EKOS ARCHITECTURE AGENT                  │
│                                                          │
│  ┌──────────────┐                                        │
│  │ State Machine│                                        │
│  └──────┬───────┘                                        │
│         │                                                │
│         ├──────────────┐                                 │
│         ↓              ↓                                 │
│  ┌────────────┐  ┌──────────────┐                       │
│  │ Tool Router│  │ Policy Engine│                       │
│  └─────┬──────┘  └──────────────┘                       │
│        │                                                 │
│        ├── Extractors                                    │
│        ├── Analyzers                                     │
│        ├── LLM Providers                                 │
│        ├── Generators                                    │
│        └── Evaluators                                    │
│                                                          │
│                    ↓                                     │
│            Architecture Knowledge Model                  │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

---

# 6. Core State Machine

The primary states are:

```text
INITIALIZING
COLLECTING
ANALYZING
REASONING
UPDATING_MODEL
GENERATING
EVALUATING
PLANNING_INVESTIGATION
INVESTIGATING
COMPLETED
FAILED
```

Optional states:

```text
WAITING_FOR_HUMAN
PAUSED
CANCELLED
```

---

# 7. State Diagram

```text
                         ┌──────────────┐
                         │ INITIALIZING │
                         └──────┬───────┘
                                ↓
                         ┌──────────────┐
                         │  COLLECTING  │
                         └──────┬───────┘
                                ↓
                         ┌──────────────┐
                         │   ANALYZING  │
                         └──────┬───────┘
                                ↓
                         ┌──────────────┐
                         │   REASONING  │
                         └──────┬───────┘
                                ↓
                       ┌──────────────────┐
                       │  UPDATING_MODEL  │
                       └────────┬─────────┘
                                ↓
                         ┌──────────────┐
                         │   GENERATING  │
                         └──────┬───────┘
                                ↓
                         ┌──────────────┐
                         │  EVALUATING  │
                         └──────┬───────┘
                                ↓
                         ┌──────────────┐
                         │   DECISION   │
                         └──────┬───────┘
                                │
                ┌───────────────┼────────────────┐
                ↓               ↓                ↓
             COMPLETE       INVESTIGATE         FAIL
                │               │                │
                ↓               ↓                ↓
             COMPLETED    PLAN_INVESTIGATION   FAILED
                                ↓
                         INVESTIGATING
                                ↓
                            COLLECTING
                                ↺
```

---

# 8. State: INITIALIZING

Purpose:

Prepare the investigation.

Tasks:

```text
load configuration
initialize storage
detect repository
detect technologies
load previous AKM if available
load previous baseline if available
load human reviews
initialize LLM provider
initialize extractors
initialize evaluator
initialize policies
```

Example:

```yaml
initialization:
  repository: ./legacy-system

  llm:
    mode: local
    provider: ollama
    model: qwen3

  policy:
    max_iterations: 5
    quality_threshold: 0.90
```

---

# 9. State: COLLECTING

The agent obtains evidence.

Two modes exist.

## Broad collection

Used during initial investigation.

```text
repository
source code
configuration
dependencies
documentation
infrastructure
```

## Targeted collection

Used after evaluation feedback.

```text
specific files
specific directories
specific technology
specific service
specific relationship
specific question
```

The agent should prefer targeted collection after the first iteration.

---

# 10. Collection Decision

The agent should ask:

```text
What information is needed?

Where can it be found?

Which extractor can obtain it?

Is it already present in the Evidence Store?
```

Example:

```yaml
investigation_task:
  question: "Does PaymentService consume Kafka events?"

  target:
    service: payment

  evidence_required:
    - source_code
    - configuration

  preferred_extractors:
    - python
    - yaml
```

---

# 11. State: ANALYZING

Analysis converts raw evidence into structured observations.

Example:

```text
source code
    ↓
AST
    ↓
imports
calls
classes
modules
configuration references
```

The analysis layer should be primarily deterministic.

LLMs should not be required for basic structural analysis.

---

# 12. State: REASONING

Reasoning combines:

```text
new evidence
existing facts
existing relationships
existing claims
existing contradictions
human decisions
```

The agent may use:

```text
rules
graph algorithms
local LLM
cloud LLM
```

The reasoning engine produces structured claims.

Example:

```yaml
claim:
  type: inference

  statement:
    "PaymentService consumes order events."

  evidence:
    - evd.kafka.consumer
    - evd.payment.config

  confidence:
    level: high
```

---

# 13. Reasoning Must Be Incremental

The agent must not rebuild all reasoning from scratch unless explicitly requested.

Preferred:

```text
existing AKM
+
new evidence
+
affected subgraph
↓
incremental reasoning
```

This makes large repositories practical.

---

# 14. State: UPDATING_MODEL

The agent commits validated facts and inferences into the AKM.

Before committing:

```text
schema validation
reference validation
evidence validation
confidence validation
human review protection
contradiction detection
```

Invalid LLM output must not enter the canonical model.

---

# 15. Human-Confirmed Knowledge

If a human has confirmed:

```yaml
review:
  status: confirmed
```

the agent MUST NOT silently replace it.

If new evidence conflicts with it:

```text
Human decision
       +
New evidence
       ↓
Contradiction
       ↓
Human review required
```

---

# 16. State: GENERATING

The agent selects required documentation views.

Example:

```text
C4 Context
C4 Container
Deployment
Data Flow
Integration
Technology
Security
Architecture Summary
```

The generator reads the AKM.

It does not independently discover architecture.

---

# 17. State: EVALUATING

The evaluator reviews:

```text
AKM
generated documents
diagrams
evidence coverage
cross-view consistency
```

Evaluation must be independent from generation.

The evaluator should ideally use a separate prompt, model context, or evaluation strategy.

---

# 18. Evaluation Dimensions

Minimum dimensions:

```text
completeness
correctness
consistency
evidence coverage
traceability
unsupported claims
contradictions
cross-view consistency
```

Example:

```yaml
evaluation:
  score: 0.84

  issues:
    - severity: high
      type: missing_relationship

    - severity: medium
      type: unsupported_claim

    - severity: high
      type: contradiction
```

---

# 19. State: DECISION

The orchestrator decides what happens next.

Possible outcomes:

```text
COMPLETE
INVESTIGATE
WAIT_FOR_HUMAN
FAIL
```

Decision logic:

```text
IF critical contradiction exists
    → WAIT_FOR_HUMAN

ELSE IF quality >= threshold
    → COMPLETE

ELSE IF high-priority unknowns exist
    → INVESTIGATE

ELSE IF meaningful investigation is possible
    → INVESTIGATE

ELSE
    → COMPLETE_WITH_WARNINGS
```

---

# 20. State: PLANNING_INVESTIGATION

The agent converts evaluation issues into executable investigation tasks.

Example:

```yaml
tasks:

  - id: investigation.001

    question:
      "Who owns CustomerDB?"

    priority:
      high

    target:
      database.customer

    actions:
      - inspect terraform
      - inspect repository ownership
      - inspect deployment metadata

    expected_evidence:
      - infrastructure
      - ownership
      - documentation
```

---

# 21. Investigation Priority

Recommended priority:

```text
CRITICAL
HIGH
MEDIUM
LOW
```

The planner should prioritize tasks by:

```text
architectural impact
uncertainty
evidence availability
cost
dependency impact
```

A useful priority score:

```text
priority =
    impact
    × uncertainty
    × evidence_value
    ÷ estimated_cost
```

---

# 22. State: INVESTIGATING

The agent executes investigation tasks.

For each task:

```text
select tool
collect evidence
validate result
store evidence
mark task progress
```

Example:

```text
Task:
"Inspect PaymentService Kafka consumers."

Tool selection:
Python source extractor
YAML configuration extractor

Result:
Kafka consumer discovered.

Task:
completed
```

---

# 23. Tool Router

The Tool Router maps questions to tools.

```text
Question
   ↓
Tool Router
   ↓
┌─────────────────────────────┐
│ Source extractor            │
│ SQL parser                  │
│ Terraform parser            │
│ Kubernetes parser           │
│ OpenAPI parser              │
│ Git analyzer                │
│ Documentation search        │
│ Runtime collector           │
│ Local LLM                   │
└─────────────────────────────┘
```

The LLM should not always be the first tool.

---

# 24. Tool Selection Policy

Example:

```text
Question:
"What services call CustomerService?"

Preferred:

1. static source analysis
2. API definitions
3. configuration
4. runtime telemetry
5. LLM reasoning
```

Another example:

```text
Question:
"What business capability does CustomerService provide?"

Preferred:

1. existing domain metadata
2. documentation
3. source semantics
4. local LLM
```

---

# 25. Local LLM Decision Policy

The agent should use a local LLM when:

```text
semantic interpretation is required
AND
local model is available
AND
task quality is acceptable
```

Example:

```yaml
llm_policy:
  default: local

  allow_cloud: false

  fallback:
    enabled: false
```

Cloud fallback must be explicit.

---

# 26. LLM Escalation

Optional policy:

```text
local LLM
   ↓
confidence insufficient
   ↓
retry with additional context
   ↓
still insufficient
   ↓
optional cloud LLM
```

Cloud escalation must never happen silently.

---

# 27. Agent Memory

The agent has three kinds of memory.

## Persistent Knowledge

```text
Architecture Knowledge Model
```

## Investigation Memory

```text
completed tasks
failed tasks
questions
evaluation history
iterations
tool results
```

## Execution Memory

```text
current state
current task
current context
temporary LLM context
```

---

# 28. Investigation State

Example:

```yaml
investigation:
  id: investigation.2026-08-21

  state: evaluating

  iteration: 3

  statistics:
    evidence: 412
    entities: 143
    relationships: 387
    claims: 219

  quality:
    score: 0.91

  tasks:
    completed: 17
    pending: 2
    failed: 1
```

---

# 29. Iteration Model

An iteration is:

```text
collection
→ analysis
→ reasoning
→ model update
→ generation
→ evaluation
→ decision
```

Example:

```yaml
iteration:
  number: 3

  input:
    tasks:
      - investigation.014
      - investigation.015

  output:
    evidence_added: 43
    claims_added: 17
    relationships_added: 29

  evaluation:
    before: 0.84
    after: 0.91
```

---

# 30. Progress Tracking

The agent should expose progress.

Example:

```text
EKOS Architecture Investigation

Iteration: 3 / 5

[████████████████░░░░] 84%

Collection       ✓
Analysis         ✓
Reasoning        ✓
Model update     ✓
Generation       ✓
Evaluation       ✓
Investigation    in progress

Quality: 84%
Evidence: 412
Unknowns: 8
High priority: 2
```

---

# 31. Failure Handling

Failures must be classified.

```text
tool_failure
parse_failure
llm_failure
schema_failure
evidence_failure
evaluation_failure
configuration_failure
policy_failure
```

The agent should retry only when retrying is meaningful.

---

# 32. Retry Policy

Example:

```yaml
retry:
  tool:
    max_attempts: 2

  llm:
    max_attempts: 2

  parser:
    max_attempts: 1
```

Retries should not consume unlimited investigation budget.

---

# 33. Failed Investigation Tasks

A failed task should become explicit state:

```yaml
task:
  id: investigation.008

  status: failed

  reason:
    "No deployment metadata available."

  next_action:
    "Request human input or continue with documented uncertainty."
```

The agent must not convert failure into an invented conclusion.

---

# 34. Human Interaction

Human input is required when:

```text
critical contradiction
ambiguous business meaning
security-sensitive decision
conflicting authoritative sources
insufficient evidence
```

Example:

```text
EKOS needs your input.

Two sources disagree:

1. architecture.md → Oracle
2. production config → PostgreSQL

Which source represents the current production architecture?
```

The response becomes part of the AKM.

---

# 35. Human Review State

```text
EVALUATING
    ↓
CRITICAL AMBIGUITY
    ↓
WAITING_FOR_HUMAN
    ↓
human answer
    ↓
UPDATE_MODEL
    ↓
EVALUATE
```

---

# 36. Stop Conditions

The agent must stop when:

```text
quality threshold reached
```

or:

```text
no high-priority unknowns remain
```

or:

```text
no meaningful evidence can be collected
```

or:

```text
maximum iteration count reached
```

or:

```text
human cancellation
```

---

# 37. Quality-Based Completion

Example:

```yaml
completion_policy:
  quality_threshold: 0.90

  required:
    evidence_coverage: 0.90
    critical_issues: 0
    high_priority_unknowns: 0
```

All conditions should be configurable.

---

# 38. Diminishing Returns

The agent should detect when further investigation has low value.

Example:

```text
Iteration 1: 0.61
Iteration 2: 0.79
Iteration 3: 0.91
Iteration 4: 0.915
```

If:

```text
quality improvement < 1%
```

and no critical issue remains:

```text
STOP
```

This prevents wasteful agent loops.

---

# 39. Investigation Budget

The agent must operate within budgets.

```yaml
budget:
  max_iterations: 5
  max_llm_calls: 100
  max_investigation_tasks: 50
  max_runtime_minutes: 30
```

Optional:

```text
max_tokens
max_files_scanned
max_cloud_cost
```

---

# 40. Context Management

The agent should not send the entire repository to an LLM.

Instead:

```text
Question
   ↓
Relevant AKM subgraph
   ↓
Relevant evidence
   ↓
Relevant source snippets
   ↓
LLM
```

Context selection should be query-driven.

---

# 41. Evidence Context Builder

Example:

```yaml
context:
  question:
    "Does PaymentService consume Kafka?"

  entities:
    - service.payment

  relationships:
    - service.payment -> kafka

  evidence:
    - application.yml
    - KafkaConsumer.java
    - deployment.yaml

  existing_claims:
    - claim.102
```

This is the preferred LLM context.

---

# 42. Avoiding Self-Confirmation

A critical design principle:

> The component that generates documentation must not be the only component that validates it.

Recommended:

```text
Generator
    ↓
Document
    ↓
Independent Evaluator
    ↓
Evaluation
```

The evaluator should have access to:

```text
source evidence
AKM
generated document
```

but should not blindly trust the generator's claims.

---

# 43. Agent Decision Record

Every major agent decision should be recorded.

Example:

```yaml
decision:
  state: evaluating

  decision:
    INVESTIGATE

  reason:
    "Two high-priority relationships lack evidence."

  selected_tasks:
    - investigation.021
    - investigation.022

  policy:
    architecture-quality-v2
```

This improves observability and debugging.

---

# 44. Agent Event Log

Recommended event types:

```text
InvestigationStarted
CollectionStarted
EvidenceCollected
AnalysisCompleted
ReasoningStarted
ClaimCreated
ClaimRejected
ModelUpdated
GenerationStarted
GenerationCompleted
EvaluationStarted
EvaluationCompleted
InvestigationTaskCreated
InvestigationTaskCompleted
InvestigationTaskFailed
HumanInputRequired
HumanInputReceived
IterationStarted
IterationCompleted
InvestigationCompleted
InvestigationFailed
```

---

# 45. Event Example

```json
{
  "event": "EvidenceCollected",
  "investigation_id": "inv-001",
  "iteration": 3,
  "evidence_id": "evd-412",
  "source": "src/payment/KafkaConsumer.java",
  "extractor": "java.ast",
  "timestamp": "2026-08-21T13:10:00Z"
}
```

---

# 46. Deterministic vs Agentic Operations

The boundary should be explicit.

## Deterministic

```text
file discovery
parsing
AST
dependency extraction
schema validation
graph traversal
reference validation
diff
baseline comparison
```

## Agentic

```text
question formulation
semantic interpretation
reasoning
investigation planning
tool selection
ambiguity resolution
evaluation
stopping decision
```

This separation improves reliability.

---

# 47. Agent Policies

Policies control behavior.

Example:

```yaml
policy:
  llm:
    mode: local

  investigation:
    max_iterations: 5
    max_tasks: 50

  evaluation:
    quality_threshold: 0.90
    fail_on_critical_contradiction: true

  human_review:
    required_for:
      - critical_contradiction
      - security_decision
```

Policies should be versioned.

---

# 48. CLI

The first implementation may expose:

```bash
ekos architecture investigate ./repository
```

Options:

```bash
--llm local
--model qwen3
--offline
--max-iterations 5
--quality-threshold 0.90
--output ./docs
--format arc42
--format c4
```

---

# 49. Non-Interactive Mode

For CI/CD:

```bash
ekos architecture investigate . \
  --offline \
  --max-iterations 3 \
  --quality-threshold 0.85 \
  --fail-on-critical
```

Exit codes:

```text
0 = success
1 = quality threshold not reached
2 = critical contradiction
3 = execution failure
4 = configuration error
```

---

# 50. CI/CD Mode

Possible workflow:

```text
Pull Request
    ↓
EKOS Architecture Agent
    ↓
Architecture Diff
    ↓
Drift Detection
    ↓
Evaluation
    ↓
PR comment
```

Example:

```text
Architecture changed:

+ PaymentService
+ Kafka dependency
- LegacyPaymentAdapter

Documentation impact:
  C4 Container
  Integration View
  Deployment View

Documentation is stale.
```

---

# 51. Persistent Checkpointing

The agent must persist state after meaningful transitions.

If execution stops:

```text
restart
   ↓
load investigation state
   ↓
resume from last safe state
```

This is important for large repositories.

---

# 52. Idempotency

Running:

```bash
ekos architecture investigate .
```

twice should not duplicate:

```text
entities
relationships
evidence
claims
```

Stable identifiers and content hashes should be used.

---

# 53. Concurrency

Independent investigation tasks may execute in parallel.

Example:

```text
Task A:
inspect Terraform

Task B:
inspect Kubernetes

Task C:
inspect OpenAPI
```

Then:

```text
merge evidence
↓
reason
```

Reasoning should normally occur after the relevant evidence batch is available.

---

# 54. Concurrency Safety

Concurrent tasks must not independently mutate the canonical AKM without coordination.

Preferred:

```text
parallel collection
       ↓
evidence store
       ↓
single model update transaction
       ↓
reasoning
```

---

# 55. Security

The agent must support:

```text
secret detection
credential redaction
PII detection
sensitive-file exclusion
offline mode
provider allowlists
```

Example excluded files:

```text
.env
*.pem
*.key
credentials.json
secrets.yaml
```

Configuration must be explicit.

---

# 56. Auditability

Every important architecture claim should be answerable with:

```text
Who/what created it?
When?
From which evidence?
Using which model?
Using which prompt?
What confidence?
Was it reviewed?
```

This is essential for enterprise use.

---

# 57. Rust Implementation

Suggested modules:

```text
src/
├── agent/
│   ├── state.rs
│   ├── orchestrator.rs
│   ├── policy.rs
│   ├── decision.rs
│   └── checkpoint.rs
│
├── investigation/
│   ├── task.rs
│   ├── planner.rs
│   ├── executor.rs
│   └── budget.rs
│
├── collection/
├── analysis/
├── reasoning/
├── evaluation/
├── generation/
├── model/
├── evidence/
├── llm/
├── events/
└── mcp/
```

---

# 58. Core Rust State Enum

Conceptual:

```rust
enum AgentState {
    Initializing,
    Collecting,
    Analyzing,
    Reasoning,
    UpdatingModel,
    Generating,
    Evaluating,
    PlanningInvestigation,
    Investigating,
    WaitingForHuman,
    Completed,
    Failed,
    Cancelled,
}
```

---

# 59. State Transition Contract

```rust
trait StateHandler {
    fn execute(
        &self,
        context: &mut AgentContext,
    ) -> Result<Transition>;
}
```

Example:

```rust
enum Transition {
    Next(AgentState),
    Complete,
    Fail(AgentError),
    WaitForHuman(HumanQuestion),
}
```

---

# 60. Agent Context

```rust
struct AgentContext {
    investigation: InvestigationState,
    knowledge_model: ArchitectureModel,
    evidence_store: EvidenceStore,
    policy: AgentPolicy,
    llm: LlmProvider,
    tools: ToolRegistry,
    events: EventStore,
}
```

---

# 61. Tool Registry

```rust
trait ArchitectureTool {
    fn name(&self) -> &str;

    fn capabilities(&self) -> Vec<Capability>;

    fn execute(
        &self,
        request: ToolRequest,
    ) -> Result<ToolResult>;
}
```

Capabilities might include:

```text
source_analysis
sql_analysis
terraform_analysis
kubernetes_analysis
documentation_search
runtime_observation
semantic_reasoning
evaluation
```

---

# 62. Investigation Task Contract

```rust
struct InvestigationTask {
    id: TaskId,
    question: String,
    priority: Priority,
    targets: Vec<Target>,
    required_evidence: Vec<EvidenceType>,
    status: TaskStatus,
}
```

---

# 63. Agent Loop

Conceptually:

```rust
loop {
    match state {

        Collecting => collect(),

        Analyzing => analyze(),

        Reasoning => reason(),

        UpdatingModel => update_model(),

        Generating => generate(),

        Evaluating => evaluate(),

        PlanningInvestigation => plan(),

        Investigating => investigate(),

        WaitingForHuman => wait(),

        Completed => break,

        Failed => break,
    }
}
```

The implementation should preserve state after every meaningful transition.

---

# 64. MVP Agent

The first version should implement only:

```text
INITIALIZING
COLLECTING
ANALYZING
REASONING
UPDATING_MODEL
GENERATING
EVALUATING
PLANNING_INVESTIGATION
INVESTIGATING
COMPLETED
FAILED
```

Supported sources:

```text
Git
source code
Markdown
YAML
JSON
configuration
dependencies
```

Supported LLM:

```text
local Ollama
```

Supported output:

```text
C4 Context
C4 Container
Architecture Summary
```

---

# 65. MVP Investigation Loop

```text
1. Scan repository
2. Extract evidence
3. Build initial AKM
4. Run reasoning
5. Generate documentation
6. Evaluate documentation
7. Create investigation tasks
8. Execute targeted collection
9. Update AKM
10. Regenerate
11. Evaluate
12. Stop
```

Maximum:

```text
3 iterations
```

for MVP.

---

# 66. Phase 2

Add:

```text
Terraform
Kubernetes
OpenAPI
SQL
deployment analysis
data architecture
security analysis
MCP
human review
architecture diff
architecture drift
```

---

# 67. Phase 3

Add:

```text
runtime telemetry
logs
metrics
traces
continuous investigation
continuous drift detection
architecture Q&A
target architecture
migration planning
architecture fitness functions
```

---

# 68. Example End-to-End Session

Input:

```bash
ekos architecture investigate ./legacy-platform
```

Agent:

```text
Initializing...
Repository detected.

Local LLM:
Ollama / qwen3

Iteration 1

Collecting...
  1,842 files
  73 configuration files
  21 dependency manifests
  14 documentation files

Analyzing...
  118 entities
  264 relationships

Reasoning...
  31 inferred relationships

Generating...
  C4 Context
  C4 Container
  Architecture Summary

Evaluating...
  Score: 0.71

High priority gaps:
  1. Payment integration
  2. Deployment topology
  3. CustomerDB ownership
```

Agent creates investigation tasks.

```text
Iteration 2

Targeted collection:
  PaymentService
  Kubernetes manifests
  Terraform ownership metadata

Evidence added:
  87

Evaluation:
  0.86
```

Iteration 3:

```text
Evaluation:
  0.93

Critical issues:
  0

High-priority unknowns:
  0

Stopping investigation.
```

Final:

```text
Architecture reconstruction completed.

Quality: 93%
Iterations: 3
Evidence: 496
Entities: 147
Relationships: 391
Unknowns: 4
Unresolved contradictions: 1
```

---

# 69. Final Investigation Artifact

The agent should produce:

```text
.ekos/
└── architecture/
    ├── model.json
    ├── evidence.json
    ├── investigations.json
    ├── evaluations.json
    ├── events.json
    ├── baselines/
    └── provenance.json

docs/
├── architecture-summary.md
├── c4/
│   ├── context.md
│   └── containers.md
├── deployment.md
├── data.md
├── integration.md
├── security.md
├── risks.md
├── unknowns.md
└── investigation-report.md
```

---

# 70. Key Design Principle

The Architecture Agent should optimize for:

```text
knowledge quality
```

not:

```text
document length
```

A short, evidence-backed architecture model with explicit unknowns is better than a comprehensive-looking document containing hallucinated architecture.

---

# 71. Final Agent Principle

EKOS should behave like an architect investigating an unfamiliar legacy system:

```text
"What do I know?"
       ↓
"What evidence supports it?"
       ↓
"What don't I know?"
       ↓
"What evidence would answer that?"
       ↓
"Which tool can obtain it?"
       ↓
"Does the new evidence change my model?"
       ↓
"Is the documentation internally consistent?"
       ↓
"Is the architecture sufficiently understood?"
```

The agent stops only when it has sufficient evidence to justify stopping.

---

# 72. Relationship to RFC 2

RFC 2 defines:

```text
WHAT EKOS KNOWS
```

RFC 3 defines:

```text
HOW EKOS INVESTIGATES AND IMPROVES WHAT IT KNOWS
```

The separation is intentional.

```text
RFC 2
Architecture Knowledge Model
        +
Evidence
Claims
Relationships
Confidence
Provenance
Views

RFC 3
Architecture Agent
        +
State Machine
Tool Selection
Investigation
Evaluation
Feedback
Budgets
Stopping Criteria
```

Together:

```text
                 EKOS ARCHITECTURE INTELLIGENCE

       ┌──────────────────────────────────────────┐
       │           RFC 3: AGENT                   │
       │                                          │
       │ Collect → Analyze → Reason → Evaluate    │
       │       ↑                    ↓             │
       │       └── Investigate ← Feedback         │
       └──────────────────┬───────────────────────┘
                          ↓
       ┌──────────────────────────────────────────┐
       │           RFC 2: AKM                     │
       │                                          │
       │ Evidence → Claims → Relationships       │
       │ Facts → Inferences → Unknowns            │
       │ Confidence → Provenance → Reviews        │
       └──────────────────┬───────────────────────┘
                          ↓
                   Architecture Views
                          ↓
               Professional Documentation
```

This forms the foundation for an autonomous, evidence-driven architecture knowledge compiler.

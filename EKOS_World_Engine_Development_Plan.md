# EKOS World Engine — End-to-End Development Plan

## 1. Executive Summary

This document defines the implementation roadmap for extending EKOS from an engineering knowledge compiler into a general-purpose **knowledge graph, world-state, and agent simulation platform**.

Core pipeline:

```text
Raw Sources
    ↓
Ontology Generation
    ↓
Knowledge Graph
    ↓
World State
    ↓
Agent Memory & Beliefs
    ↓
Multi-Agent Simulation
    ↓
Event / Trajectory Analysis
    ↓
Reports
    ↓
Interactive World / Character Interviews
```

The first implementation should be local-first and deterministic where possible. X and Reddit should be future environment adapters, not the core simulation engine.

The first goal is to prove that EKOS can:

1. extract structured knowledge;
2. construct a provenance-aware graph;
3. create agents with separate beliefs, goals, memories, and knowledge;
4. simulate interactions over multiple rounds;
5. persist every state transition;
6. explain why a simulation reached a particular outcome;
7. let users interrogate the resulting world.

---

# 2. Product Vision

### Core positioning

> **EKOS turns raw information into a structured world model that humans and AI agents can explore, simulate, and interrogate.**

For the engineering audience:

> **EKOS gives AI agents structured memory and context about complex systems.**

For the broader simulation use case:

> **EKOS is a knowledge and simulation layer for building explainable agent worlds.**

---

# 3. Target Pipeline

```text
                    SOURCES
                       |
        +--------------+--------------+
        |              |              |
      Reports        Notes          Repositories
        |              |              |
        +--------------+--------------+
                       ↓
              01. ONTOLOGY
                  GENERATION
                       ↓
              02. KNOWLEDGE
                   GRAPH
                       ↓
                03. WORLD STATE
                       ↓
             +---------+---------+
             |         |         |
           Agent A   Agent B   Agent C
             |         |         |
             +---------+---------+
                       ↓
              04. SIMULATION
                       ↓
                  EVENT LOG
                       ↓
             +---------+---------+
             |         |         |
          Timeline   Analysis   Report
             |                   |
             +---------+---------+
                       ↓
              05. DEEP INTERACTION
                       |
              +--------+--------+
              |                 |
        Ask the World     Interview Agent
```

---

# 4. Design Principles

## 4.1 Evidence over assertions

The graph must preserve provenance.

Do not treat an LLM-generated statement as unquestionable truth.

Every important fact or relationship should be able to carry:

- source;
- source location;
- extraction method;
- timestamp;
- confidence;
- supporting evidence;
- optional contradictory evidence.

Example:

```yaml
relationship:
  source: Alice
  type: OPPOSES
  target: Bob
  confidence: 0.82
  provenance:
    source_document: report_17.md
    source_location: "paragraph 14"
    extracted_at: "2026-08-13T..."
```

## 4.2 Separate world truth from agent beliefs

This is a fundamental architectural decision.

The world may contain:

```text
Alice stole the money.
```

Alice may know:

```text
I stole the money.
```

Bob may believe:

```text
The money was stolen by Charlie.
```

Charlie may believe:

```text
Alice probably stole it.
```

These must not be stored as the same thing.

Use separate concepts:

```text
World Facts
Agent Beliefs
Agent Knowledge
Agent Memory
Agent Goals
Agent Fears
Agent Intentions
```

## 4.3 Temporal state

Relationships and beliefs can change.

The graph must support temporal validity:

```yaml
relationship:
  source: Alice
  type: SUPPORTS
  target: Bob
  valid_from: 2026-01-01
  valid_until: 2026-03-15
```

Simulation state is therefore a sequence of graph states:

```text
S0 → S1 → S2 → S3 → ... → Sn
```

## 4.4 Deterministic simulation protocol

The simulation engine must not simply ask an LLM:

> "What happens next?"

Instead use an explicit lifecycle:

```text
observe()
    ↓
reason()
    ↓
choose_action()
    ↓
validate_action()
    ↓
execute_action()
    ↓
record_event()
    ↓
update_world()
    ↓
update_memory()
```

This makes the simulation testable and reproducible.

## 4.5 Local-first

The initial implementation should support local execution.

Recommended components:

- local LLM through Ollama or another compatible runtime;
- local embeddings if needed;
- local graph storage;
- local TTS for later video generation;
- FFmpeg for media generation.

Cloud LLMs should be optional adapters.

---

# 5. Phase 0 — Repository and Architecture Preparation

## Objective

Prepare EKOS for a second domain without breaking its existing engineering-knowledge functionality.

## Tasks

### 5.1 Separate domain-independent core

Refactor the current EKOS core conceptually into:

```text
ekos-core
ekos-ontology
ekos-graph
ekos-provenance
ekos-world
ekos-agent
ekos-simulation
ekos-report
ekos-mcp
```

Exact crate/module names should follow the existing repository architecture.

### 5.2 Define stable interfaces

Expose interfaces rather than hard-coding source types:

```text
SourceAdapter
OntologyExtractor
GraphStore
WorldStore
AgentRuntime
SimulationEnvironment
ReportGenerator
```

### 5.3 Preserve existing functionality

Existing capabilities such as:

- repository analysis;
- ontology generation;
- documentation generation;
- MCP generation;

must continue working.

### Acceptance criteria

- Existing tests pass.
- Existing CLI commands continue to work.
- New graph/world modules can be tested independently.
- Generic graph code does not require code-specific concepts.

---

# 6. Phase 1 — Formalize the Ontology Model

You already have ontology generation. The next task is to make its output suitable for simulation.

## 6.1 Core entity model

Initial entity types:

```text
Person
Organization
Project
Product
Location
Document
Event
Topic
Claim
Concept
Resource
```

Later:

```text
Service
Repository
API
Database
Team
Agent
Channel
Message
```

## 6.2 Relationship model

Initial relationships:

```text
KNOWS
WORKS_FOR
OWNS
SUPPORTS
OPPOSES
TRUSTS
DISTRUSTS
MENTIONS
CLAIMS
CONTRADICTS
CAUSED
DEPENDS_ON
INFLUENCES
PARTICIPATES_IN
LOCATED_IN
```

Relationships must be extensible.

## 6.3 Claims

A claim should be a first-class object:

```yaml
claim:
  id: claim_001
  subject: Alice
  predicate: OPPOSES
  object: Bob
  confidence: 0.78
  source:
    document: report_17.md
    location: paragraph_12
```

This allows the system to distinguish:

```text
fact
claim
belief
hypothesis
prediction
```

---

# 7. Phase 2 — Knowledge Graph

## Objective

Build the persistent graph layer.

## 7.1 Graph primitives

Implement:

```text
Entity
Relationship
Claim
Evidence
Source
Event
```

Every object must have a stable ID.

## 7.2 Provenance

Minimum provenance:

```text
source_id
source_type
source_location
created_at
extracted_at
extractor
confidence
```

## 7.3 Graph operations

Implement:

```text
create_entity()
get_entity()
update_entity()
delete_entity()

create_relationship()
get_relationships()
delete_relationship()

find_neighbors()
find_path()
find_dependents()
find_related_entities()

query_subgraph()
```

## 7.4 Temporal operations

Implement:

```text
get_state_at(timestamp)
get_relationship_history(entity)
get_entity_history(entity)
```

## 7.5 Graph serialization

Support a portable format such as JSON:

```json
{
  "entities": [],
  "relationships": [],
  "claims": [],
  "events": []
}
```

The graph must be exportable and reloadable so scenarios are reproducible.

---

# 8. Phase 3 — World Model

## Objective

Turn the graph into a simulation-ready world.

The world is a graph plus state:

```text
World
├── entities
├── relationships
├── facts
├── events
├── channels
├── resources
├── time
└── metadata
```

## 8.1 World state

Example:

```yaml
world:
  time: 2026-08-13T10:00:00Z
  entities:
    - alice
    - bob
    - charlie
  relationships:
    - alice_supports_bob
    - bob_distrusts_alice
  resources:
    alice:
      influence: 0.8
      information: 0.9
```

## 8.2 Events

Events must be immutable:

```yaml
event:
  id: event_001
  round: 3
  actor: alice
  action: ACCUSE
  target: bob
  context:
    channel: public_forum
  effects:
    bob_reputation: -0.2
    alice_reputation: -0.05
```

---

# 9. Phase 4 — Agent Model

## Objective

Create agents with limited information and individual internal state.

## 9.1 Agent schema

```yaml
agent:
  id: alice
  name: Alice
  role: founder
  goals:
    - retain_control
  beliefs:
    - bob_wants_to_replace_me
  fears:
    - public_scandal
  knowledge:
    - event_001
    - event_004
  relationships:
    bob:
      trust: -0.7
  resources:
    influence: 0.8
    money: 0.5
    information: 0.9
```

## 9.2 Agent memory

Separate:

```text
Short-term memory
Long-term memory
Observed events
Beliefs
Private knowledge
Incorrect beliefs
```

An agent must not automatically receive the entire world graph.

## 9.3 Agent observation

Implement:

```text
agent.observe(world)
```

Observation returns only information available to that agent.

Example:

```text
World:
Alice stole money.

Alice sees:
Alice stole money.

Bob sees:
Money disappeared.

Charlie sees:
Alice was near the money.
```

---

# 10. Phase 5 — Agent Decision Engine

## Objective

Create a provider-independent decision API.

Possible implementations:

```text
RuleBasedAgent
LocalLLMAgent
CloudLLMAgent
HybridAgent
```

## Decision contract

Input:

```json
{
  "agent": {},
  "observations": [],
  "goals": [],
  "beliefs": [],
  "available_actions": []
}
```

Output:

```json
{
  "action": "POST_MESSAGE",
  "target": "bob",
  "content": "...",
  "reasoning_summary": "...",
  "confidence": 0.74
}
```

Do not require storing hidden chain-of-thought. Store only concise decision metadata suitable for auditing.

---

# 11. Phase 6 — Action System

Define a finite action vocabulary.

Initial actions:

```text
POST_MESSAGE
SEND_MESSAGE
SUPPORT
OPPOSE
SHARE_INFORMATION
WITHHOLD_INFORMATION
FORM_ALLIANCE
BREAK_ALLIANCE
REQUEST_INFORMATION
CHANGE_GOAL
CHANGE_BELIEF
DO_NOTHING
```

Each action must have:

```text
preconditions
effects
cost
visibility
target
validation rules
```

Example:

```yaml
action:
  type: FORM_ALLIANCE
  preconditions:
    - trust > 0.4
  effects:
    relationship_change: +0.2
  visibility:
    public: true
```

---

# 12. Phase 7 — Simulation Engine

## Objective

Run multiple rounds while preserving every state transition.

## Simulation lifecycle

```text
initialize_world()

for round in rounds:

    snapshot_world()

    collect observations for all agents

    generate decisions

    validate actions

    resolve conflicts

    execute actions

    persist events

    update world

    update agent memories

    calculate round metrics

generate final state
```

## Recommended execution model

For each round:

```text
1. Observe
2. Decide
3. Resolve conflicts
4. Execute
5. Persist
6. Update
```

Do not immediately mutate the world after Agent A acts before Agent B has observed the same round. This makes the simulation easier to reason about.

---

# 13. Phase 8 — Parallel Agent Execution

Agents should be conceptually parallel.

Initial implementation:

```text
Round N
    ↓
Collect observations for all agents
    ↓
Generate decisions
    ↓
Resolve conflicts
    ↓
Apply actions
    ↓
Generate events
    ↓
Update state
```

This avoids order-dependent behavior.

Later, truly parallel execution can be added where safe.

---

# 14. Phase 9 — Conflict Resolution

Example:

```text
Alice → SUPPORT Bob
Charlie → OPPOSE Bob
```

Implement:

```text
Action priority
Resource constraints
Visibility
Ordering
Conflict rules
Randomness seed
```

Every simulation must support:

```text
--seed 12345
```

so it can be reproduced.

---

# 15. Phase 10 — Scenario Definition

Create a scenario file.

Example:

```yaml
scenario:
  id: open_source_conflict
  name: "The Battle for Project X"

world:
  sources:
    - reports/report_01.md
    - reports/report_02.md

agents:
  - alice.yaml
  - bob.yaml
  - charlie.yaml

environment:
  type: virtual_forum

simulation:
  rounds: 20
  seed: 42
```

CLI:

```bash
ekos simulate scenario.yaml
```

---

# 16. Phase 11 — Virtual Social Environment

Do not integrate X or Reddit initially.

Build:

```text
VirtualForum
```

Capabilities:

```text
create_channel()
publish_message()
reply()
like()
share()
follow()
read_messages()
```

Messages become graph events.

Example:

```text
Alice posts
      ↓
Message event
      ↓
Bob observes
      ↓
Bob decides
      ↓
Bob replies
```

This provides the social-simulation model without external API complexity.

---

# 17. Phase 12 — Event Store

Every simulation action must produce an immutable event.

Example:

```json
{
  "id": "event_042",
  "round": 7,
  "timestamp": "...",
  "actor": "alice",
  "action": "POST_MESSAGE",
  "target": "public_forum",
  "content": "...",
  "observed_by": [
    "bob",
    "charlie"
  ]
}
```

The event log becomes the basis for:

- replay;
- debugging;
- reports;
- timelines;
- analytics;
- video;
- agent interviews.

---

# 18. Phase 13 — Simulation Replay

Implement:

```bash
ekos replay simulation.json
```

Capabilities:

```text
start
pause
next round
inspect event
inspect agent
inspect graph
jump to round
```

This is important for both debugging and demos.

---

# 19. Phase 14 — Metrics

Track objective simulation metrics.

Examples:

```text
relationship volatility
number of conflicts
alliances
information propagation
reputation changes
agent influence
coalition size
network centrality
goal achievement
resource consumption
```

These metrics describe the simulated world. They are not automatically real-world predictive measures.

---

# 20. Phase 15 — Turning Point Detection

Identify events that materially changed the trajectory.

Example:

```text
Round 7
Alice accused Bob.

Impact:
- Bob/Alice trust: -0.7
- Charlie joined Bob
- Alice reputation: -0.1
```

Output:

```yaml
turning_point:
  round: 7
  event: event_042
  impact:
    trust: high
    coalition: medium
    reputation: low
```

---

# 21. Phase 16 — Report Generation

Generate:

## Executive summary

```text
The simulation resulted in a coalition split after...
```

## Timeline

```text
Round 1
Round 4
Round 7
Round 12
Round 18
```

## Key turning points

```text
1. ...
2. ...
3. ...
```

## Risks

```text
High
Medium
Low
```

## Agent outcomes

```text
Alice:
Goal achievement: 0.72

Bob:
Goal achievement: 0.41
```

## Confidence

Clearly distinguish:

```text
Source confidence
Simulation confidence
Outcome stability
```

Do not present a simulation as a factual prediction.

---

# 22. Phase 17 — Scenario Variations

Run:

```text
Scenario A
Scenario B
Scenario C
```

with different initial conditions.

Example:

```text
A: Alice trusts Bob
B: Alice distrusts Bob
C: Bob receives additional information
```

Compare:

```text
Outcome
Coalition structure
Conflict level
Goal achievement
```

This is more useful than running one isolated simulation.

---

# 23. Phase 18 — Monte Carlo Simulation

Only after deterministic simulation works.

Run:

```bash
ekos simulate scenario.yaml --runs 100
```

Use different seeds and controlled variations.

Output:

```text
Outcome A: 47%
Outcome B: 31%
Outcome C: 22%
```

These percentages mean:

> percentage of simulation runs under the defined model, assumptions, and seeds.

They must not be presented as real-world probabilities without empirical validation.

---

# 24. Phase 19 — Deep Interaction

Implement two interfaces.

## 24.1 Ask the World

Examples:

```text
Why did the conflict escalate?

What caused Bob to change strategy?

Which event had the largest impact?

What assumptions led to this outcome?
```

Answers should be generated from:

```text
world state
event history
graph
simulation metrics
```

## 24.2 Interview an Agent

Example:

```text
Interview Alice
```

Questions:

```text
Why did you oppose Bob?

What did you know at round 5?

What changed your opinion?

What was your primary goal?
```

The agent must answer according to its historical state.

For example:

```text
Interview Alice at round 5
```

must not use knowledge acquired at round 15.

---

# 25. Phase 20 — Counterfactuals

Allow:

> What if Alice had trusted Bob?

Clone the world:

```text
World A
   |
   +---- baseline

World B
   |
   +---- modified assumption
```

Run both and compare.

Potential command:

```bash
ekos compare   --baseline baseline.yaml   --counterfactual trust-bob.yaml
```

This can become one of the strongest long-term EKOS capabilities.

---

# 26. Phase 21 — MCP Integration

Expose EKOS World Engine through MCP.

Initial tools:

```text
query_world
query_entity
query_relationships
inspect_agent
inspect_event
get_timeline
get_simulation_report
run_simulation
compare_simulations
interview_agent
```

This allows Claude Code and other AI agents to interact with the simulation.

---

# 27. Phase 22 — Web UI

Only after CLI functionality is stable.

Minimal screens:

## Scenario

```text
Scenario name
Sources
Agents
Simulation configuration
```

## Graph

Interactive graph visualization.

## Timeline

```text
Round 1 → Round 20
```

## Simulation

Live event stream.

## Agent

```text
Goals
Beliefs
Memory
Relationships
Actions
```

## Report

Executive summary and turning points.

## Interview

Chat interface.

---

# 28. Phase 23 — External Platform Adapters

Only after the virtual environment works.

Architecture:

```text
Simulation Engine
       |
       +--- Virtual Forum
       |
       +--- X Adapter
       |
       +--- Reddit Adapter
       |
       +--- Discord Adapter
```

Adapters translate:

```text
Platform event
     ↓
EKOS event
```

and:

```text
EKOS action
     ↓
Platform action
```

For external platforms, implement strict rate limits, authentication, platform rules, and explicit opt-in. Do not design the system for deceptive coordinated activity or manipulation of real users.

---

# 29. Phase 24 — Video Generation

Reuse the same event stream:

```text
Simulation
    ↓
Timeline
    ↓
Narrative
    ↓
Scenes
    ↓
Diagram
    ↓
Local TTS
    ↓
FFmpeg
    ↓
MP4
```

Potential command:

```bash
ekos report --format video simulation.json
```

Video sections:

```text
Introduction
World overview
Main actors
Initial relationships
Key events
Turning points
Final state
Alternative outcomes
```

---

# 30. Phase 25 — Example Demo Scenario

Build one polished fictional scenario.

## The Battle for Open Source Project X

Actors:

```text
Alice — project founder
Bob — major contributor
Acme Corp — sponsor
Charlie — community maintainer
Rival Project — competitor
```

Initial sources:

```text
10 reports
20 events
15 relationships
```

Pipeline:

```text
Documents
    ↓
Ontology
    ↓
Knowledge Graph
    ↓
5 agents
    ↓
20 rounds
    ↓
100+ events
    ↓
Report
    ↓
Agent interviews
```

Demo questions:

```text
Why did the conflict escalate?

Why did Charlie change sides?

What did Alice know at round 7?

Which event was the turning point?

What happens if Acme Corp withdraws funding?
```

---

# 31. Phase 26 — Engineering Scenario

After the fictional demo, create a scenario directly connected to EKOS's original purpose.

Example:

> **What happens if we remove a critical service from a legacy architecture?**

Input:

```text
GitHub repository
```

EKOS:

```text
Code
 ↓
Engineering Ontology
 ↓
Architecture Graph
 ↓
Service Agents
 ↓
Simulation
```

Question:

```text
What happens if Kafka is removed?
```

The simulation can model:

```text
Service dependencies
Data flows
Failure propagation
Recovery
```

This reconnects the World Engine with software engineering.

---

# 32. Testing Strategy

## Unit tests

Test:

- graph operations;
- provenance;
- temporal state;
- agent state;
- action validation;
- event generation;
- state transitions.

## Integration tests

Test:

```text
Ontology → Graph
Graph → World
World → Agent
Agent → Action
Action → Event
Event → World
```

## Determinism tests

Given:

```text
same scenario
same seed
same model
same configuration
```

the result should be reproducible.

## LLM tests

Use fixtures for known inputs.

Do not rely only on exact text matching.

Validate:

- action schema;
- allowed actions;
- target validity;
- consistency;
- evidence references.

---

# 33. Observability

Every simulation should produce:

```text
simulation_id
scenario_id
seed
model
model_version
configuration
round_count
event_count
duration
errors
```

Agent decisions should record:

```text
agent_id
round
observation IDs
selected action
decision confidence
```

Do not store private chain-of-thought. Store concise decision metadata and evidence references instead.

---

# 34. Security

Source content is untrusted.

Protect against:

- prompt injection;
- malicious documents;
- malicious repository instructions;
- tool-call injection;
- data exfiltration.

LLM-generated instructions must never automatically gain system privileges.

For local execution:

```text
sandbox external commands
restrict filesystem access
restrict network access
```

---

# 35. Performance Strategy

Start small.

MVP target:

```text
5 agents
20 rounds
100–500 events
10–50 source documents
```

Do not optimize prematurely.

Later:

```text
100 agents
1,000 rounds
100K events
```

may require:

- event streaming;
- graph indexes;
- caching;
- parallel inference;
- vector search;
- distributed workers.

---

# 36. Suggested Repository Structure

Possible structure:

```text
ekos/
├── crates/
│   ├── ekos-core/
│   ├── ekos-ontology/
│   ├── ekos-provenance/
│   ├── ekos-graph/
│   ├── ekos-world/
│   ├── ekos-agent/
│   ├── ekos-simulation/
│   ├── ekos-report/
│   ├── ekos-mcp/
│   └── ekos-cli/
│
├── scenarios/
│   ├── open-source-conflict/
│   └── engineering-incident/
│
├── examples/
├── docs/
├── tests/
└── scripts/
```

Adapt this to the current EKOS repository instead of blindly restructuring it.

---

# 37. CLI Design

Potential commands:

```bash
ekos ontology <source>

ekos graph build <ontology>

ekos world create <graph>

ekos simulate <scenario>

ekos replay <simulation>

ekos report <simulation>

ekos interview <simulation> <agent>

ekos compare <simulation-a> <simulation-b>

ekos mcp serve

ekos video <simulation>
```

The exact syntax can be simplified later.

---

# 38. Configuration

Use YAML or TOML.

Example:

```yaml
simulation:
  rounds: 20
  seed: 42

agents:
  runtime: local
  model: qwen

environment:
  type: virtual_forum

memory:
  type: graph

report:
  include:
    - timeline
    - turning_points
    - risks
    - agent_outcomes
```

---

# 39. Development Milestones

## M1 — Generic Ontology

Deliver:

- generic entities;
- relationships;
- claims;
- provenance.

Success:

```text
A non-code document can be represented.
```

## M2 — Persistent Graph

Deliver:

- graph store;
- graph queries;
- temporal relationships;
- serialization.

Success:

```text
Ontology can be loaded and queried.
```

## M3 — World Model

Deliver:

- world state;
- immutable events;
- snapshots;
- replay.

Success:

```text
A world can evolve through events.
```

## M4 — Agent Model

Deliver:

- goals;
- beliefs;
- memory;
- knowledge;
- relationships.

Success:

```text
Two agents can have different views of the same world.
```

## M5 — Simulation MVP

Deliver:

- 3–5 agents;
- 10–20 rounds;
- action system;
- deterministic seed;
- event log.

Success:

```text
A complete simulation runs reproducibly.
```

## M6 — Analysis

Deliver:

- timeline;
- turning points;
- metrics;
- report.

Success:

```text
The system can explain the simulated trajectory.
```

## M7 — Deep Interaction

Deliver:

- Ask World;
- Interview Agent;
- historical state queries.

Success:

```text
Users can interrogate the simulation.
```

## M8 — Counterfactuals

Deliver:

- world cloning;
- modified assumptions;
- comparative simulation.

Success:

```text
Users can compare alternative trajectories.
```

## M9 — MCP

Deliver:

- MCP server;
- world query tools;
- simulation tools.

Success:

```text
An external AI agent can interact with EKOS World Engine.
```

## M10 — Demo UI

Deliver:

- graph;
- timeline;
- agents;
- report;
- chat.

Success:

```text
A non-technical user can run and understand a simulation.
```

---

# 40. MVP Definition

The first public MVP should contain only:

```text
✓ Generic ontology
✓ Provenance
✓ Knowledge graph
✓ World state
✓ 3–5 agents
✓ Agent-specific beliefs
✓ Agent-specific memory
✓ Virtual forum
✓ 10–20 simulation rounds
✓ Immutable event log
✓ Deterministic seed
✓ Timeline
✓ Turning points
✓ Basic report
✓ Agent interview
```

Do not include initially:

```text
✗ X integration
✗ Reddit integration
✗ 100+ agents
✗ Real-world prediction claims
✗ Complex web UI
✗ AI video
✗ Monte Carlo
✗ Distributed infrastructure
```

---

# 41. Definition of Done for MVP

This command must work:

```bash
ekos simulate scenarios/open-source-conflict/scenario.yaml
```

and produce:

```text
simulation/
├── world.json
├── events.jsonl
├── timeline.json
├── metrics.json
├── report.md
└── final-state.json
```

The user must then be able to run:

```bash
ekos interview simulation/ alice
```

and ask:

```text
Why did you oppose Bob?

What did you know at round 7?

What changed your strategy?
```

Answers must be based on Alice's state at the appropriate simulation time.

---

# 42. Product Story

The final product story should be:

> **Information is fragmented.**
>
> EKOS turns fragmented information into structured knowledge.
>
> **Knowledge is static.**
>
> EKOS turns knowledge into a living world model.
>
> **Worlds contain actors with different goals and beliefs.**
>
> EKOS lets agents interact inside that world.
>
> **Simulation produces trajectories.**
>
> EKOS explains the trajectory and lets humans interrogate it.

Short version:

> **Build worlds from knowledge. Simulate what happens next. Understand why.**

---

# 43. Positioning Constraint

Do not market the first version as:

> "EKOS predicts the future."

Instead:

> **"EKOS simulates possible trajectories under explicit assumptions."**

The system should clearly distinguish:

```text
Observed fact
    ≠
Extracted claim
    ≠
Agent belief
    ≠
Simulation assumption
    ≠
Simulation outcome
    ≠
Real-world prediction
```

This distinction should exist in both the data model and the UI.

---

# 44. Recommended Development Order

```text
1. Refactor generic core
        ↓
2. Formalize ontology
        ↓
3. Add provenance
        ↓
4. Build persistent graph
        ↓
5. Add temporal state
        ↓
6. Build world model
        ↓
7. Build agent state
        ↓
8. Build action system
        ↓
9. Build deterministic simulation
        ↓
10. Build virtual forum
        ↓
11. Persist event stream
        ↓
12. Build replay
        ↓
13. Build metrics
        ↓
14. Detect turning points
        ↓
15. Generate reports
        ↓
16. Add agent interviews
        ↓
17. Add counterfactuals
        ↓
18. Add MCP
        ↓
19. Build UI
        ↓
20. Add local LLM optimization
        ↓
21. Add video generation
        ↓
22. Add external platform adapters
```

---

# 45. First Implementation Task

Do not start by building the simulation engine.

Start with:

> **Implement a generic, provenance-aware, temporal Knowledge Graph that can ingest the ontology output EKOS already generates and represent entities, claims, relationships, evidence, and events independently of the original source type.**

Create a test fixture:

```text
5 people
3 organizations
10 events
15 relationships
5 claims
```

Load it into EKOS.

Query it.

Serialize it.

Reload it.

Only when this works should the agent/simulation layer begin.

The graph becomes the foundation for:

```text
             EKOS Knowledge Graph
                     |
       +-------------+-------------+
       |             |             |
   Engineering    Documents     Simulations
       |             |             |
      MCP          Reports       Agents
                                  |
                              World State
                                  |
                              Scenarios
```

---

# 46. Final Strategic Recommendation

The most important architectural decision is **not to turn EKOS into a social-media simulator**.

Instead, build:

> **EKOS = knowledge graph + provenance + memory + world state + agent context infrastructure.**

Simulation becomes the first major application layer.

This preserves the existing engineering product while opening new use cases:

```text
Engineering
    ↓
Codebase Intelligence

Documents
    ↓
Knowledge Extraction

Simulation
    ↓
Agent Worlds

AI Agents
    ↓
Persistent Context

Reports
    ↓
Explainable Trajectories
```

The next concrete engineering milestone should therefore be:

> **Build `ekos-sim` MVP on top of a generic EKOS Knowledge Graph: 3–5 agents, separate beliefs and memories, 10–20 deterministic rounds, a virtual forum, immutable event history, trajectory analysis, and agent interviews.**

If that works, EKOS will have a genuinely new capability rather than another isolated documentation feature.

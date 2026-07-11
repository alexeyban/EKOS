Enterprise Knowledge Compiler Development Roadmap (v0.1)
Phase 0 — Bootstrap

Goal

Create a working development environment and compiler workspace.

Deliverables:

ekos/

    crates/
        compiler-core/
        compiler-sdk/
        scheduler/
        artifact/
        observation/
        cli/

    examples/

    docs/

    tests/

    scripts/

Technology

Rust Workspace
Cargo
GitHub Actions
Docker
protobuf / FlatBuffers (later)
SQLite only for temporary metadata (optional)

Success criteria

cargo build

cargo test

ekos --help
Phase 1 — Compiler Core

Goal

Build compiler infrastructure only.

No enterprise logic.

Implement

Compiler

Pass Manager

Scheduler

Artifact Manager

Logging

Diagnostics

Configuration

CLI

CLI

ekos init

ekos build

ekos clean

ekos doctor

No AI.

No connectors.

Phase 2 — Artifact System

Everything becomes artifacts.

Create artifact types.

ObservationArtifact

KnowledgeArtifact

EvidenceArtifact

DiagnosticArtifact

IndexArtifact

Every artifact

unique id
checksum
metadata
dependencies
version

Compiler should already understand

read

write

cache

reuse
Phase 3 — Observation SDK

Now build SDK.

Goal

Anyone can implement connectors.

SDK

trait Observer {

    fn scan(...)

}

Example

File Observer

Git Observer

SQL Observer

No AI.

Only observation.

Phase 4 — Observation Compiler

Now implement

Git

Filesystem

PostgreSQL

SQL Server

Output

Observation Package

For example

snapshot/

git/

database/

files/

metadata.json

Now compiler already works.

Phase 5 — Intermediate Representation

Only now introduce

KIR

Knowledge Intermediate Representation

Create

Object

Relationship

Event

Evidence

Nothing else.

No optimization.

Phase 6 — Knowledge Recovery

First AI milestone.

Compiler Pass

SQL Analyzer

↓

Business Entity

Relationship

Evidence

Another

Git Analyzer

Another

Confluence Analyzer

Output

Recovered Knowledge
Phase 7 — Identity Resolution

Separate project.

Input

Customer

Buyer

Client

Output

Canonical Object

This should become reusable.

Phase 8 — Semantic Compiler

Compile

Recovered Knowledge

↓

Canonical Knowledge Model

Still JSON.

No binary.

Phase 9 — Knowledge Ledger

Now build

append-only

ledger.

Store

Objects

Relationships

Events

Evidence

Current state

History

Indexes

Phase 10 — Runtime

Implement

Load Object

Load Neighborhood

Reconstruct State

Historical State

No AI yet.

Phase 11 — AI Runtime

Finally

LLM.

Compiler finished.

Runtime finished.

Now

Question

↓

Runtime

↓

Context

↓

LLM
Phase 12 — EKL

Only after compiler works.

Now define

Enterprise Knowledge Language.

Phase 13 — Optimizer

Incremental compilation.

Parallel compilation.

Caching.

Knowledge diff.

Knowledge merge.

Knowledge branch.

Phase 14 — Enterprise Scale

Only now

SAP

Salesforce

Oracle

Fabric

Snowflake

Kubernetes

Everything else.

Repository Structure
ekos/

    crates/

        compiler-core/

        scheduler/

        artifact/

        ledger/

        runtime/

        compiler-sdk/

        observation-sdk/

        identity/

        recovery/

        semantic/

        cli/

        common/

    plugins/

        postgres/

        sqlserver/

        git/

        confluence/

        jira/

    examples/

    docs/

    tests/

    benchmark/
Claude Code Development Rules

This is probably the most important part.

Claude should never generate random code.

Every task must follow exactly one workflow.

Task

↓

Design

↓

Architecture Review

↓

Interfaces

↓

Tests

↓

Implementation

↓

Refactoring

↓

Documentation

↓

Integration

↓

Benchmark

↓

Merge
Every Pull Request

Must satisfy

✓ Tests

✓ Documentation

✓ Benchmarks

✓ No public API break

✓ Compiler diagnostics

✓ Logging

✓ Examples
Coding Rules
Rust 2024 edition
Zero unsafe unless formally justified
No global mutable state
Dependency injection through traits
Every public API documented
Every artifact serializable
Every compiler pass deterministic
No hidden side effects
Pure functions wherever possible
Content-addressable artifacts
Reproducible builds
Version Strategy
v0.1

Compiler Infrastructure

------------

v0.2

Observation

------------

v0.3

Knowledge Recovery

------------

v0.4

Identity Resolution

------------

v0.5

Knowledge Ledger

------------

v0.6

Runtime

------------

v0.7

AI

------------

v1.0

Enterprise Knowledge Compiler
One final recommendation

I would add Phase -1, before writing any production code.

EKOS RFC Process

Every significant architectural decision starts as an RFC:

docs/rfcs/

0001-compiler-core.md

0002-artifact-system.md

0003-kir.md

0004-ledger.md

0005-runtime.md

...

No feature is implemented until its RFC is accepted.

This is how projects like Rust, Swift, Kubernetes, and many mature open-source ecosystems evolve. Given the ambition of EKOS, I think an RFC-driven development process will be invaluable. It will let Claude Code implement features against stable architectural contracts rather than evolving ideas, keeping the codebase aligned with the long-term vision we've developed.
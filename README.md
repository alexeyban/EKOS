# EKOS — Enterprise Knowledge Operating System

EKOS is an AI-native platform that continuously reconstructs, compiles, stores and serves enterprise knowledge.

Unlike traditional enterprise systems that manage data, documents or metadata independently, EKOS treats the entire enterprise as a living knowledge system — a permanently evolving semantic model that can be trusted by both humans and AI.

## The Problem

Modern enterprises contain enormous amounts of valuable knowledge distributed across disconnected systems: source code, databases, data warehouses, documentation, wikis, Git repositories, infrastructure-as-code, APIs, runtime logs, and monitoring systems. Every system contains only a partial description of reality. Documentation becomes outdated. Employees leave. Business logic remains hidden inside production code. AI assistants receive fragmented, inconsistent, and often contradictory information.

**Enterprises continuously lose knowledge.**

## The Insight

The enterprise already contains its own documentation — embedded inside source code, SQL, infrastructure definitions, APIs, logs, deployment history, schemas, and runtime behaviour. The problem is not missing information. The problem is the absence of a **compiler** capable of transforming enterprise reality into enterprise knowledge.

EKOS is that compiler.

## Architecture

```
          Enterprise Systems
 Git   SQL   APIs   Confluence   Logs   Cloud   Monitoring
                        |
                 Observation Layer        ← collects facts, no interpretation
                        |
               Knowledge Compiler         ← multi-pass: normalize → analyze → recover → verify
                        |
          ┌─────────────┴─────────────┐
   Knowledge Recovery          Identity Resolution
          └─────────────┬─────────────┘
                        |
          Canonical Knowledge Model (CKM)  ← language/storage/AI-provider independent
                        |
           Semantic Knowledge Ledger        ← append-only, every fact traceable to evidence
                        |
          ┌─────────────┴─────────────┐
    Knowledge Runtime          Knowledge Services
          └─────────────┬─────────────┘
                        |
            AI Agents & Enterprise Applications
```

### Semantic Primitives

The ledger stores four immutable primitives:

| Primitive | Description |
|-----------|-------------|
| **Object** | Identity of a concept: Customer, Product, Dataset, Service, Business Rule |
| **Relationship** | Semantic connection between objects (first-class, not just a foreign key) |
| **Event** | Immutable change — the only mechanism that mutates enterprise state |
| **Evidence** | Origin of knowledge: SQL query, source code, Git commit, log line, API spec |

Every semantic conclusion is supported by evidence. Every change is auditable.

### Key Invariants

- The **Observation Layer** collects facts only — it never interprets business meaning.
- The **ledger is append-only** — knowledge is never modified in place.
- The **Runtime is read-only** — it reconstructs and interprets state, never modifies it.
- **AI systems** consume reconstructed knowledge through the Runtime; they never touch raw enterprise systems directly.
- Every compiler pass is **deterministic** and **side-effect-free**.
- Every artifact is **content-addressable** (id + checksum + metadata + dependencies + version).

## Implementation

**Language:** Rust (2024 edition), Cargo workspace.

**Planned crates:** `compiler-core`, `compiler-sdk`, `observation-sdk`, `artifact`, `scheduler`, `ledger`, `runtime`, `identity`, `recovery`, `semantic`, `common`, `cli`.

**Planned plugins:** PostgreSQL, SQL Server, Git, Confluence, Jira.

### AI agent access (MCP)

`ekos mcp serve --workspace <dir>` exposes the read-only Runtime as a Model Context Protocol
server over stdio (RFC 0013) — tools: `ekos_search`, `ekos_ekl`, `ekos_neighborhood`,
`ekos_state`, `ekos_status`. Connect Claude Code with:

```bash
claude mcp add ekos -- ekos --config /path/to/ekos.toml mcp serve --workspace /path/to/workspace
```

## Development Process

All significant architectural decisions begin as RFCs in `docs/rfcs/`. No feature is implemented until its RFC is accepted. See `CLAUDE.md` for the full mandatory development workflow.

## Versioning Roadmap

| Version | Milestone |
|---------|-----------|
| v0.1 | Compiler Infrastructure |
| v0.2 | Observation Layer |
| v0.3 | Knowledge Recovery |
| v0.4 | Identity Resolution |
| v0.5 | Knowledge Ledger |
| v0.6 | Runtime |
| v0.7 | AI Layer |
| v1.0 | Enterprise Knowledge Compiler |

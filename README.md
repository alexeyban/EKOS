# EKOS — Enterprise Knowledge Operating System

EKOS is an AI-native platform that continuously reconstructs, compiles, stores and serves enterprise knowledge.

Unlike traditional enterprise systems that manage data, documents or metadata independently, EKOS treats the entire enterprise as a living knowledge system — a permanently evolving semantic model that can be trusted by both humans and AI.

## About

EKOS is a **compiler for enterprise knowledge**, not a database or document store. It observes an
enterprise's existing systems — source code, Git history, SQL schemas, GitHub issues/PRs,
Confluence, local PDF/DOCX documents, crypto/DeFi exports — without interpreting them, compiles
those observations through deterministic passes into a Canonical Knowledge Model, and stores the
result in an append-only ledger where every conclusion carries the evidence it was derived from. AI
agents (Claude Code among them) read that ledger through a read-only Model Context Protocol server
(`ekos mcp serve`, RFC 0013) — they never touch raw enterprise systems directly.

The project follows an RFC-first workflow (`docs/rfcs/`): every capability is designed in writing
before it's implemented, and the `devlog_*.md` files at the repo root are the running record of
what shipped, why, and what was learned building it. It is written in Rust (2024 edition) as a
Cargo workspace, and is licensed under the [MIT License](LICENSE).

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

**Crates (`ekos/crates/`):** `compiler-core`, `compiler-sdk`, `observation-sdk`, `artifact`, `kir`,
`scheduler`, `ledger`, `runtime`, `identity`, `recovery`, `ekl`, `semantic`, `marketing`, `common`,
`cli`.

**Connectors (`ekos/plugins/`):** File, Git, GitHub issues/PRs, Confluence, local documents
(PDF/DOCX/text/Markdown/HTML/email — text, tables, image OCR), crypto/DeFi export, plus
scaffolded proof-of-concept clients for Salesforce, SAP, Oracle, Microsoft Fabric, and Snowflake
(real API shapes, mock-tested — none yet exercised against a live account). PostgreSQL, SQL
Server, and Jira remain planned.

### Document semantic memory (RFC 0025/0026)

Beyond structural parsing, an opt-in pass reads local documents through an LLM to extract real
entities (`Concept` objects) and the relationships between them — so the same concept mentioned
across different documents becomes one findable, linkable thing instead of isolated text hits.
Enable it in `ekos.toml`:

```toml
[llm]
provider = "ollama"   # or omit for Anthropic via ANTHROPIC_API_KEY; any LlmProvider works

[document-semantics]
enabled = true
```

Then `ekos recover` runs `DocumentSemanticsAnalyzerPass` alongside the structural document pass,
and the extracted `Concept` objects are queryable through the same MCP tools as everything else
— `ekos_search`, `ekos_neighborhood`, `ekos_dependents`, `ekos ask`. No new tool, no new query
surface — this is exactly the point: AI tools get real memory through the Runtime they already
talk to.

### AI agent access (MCP)

`ekos mcp serve --workspace <dir>` exposes the read-only Runtime as a Model Context Protocol
server over stdio (RFC 0013) — tools: `ekos_search`, `ekos_ekl`, `ekos_neighborhood`,
`ekos_state`, `ekos_dependents` (single-hop impact analysis), `ekos_impact` (directed,
kind-filtered, multi-hop impact tracing — RFC 0018), `ekos_diff` (what changed since T),
`ekos_status`. Connect Claude Code with:

```bash
claude mcp add ekos -- ekos --config /path/to/ekos.toml mcp serve --workspace /path/to/workspace
```

The server also honors `EKOS_WORKSPACE` and `EKOS_CONFIG` environment variables, so a
registration can be path-free: `claude mcp add ekos --env EKOS_WORKSPACE=/path/to/workspace -- ekos mcp serve`.

### Marketing agent (RFC 0027)

`ekos marketing publish [devlog]` turns a `devlog_N.md` into a human-approved X (Twitter) release
announcement: it classifies the devlog's importance (skipping docs/tests/refactor-only entries),
drafts a tweet through the same `LlmProvider` used elsewhere, validates it (length, EKOS mention,
GitHub link, hashtag count), asks for Y/N/E approval, and publishes via a real OAuth 1.0a-signed
`POST /2/tweets` — with `marketing/posted/tweets.json` preventing the same devlog from ever being
posted twice.

```bash
ekos marketing publish            # latest devlog_*.md, interactive approval
ekos marketing publish 28         # a specific devlog number
ekos marketing publish --dry-run  # preview only — never posts, never records
```

Configure via `[marketing]`/`[marketing.twitter]` in `ekos.toml` (see `marketing/README.md`);
publishing requires `TWITTER_API_KEY`/`TWITTER_API_SECRET`/`TWITTER_ACCESS_TOKEN`/
`TWITTER_ACCESS_SECRET` in the environment and stays off until `[marketing.twitter] enabled =
true` is set explicitly.

### Demo: skills + custom subagents

`demo/` contains a rehearsable, 8-act demo of EKOS's Claude Code integration, run against
a real compiled workspace — two skills (`ekos-knowledge`, `memory`) and four custom
subagents, each embodying one capability:

| Agent | Model | Capability |
|---|---|---|
| `estate-scout` | haiku | existence — "what's out there?" (MCP-only, no file access) |
| `impact-analyst` | sonnet | consequence — blast radius + cited evidence |
| `memory-keeper` | sonnet | memory — the only agent that writes (recall, capture, async refresh) |
| `estate-architect` | inherit | synthesis — designs from the workspace's own prior art |

**Install the agents:**

```bash
cp demo/agents/*.md ~/.claude/agents/
```

Then in Claude Code, run `/agents` and confirm all four appear.

**Run it live** — open Claude Code from the workspace root (the directory containing
`ekos.toml`) and follow the acts in [`demo/DEMO.md`](demo/DEMO.md), which gives the exact
prompt, expected MCP calls, and payoff line for each act.

**Run it headless** (rehearsal, transcripts, or a live-demo fallback):

```bash
sh demo/headless.sh          # generate a transcript for all 7 acts
sh demo/headless.sh 2 7      # just specific acts
```

Transcripts land in `demo/transcripts/act-N.md` — see the ones already committed there for
real, unedited examples of what each act produces.

Before presenting, work through **Act 0** in `demo/DEMO.md`: refresh the ledger, start a
fresh MCP connection (a long-running one can go stale after a rebuild), install the agents,
and smoke-test headlessly first.

### Compact storage (RFC 0015)

Workspaces created before RFC 0015 can be shrunk in place (both commands verify before
touching anything and leave backups):

```bash
ekos ledger status --storage   # per-component size report
ekos ledger migrate            # ledger v1 → v2: dictionary-zstd payloads (~2.5x smaller)
ekos artifact repack           # loose JSON files → packed segments (~7x smaller on disk)
```

### Fact-segment engine (RFC 0016, experimental opt-in)

`ekos ledger migrate --v3` migrates a workspace onto the fact-segment engine
(EAV facts, immutable segments, tantivy search, mmap'd reads) — every version
is signature-verified during migration, the SQLite source is left untouched,
and deleting `.ekos/ledger/facts/` rolls back. Migrated workspaces are served
by the fact engine automatically. The RFC's storage gate was amended with
measurements in hand (≤2× of the v2 ledger at equal-or-better read latency —
it passes at 1.66× with 19× faster search); fresh workspaces keep the SQLite
default during the soak period (devlog 18).

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

## License

MIT — see [LICENSE](LICENSE).

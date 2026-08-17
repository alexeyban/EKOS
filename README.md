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

EKOS also has a community token (`TOKENOMICS.md`) whose utility is designed to grow alongside
the platform — see `VISION.md` for the phased ecosystem roadmap behind it.

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
- **AI systems** consume reconstructed knowledge through the Runtime; they never touch raw enterprise systems directly. The one explicit, scoped, audited exception is the gated `ekos_clickhouse_query` MCP tool (RFC 0056, off by default) — see below.
- Every compiler pass is **deterministic** and **side-effect-free**.
- Every artifact is **content-addressable** (id + checksum + metadata + dependencies + version).
- **Secrets and PII are never observed or stored** (RFC 0043) — a built-in baseline redacts known
  secret shapes (AWS/GitHub/Slack/Google/Stripe tokens, private keys, JWTs, generic
  key/password assignments) from all observed content, and excludes files like `.env`/`*.pem`/
  `id_rsa` entirely. `ekos.toml`'s `[security]` section can only extend this baseline, never
  disable it.

## Implementation

**Language:** Rust (2024 edition), Cargo workspace.

**Crates (`ekos/crates/`):** `compiler-core`, `compiler-sdk`, `observation-sdk`, `artifact`, `kir`,
`scheduler`, `ledger`, `runtime`, `identity`, `recovery`, `ekl`, `semantic`, `marketing`, `docs-gen`,
`dbt-gen`, `common`, `cli`, `demo-server`, `simulation` (RFC 0047-0055's opt-in World Engine — see
below), and `clickhouse-query` (RFC 0056's opt-in live NL-to-SQL query engine — see below).

**Connectors (`ekos/plugins/`):** File, Git, GitHub issues/PRs, Confluence, local documents
(PDF/DOCX/text/Markdown/HTML/email — text, tables, image OCR), Pentaho Kettle (`.ktr`/`.kjb` —
RFC 0027), Python/PySpark source (real AST parsing, DataFrame chains recovered into the
Transformation IR — RFC 0038/0040), Rust source (real AST parsing, real function-call graph —
RFC 0041), ClickHouse (real HTTP client, schema metadata plus an opt-in live query engine — RFC
0056), crypto/DeFi export, plus scaffolded proof-of-concept clients for Salesforce, SAP, Oracle,
Microsoft Fabric, and Snowflake (real API shapes, mock-tested — none yet exercised against a live
account). PostgreSQL, SQL Server, and Jira remain planned.

## Installation

EKOS builds from source — there's no prebuilt binary release yet. **The Cargo workspace root is
`ekos/`, not the repo root** — there is no top-level `Cargo.toml`, so `cargo` commands must be run
from inside `ekos/` (or with `--manifest-path ekos/Cargo.toml`).

Prerequisites on both platforms:
- **Rust**, stable channel, via [rustup](https://rustup.rs) — 2024 edition needs rustc 1.85+;
  installing the latest stable is fine.
- **A C/C++ toolchain** — `rusqlite`'s bundled SQLite and the `zstd` crate both compile native C
  source at build time, so a working `cc` is required even though the project itself is pure Rust.
- **Git**, to clone the repo.

### macOS

```bash
# 1. C toolchain (skip if already installed)
xcode-select --install

# 2. Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 3. Clone and build (note: cd into ekos/, the actual workspace root)
git clone https://github.com/alexeyban/EKOS.git
cd EKOS/ekos
cargo build --release --workspace

# 4. Binary is at target/release/ekos — run it directly, or install onto PATH:
cargo install --path crates/cli
```

(Homebrew's `rustup-init` — `brew install rustup-init && rustup-init` — works the same way if you
prefer Homebrew-managed installs.)

### Windows 11

Two supported paths — WSL2 is the path of least friction for a Unix-first Rust CLI project, since
it gives you a real Linux toolchain; native Windows works too and is fully supported by Rust.

**Option A — WSL2 (recommended):**

```powershell
wsl --install                       # if WSL2 isn't already set up; reboot if prompted
```

Then open the Ubuntu shell it installs and follow the **macOS/Linux steps above** (`xcode-select`
isn't applicable — `sudo apt install build-essential` gives you the C toolchain instead — then the
same `rustup.rs` install and `cargo build` commands).

**Option B — Native Windows:**

```powershell
# 1. C++ build tools (provides the MSVC linker rustc's default toolchain needs)
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# 2. Rust (accept the default x86_64-pc-windows-msvc toolchain when prompted)
winget install Rustlang.Rustup

# 3. Git, if not already installed
winget install Git.Git

# 4. Clone and build (open a new terminal first, so the updated PATH takes effect)
git clone https://github.com/alexeyban/EKOS.git
cd EKOS\ekos
cargo build --release --workspace

# 5. Binary is at target\release\ekos.exe — run it directly, or install onto PATH:
cargo install --path crates\cli
```

### Verify the install

```bash
ekos --help                # or: cargo run -p ekos -- --help, from ekos/
ekos init                  # creates .ekos/ in the current directory
cargo test --workspace     # optional: run the test suite (500+ tests)
```

See [`CLAUDE.md`](CLAUDE.md) for the full command reference and the mandatory development
workflow if you're planning to contribute.

### Legacy transformation recovery (RFC 0027/0028/0029)

A Pentaho step, a SQL `SELECT`, a `VIEW`, and a stored procedure are all the same underlying
concept — a transformation of data from sources to a sink through filter/join/aggregate/calculate
operations. `ekos recover` compiles all of them into one shared **Transformation IR**
(`Source`/`Filter`/`Join`/`Aggregate`/`Calculate`/`Sink`/`Unmapped`), so legacy ETL logic recovered
from a Pentaho `.ktr`/`.kjb` job can be diffed against a newly drafted SQL pipeline — no manual XML
reading required. `Unmapped` is deliberate, not a gap swept under the rug: anything that can't be
parsed is still recorded as evidenced fact ("something is here, not yet understood"), never
silently dropped.

The same real-world entity observed under different names across systems (Informix `cust_mstr`,
Postgres `customers`, Databricks `gold.dim_customer`) can be linked too: `ekos identity scan`
scores candidate cross-system matches (column overlap, naming-pattern similarity, type
compatibility) and writes them as `unconfirmed` relationships — never a silent auto-merge — for
review via the `ekos_identity_review` MCP tool.

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
talk to. `ekos ask` honors `[llm] provider = "ollama"` the same way `ekos recover` does — both
commands select the LLM provider through one shared function, so a workspace configured for local
Ollama works identically for recovery and for querying.

### Documentation generation (RFC 0035/0037/0042)

`ekos docs generate` renders the compiled ledger straight into Markdown/HTML documentation —
zero LLM calls, every claim traceable to real compiled evidence. Two layouts:

```bash
ekos docs generate                              # --layout objects (default): one page per
                                                  # significant object, plus an index and ER diagram
ekos docs generate --layout curated --output doc # README.md/Architecture.md/API.md/
                                                  # SequenceDiagrams.md — the shape a developer
                                                  # actually expects, plus one detail page per
                                                  # crate/technology/pipeline/program-entity object
```

`--layout curated`'s `Architecture.md` includes a real crate/workspace dependency graph (parsed
`Cargo.toml`, not guessed), external technology dependencies, CI/CD pipelines (parsed
`.github/workflows/*.yml`), and an entity-relationship diagram; `API.md` lists real functions/
structs/enums/traits (from `RustSymbol`/`PythonSymbol` objects, RFC 0038/0040/0041) grouped by
file, each linked to its own detail page; `SequenceDiagrams.md` covers both Transformation-IR
data-flow sequences and real function-call sequences (RFC 0041's `Calls` graph). Per-entity pages
nest under `entities/<kind>/<2-char shard>/` so a large codebase's page count never blows past
GitHub's per-directory file-listing cap — this repo's own `doc/` (generated from EKOS's own
source) is the running example. `--prose` (opt-in) layers an LLM-written overview onto each
object page, reusing `ekos ask`'s exact grounding+citation pipeline, with a token-cost estimate
shown before any call.

### Hierarchical rollups (RFC 0044)

Every other context-saving mechanism in EKOS (capped search results, hop-bounded graph walks) is
*retrieval*-limiting — fewer raw facts, never a synthesized higher-level one. `ekos commit` now
also synthesizes deterministic, zero-LLM `Rollup` objects: one per directory subtree (crate-level
by default) or, in a multi-project `[observe] paths` estate, one per project — each carrying real
member counts, a kind breakdown, and boundary-relationship counts (what crosses in/out of the
subsystem), linked to every member via the same `Contains` relationship everything else already
uses. This is exactly what closes the "huge project/many projects" context-window gap: an agent
asking about a whole subsystem gets one condensed, evidence-linked object instead of personally
synthesizing meaning from dozens of raw facts. Surfaced automatically in `Architecture.md`'s new
`## Subsystems` section (see above) — this repo's own `doc/Architecture.md` shows 46 real ones,
one per crate/plugin, generated from EKOS's own source.

### Hosted demo server (RFC 0045, experimental)

`ekos/crates/demo-server` is a small, read-only web server over a **fixed two-repo catalog** —
built to answer a strategic question, not a roadmap phase: pick EKOS's single most painful task
(making sense of a codebase without hitting an LLM's context-window ceiling) and put it in front of
peers in a 5–10 minute demo, without anyone installing the CLI. Two binaries:

```bash
cargo run -p ekos-demo-server --bin prerender -- <curated-markdown-dir> <output-html-dir>  # bake step
cargo run -p ekos-demo-server --bin demo-server -- catalog.toml                            # serve
```

`prerender` pre-renders `ekos docs generate --layout curated`'s Markdown output to static HTML once,
offline (curated HTML isn't a general `docs-gen` feature yet — see the RFC). `demo-server` serves
that pre-rendered output plus a `POST /ask` endpoint that reuses `AiRuntime::ask` unmodified,
refusing to start rather than degrading silently if `ANTHROPIC_API_KEY` isn't set. Not general
self-serve ingestion — a fixed, pre-baked catalog only. **Not yet demo-ready**: implemented and
verified against a placeholder key (routing, boot check, rate limiting, static serving all confirmed
correct), but live-question answer quality is unverified pending a real API key and a rehearsed run
— see `devlog_45.md` and `TODO.md`.

### AI agent access (MCP)

`ekos mcp serve --workspace <dir>` exposes the read-only Runtime as a Model Context Protocol
server over stdio (RFC 0013) — tools: `ekos_search`, `ekos_ekl`, `ekos_neighborhood`,
`ekos_state`, `ekos_dependents` (single-hop impact analysis), `ekos_impact` (directed,
kind-filtered, multi-hop impact tracing — RFC 0018), `ekos_diff` (what changed since T),
`ekos_status`, `ekos_transformation_explain`/`ekos_transformation_diff` (Transformation IR
explanation and migration diffing — RFC 0028), and `ekos_identity_review` (confirm/reject a
cross-system identity match — RFC 0029, the one write-capable tool; every other tool reads only the
local ledger). A gated `ekos_clickhouse_query` tool (RFC 0056) is also available, off by default —
see the ClickHouse connector section below. Connect Claude Code with:

```bash
claude mcp add ekos -- ekos --config /path/to/ekos.toml mcp serve --workspace /path/to/workspace
```

The server also honors `EKOS_WORKSPACE` and `EKOS_CONFIG` environment variables, so a
registration can be path-free: `claude mcp add ekos --env EKOS_WORKSPACE=/path/to/workspace -- ekos mcp serve`.

### Marketing agent (RFC 0030)

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

### World Engine simulation (RFC 0047-0055, experimental)

Auxiliary, opt-in tooling built on top of the same ledger, kept deliberately separate from the
compiler pipeline above: multi-agent scenarios with beliefs, goals, deterministic round-based
decision-making, seed-reproducible priority/resource conflict resolution, a `VirtualForum`
(channels, replies, likes, follows, shares), a durable, replayable event log, and `world.sources`
document ingestion (real files, via the actual `localdocs` connector) — layered additively over the
existing graph (see `ekos/docs/rfcs/0047`-`0055` and `devlog_47.md`-`devlog_55.md`). Define a
scenario and its agents in YAML (source-document-style `agent.yaml`/`scenario.yaml` shapes), run
it, and read it back afterward:

```bash
ekos simulate scenario.yaml             # runs scenario.yaml's own simulation.rounds
ekos simulate scenario.yaml --rounds 5  # override the round count
ekos simulate scenario.yaml --seed 42   # override the round's priority/resource-conflict seed
ekos replay scenario.yaml               # read back every recorded round, read-only
ekos replay scenario.yaml --round 2     # narrow to one round
```

A scenario's `world: { sources: [reports/report_01.md] }` ingests real documents (PDF/DOCX/text/
Markdown/HTML/email) into its starting world; an agent's `knowledge:`/`relationships:` can
reference an ingested document by that same path string.

By default `simulate` writes to a dedicated `.ekos/simulations/<scenario-id>/ledger.db`, **never**
the real workspace ledger — simulated agents and events are fictional and regenerated on every run,
and because the ledger has no delete/tombstone mechanism (RFC 0043), they should never permanently
commingle with real, evidence-backed compiled knowledge. `--ledger <path>` opts back into a
different target explicitly, including the real workspace ledger, if a caller wants that.

This is a distinct capability from the "compiler for enterprise knowledge" positioning above, not
a replacement for it — kept intentionally separate rather than blended into one pitch. Whether it
grows into its own product surface is an open question, still being decided one RFC at a time.

### ClickHouse connector (RFC 0056)

Two independent pieces. **Compiled metadata** — `ekos build`/`ekos recover` observe a configured
ClickHouse database's `system.tables`/`system.columns` (via ClickHouse's stock HTTP interface, no
native driver) and compile every table into a real `KirObject(ObjectKind::Table)`, searchable
through `ekos_search`/`ekos ekl` and cross-system identity-resolvable against same-named tables
elsewhere in the estate, the same way file-based SQL recovery already is:

```bash
export EKOS_CLICKHOUSE_URL=http://localhost:8123
export EKOS_CLICKHOUSE_DATABASE=analytics
export EKOS_CLICKHOUSE_USER=default        # optional
export EKOS_CLICKHOUSE_PASSWORD=            # optional
ekos build && ekos recover
```

**Live NL-to-SQL query** — the one path in EKOS that intentionally crosses the Key Invariant above:
an LLM builds a ClickHouse `SELECT` from the compiled schema and the question, the generated SQL is
parsed and hard-rejected unless it's exactly one `SELECT` (no writes, no multi-statement batches),
then it's run live, redacted, and returned — every call is recorded as an Evidence/Event pair in the
ledger for audit, though the row data itself is never ledgered:

```bash
ekos clickhouse ask "how many orders were placed last week?"
```

This CLI command is always available. The matching `ekos_clickhouse_query` MCP tool is **off by
default** — `ekos mcp serve` only lists it once a workspace explicitly opts in:

```toml
[clickhouse]
enable-mcp-query = true
```

### Demo: skills + custom subagents

`demo/` contains a rehearsable, twelve-act demo of EKOS's Claude Code integration, run against
a real compiled workspace — two skills (`ekos-knowledge`, `memory`) and six custom
subagents, each embodying one capability:

| Agent | Model | Capability |
|---|---|---|
| `estate-scout` | haiku | existence — "what's out there?" (MCP-only, no file access) |
| `impact-analyst` | sonnet | consequence — blast radius + cited evidence |
| `memory-keeper` | sonnet | memory — the only agent that writes (recall, capture, async refresh) |
| `estate-architect` | inherit | synthesis — designs from the workspace's own prior art |
| `legacy-logic-recoverer` | sonnet | recovery — explains a Pentaho/SQL transformation chain, evidence per step (RFC 0027/0028) |
| `identity-reviewer` | sonnet | review — batches cross-system identity hypotheses for confirm/reject (RFC 0029) |

**Install the agents:**

```bash
cp demo/agents/*.md ~/.claude/agents/
```

Then in Claude Code, run `/agents` and confirm all six appear.

**Run it live** — open Claude Code from the workspace root (the directory containing
`ekos.toml`) and follow the acts in [`demo/DEMO.md`](demo/DEMO.md), which gives the exact
prompt, expected MCP calls, and payoff line for each act.

**Run it headless** (rehearsal, transcripts, or a live-demo fallback) — automates Acts 1–8
(the skill/single-agent acts); Acts 9–12 (multi-agent chains that each need their own scratch
workspace built first, RFC 0018/0027-0029 scenarios) are presented live only:

```bash
sh demo/headless.sh          # generate a transcript for acts 1-8
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

## Presentations

Live decks at [alexeyban.github.io/EKOS](https://alexeyban.github.io/EKOS/presentations.html) — every claim in them is reproduced live against real repos, not staged:

- [Claude Code + EKOS](https://alexeyban.github.io/EKOS/presentations/claude-code-with-ekos.html) — how Claude Code searches and analyzes a codebase through EKOS's MCP server instead of raw grep/Read, with a measured with-vs-without comparison and real token/usage numbers.
- [The AI-Native Enterprise Knowledge Compiler](https://alexeyban.github.io/EKOS/presentations/ai-native-knowledge-compiler-pitch.html) — the startup pitch, audited live by Claude Code using EKOS's own MCP server.
- [Vision & Token Utility](https://alexeyban.github.io/EKOS/presentations/vision-and-token-utility.html) — why the EKOS token's relevance is designed to grow as a consequence of platform adoption, not a promise of price.

See [alexeyban.github.io/EKOS/presentations.html](https://alexeyban.github.io/EKOS/presentations.html) for the full list.

## Official EKOS Token

Network: Solana

Contract (Mint) Address:

CwubepDFJndzSKFmAMAm9u8Xx3PrizAwSq8hcGimpump

Pump.fun:
https://pump.fun/coin/CwubepDFJndzSKFmAMAm9u8Xx3PrizAwSq8hcGimpump

See [TOKENOMICS.md](TOKENOMICS.md) for full allocation details.

## Official Channels

X (Twitter): [@ekosproject](https://x.com/ekosproject) — release announcements posted via
`ekos marketing publish` (RFC 0030).

## Founder Vesting Wallet

u2zUCiUHRoGp9jKRsyjMGQ8x9Z3UdtERm174aiXURZo

Managed through Streamflow.

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

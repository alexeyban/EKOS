# EKOS — Enterprise Knowledge Operating System

EKOS is an AI-native platform that continuously reconstructs, compiles, stores and serves enterprise knowledge.

Unlike traditional enterprise systems that manage data, documents or metadata independently, EKOS treats the entire enterprise as a living knowledge system — a permanently evolving semantic model that can be trusted by both humans and AI.

**First benchmark:** on a real 2,022-file open-source repo ([plausible/analytics](https://github.com/plausible/analytics)), cold ingestion takes 34 seconds, and answering real questions from the compiled ledger costs **67-93% fewer tokens** than raw grep-based search over the source — measured with a standard tokenizer (`tiktoken`), not a hand-rolled estimate, with the one case grep wins included rather than hidden. Full methodology, every command, and every raw output: [The First Benchmark Number](https://alexeyban.github.io/EKOS/presentations/token-benchmark.html).

## About

EKOS is a **compiler for enterprise knowledge**, not a database or document store. It observes an
enterprise's existing systems — source code, Git history, SQL schemas, GitHub issues/PRs,
Confluence, local PDF/DOCX documents, crypto/DeFi exports — without interpreting them, compiles
those observations through deterministic passes into a Canonical Knowledge Model, and stores the
result in an append-only ledger where every conclusion carries the evidence it was derived from. AI
agents (Claude Code among them) read that ledger through a read-only Model Context Protocol server
(`ekos mcp serve`, RFC 0013) — they never touch raw enterprise systems directly.

The project follows an RFC-first workflow (`docs/rfcs/`): every capability is designed in writing
before it's implemented, and the `devlogs/devlog_*.md` files are the running record of
what shipped, why, and what was learned building it. It is written in Rust (2024 edition) as a
Cargo workspace, and is licensed under the [MIT License](LICENSE).

EKOS also has a community token — utility designed to grow as a consequence of platform adoption,
not a promise of price. See [Token & Community](#token--community) below.

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

**Connectors (`ekos/plugins/`):** File, Git, GitHub issues/PRs (live-verified against a real
repo, 1,600 real issues/PRs — RFC 0062), Confluence, local documents
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

### dbt metadata extraction (RFC 0117)

dbt can point at any warehouse, so `ekos recover` extracts real `Table` objects from a dbt
project's own checked-in metadata rather than a live database connection — never `manifest.json`/
`catalog.json` either, since both are `dbt/target/` build artifacts, gitignored in every real
project inspected while designing this. One `Table` per `models/**/*.sql` file (a model exists the
moment its `.sql` file does, regardless of whether any YAML documents it) and one per declared
`sources[].tables[]` entry (no `.sql` file backs a source — it's a pre-existing table dbt only
references). `ref()`/`source()` macro calls in each model's raw SQL become real `DependsOn`
relationships, resolved against the models/sources found in that same dbt project; an unresolvable
reference (e.g. into an installed, gitignored `dbt_packages/`) is honestly skipped, never
fabricated. Declared `schema.yml` columns are merged in as-is — explicitly partial, since dbt
projects typically only document tested/described columns, not every column a model produces.

The same real-world entity observed under different names across systems (Informix `cust_mstr`,
Postgres `customers`, Databricks `gold.dim_customer`) can be linked too: `ekos identity scan`
scores candidate cross-system matches (column overlap, naming-pattern similarity, type
compatibility) and writes them as `unconfirmed` relationships — never a silent auto-merge — for
review via the `ekos_identity_review` MCP tool. Same-source duplicates (`ekos resolve`/`ekos
compile`, e.g. two `Table` objects both literally named `customers`) auto-merge only when the
match is an exact normalized name; anything fuzzy goes through that same `unconfirmed`/review
flow instead of an irreversible merge (RFC 0063) — no confidence threshold on the underlying
scoring formula reliably separates real correct fuzzy merges from real incorrect ones, so an
irreversible auto-merge isn't a safe default for that case.

### Real schema and class structure from source (RFC 0091/0092)

Beyond raw SQL DDL, `ekos recover`'s Python analyzer recognizes a real SQLAlchemy declarative
model (`__tablename__` present on a class) and compiles it into the same `Table` object shape as a
`CREATE TABLE` statement — real column names, best-effort data-type hints, and `ForeignKey` edges
resolved against other models in the same file — so a project whose entire schema is ORM-declared
(the majority shape for a modern Python backend) still gets a real `## Data Architecture` and
entity-relationship diagram instead of an empty section. The same analyzer also compiles real
class inheritance (`class Document(Base):`) into `RelationshipKind::Extends` edges between the
real `PythonSymbol` objects involved — visible on each class's own generated page and in its
Mermaid diagram — resolved only when the base class is defined in the same file, never fabricated
against an imported base EKOS can't see the definition of.

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

### Documentation generation (RFC 0035/0037/0042/0090/0094/0095)

`ekos docs generate` renders the compiled ledger straight into Markdown/HTML documentation —
zero LLM calls, every claim traceable to real compiled evidence. Three layouts:

```bash
ekos docs generate                              # --layout objects (default): one page per
                                                  # significant object, plus an index and ER diagram
ekos docs generate --layout curated --output doc # README.md/Architecture.md/API.md/
                                                  # SequenceDiagrams.md — the shape a developer
                                                  # actually expects, plus one detail page per
                                                  # crate/technology/pipeline/program-entity object
ekos docs generate --layout solution-architect --output doc-sa
                                                  # DependencyRiskReport.md/OnboardingGuide.md/
                                                  # FindingsMemo.md — a team-handoff bundle: real
                                                  # declared dependency versions and concentration
                                                  # risk, a first-day repository-layout guide, and
                                                  # an actionable findings list (unresolved
                                                  # dependencies, undeclared crate versions, missing
                                                  # doc-comment coverage). `--prose` layers an
                                                  # LLM-written executive summary on the findings
                                                  # list, never replacing the deterministic list
                                                  # underneath it.
```

`--layout curated`'s `Architecture.md` includes a real crate/workspace dependency graph (parsed
`Cargo.toml`, not guessed, annotated with a C4 mapping — crate → Container, external dependency →
External System, RFC 0065), external technology dependencies, an `## Open Questions` section
listing real knowledge gaps a deterministic pass couldn't resolve (e.g. an unresolvable workspace
dependency) rather than dropping them silently, CI/CD pipelines (parsed `.github/workflows/*.yml`),
and an entity-relationship diagram; `API.md` lists real functions/
structs/enums/traits (from `RustSymbol`/`PythonSymbol` objects, RFC 0038/0040/0041) grouped by
file, each linked to its own detail page; `SequenceDiagrams.md` covers both Transformation-IR
data-flow sequences and real function-call sequences (RFC 0041's `Calls` graph). Per-entity pages
nest under `entities/<kind>/<2-char shard>/` so a large codebase's page count never blows past
GitHub's per-directory file-listing cap — running `ekos docs generate --layout curated --output
doc` against this repo's own source is a ready example to try. Each entity page's Definition
section now shows the real human-written documentation from source when the analyzer found any
(`///` doc comments, Python docstrings, `@moduledoc`/`@doc`, JSDoc — RFC 0087), honestly stating
"Not documented in source" rather than fabricating one when it didn't. `--prose` (opt-in) layers an LLM-written overview onto each
object page, reusing `ekos ask`'s exact grounding+citation pipeline, with a token-cost estimate
shown before any call.

`Architecture.md`'s Executive Summary now surfaces two more real, deterministic signals instead of
placeholder text: **Major risks** lists real "Observed Concentration Risk" objects — any object
with 3 or more real compiled `DependsOn` dependents, a structural single-point-of-failure
candidate, never an LLM-guessed severity score (RFC 0094) — and **Architecture confidence** shows
the same real completeness/evidence-coverage score `ekos architecture investigate` computes (RFC
0065 Phase 3), now also run from the plain `docs generate` path instead of only the investigation
loop (RFC 0095). Both say so honestly when there's no real signal to compute from yet, rather than
showing a misleading 100%.

### Architecture reasoning + investigation loop (RFC 0065/0066/0067, opt-in)

Beyond deterministic extraction, `ekos architecture investigate` runs the RFC 0066 MVP agentic
loop: broad collection, deterministic crate-topology extraction, one batched LLM call classifying
each crate's architectural role (`ArchitectureReasoningPass`, RFC 0065 Phase 2), a deterministic
evaluator scoring completeness and evidence coverage (no LLM — RFC 0065 Phase 3), and — for any
crate the evaluator flags unclassified — a targeted second pass that reads that crate's own leading
doc comment for more context before trying again. Stops early once the quality threshold is met, or
after `--max-iterations`, always ending with a curated-docs `docs generate` run:

```bash
ekos architecture investigate                                 # RFC 0066 MVP defaults: 3 iterations,
                                                                # 0.90 quality threshold, --output doc
ekos architecture investigate --max-iterations 5 --quality-threshold 0.95 --output doc
```

Reuses the `[llm]` provider already configured in `ekos.toml` (local Ollama or a cloud provider) —
no separate `--llm` flag. See RFC 0067 for what's deliberately out of scope for this MVP
(persistent checkpointing/resume, concurrency-safety infrastructure, CI/CD exit codes, multi-format
output).

Because the LLM-classified crate role is a real judgment call, not a deterministic fact, two
follow-on commands treat it accordingly rather than silently trusting or silently re-deriving it:

```bash
ekos architecture diff --since <timestamp>   # real id-set comparison of technologies, crate role
                                              # classifications, risks, and open questions between
                                              # two points in time — not a fuzzy match (RFC 0108)
ekos architecture review                     # list/confirm/reject pending role classifications;
                                              # a confirmed-or-rejected review status survives the
                                              # next `ekos commit` even though the underlying claim
                                              # is content-signature-versioned and gets re-derived
                                              # on every run (RFC 0109)
```

`ekos_architecture_diff` and `ekos_architecture_review` expose both over MCP too — see the AI agent
access section below.

### LLM-backed compile-time descriptions (RFC 0088, opt-in)

Unlike `--prose` above (render-time, re-spent on every `docs generate` call), `[llm-description]`
in `ekos.toml` runs at `commit` time and persists real, evidence-grounded `ai_overview`/`ai_usage`
properties straight into the ledger — queryable through `ekos ekl`/`ekos ask`/MCP the same as any
other compiled knowledge, not just rendered once. Covers every `Module`/`Rollup`/`Crate`, and every
`Symbol` with a compiled `source_span` (Rust and Elixir today), regardless of whether RFC 0087
already found a real doc comment — a doc comment is real input to the prompt, not a skip condition.
When one exists, a new `ai_comment_check` property (`consistent`/`stale`/`incomplete`) flags a real
discrepancy between what the comment claims and what the code actually does, rendered as a visible
callout right on the entity page's Definition section — never silently trusted, never overwritten.
A single project-level call also fills `Architecture.md`'s `Purpose`/`Architecture style` fields
when real signal (a README, compiled subsystems, compiled technologies) exists to ground them.

```toml
[llm-description]
enabled = true
scope = "modules"   # "modules" (default, cheapest) | "symbols" | "all"
```

```bash
ekos commit          # shows a real call-count estimate, asks to confirm before any spend
ekos commit --yes    # skip the confirmation prompt
```

Opt-in and cost-gated like `[architecture-reasoning]` — a real, potentially large spend (~900 real
LLM calls at the default `scope = "modules"` against a real mid-size codebase, ~5x that at
`scope = "all"`), never defaulting to the more expensive tier just because it was turned on.

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
`## Subsystems` section (see above) — running the same `docs generate` command against this
repo's own source produces one rollup per crate/plugin (46 at last count).

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
server over stdio (RFC 0013) — tools: `ekos_search`, `ekos_ekl` (EKL supports point-in-time `AS
OF <timestamp>` queries and `COUNT`/`GROUP BY` aggregation — RFC 0096), `ekos_neighborhood`,
`ekos_state`, `ekos_dependents` (single-hop impact analysis), `ekos_impact` (directed,
kind-filtered, multi-hop impact tracing — RFC 0018), `ekos_diff` (raw ledger-entry changes since
T), `ekos_status`, `ekos_transformation_explain`/`ekos_transformation_diff` (Transformation IR
explanation and migration diffing — RFC 0028), `ekos_architecture_evaluate`/
`ekos_architecture_drift`/`ekos_architecture_diff` (real completeness/evidence-coverage scoring,
documentation drift, and a real architecture-level diff between two points in time — technologies,
crate role classifications, risks, open questions — distinct from `ekos_diff`'s raw entry report;
RFC 0065/0068 §55/RFC 0107-0108), and `ekos_identity_review`/`ekos_architecture_review` (confirm or
reject a cross-system identity match, or an LLM-classified crate role claim — RFC 0029/RFC 0109,
the two write-capable tools; every other tool reads only the local ledger). Long-lived server
sessions reuse one cached, read-only ledger handle across calls without ever blocking a concurrent
`ekos build`/`commit` in another process (RFC 0097). Every read tool (and `ekos ekl` run from the
CLI) appends one line to `.ekos/query-log.jsonl` — a real usage log the previous designs had no
equivalent of, groundwork for a future materialized-views pass (RFC 0114); a static heuristic
classifies each call cheap/expensive from its own arguments and opportunistically caches an
expensive one's result for an identical repeat while the workspace hasn't changed underneath it. A
gated `ekos_clickhouse_query` tool (RFC 0056) is also available, off by default — see the
ClickHouse connector section below. Connect Claude Code with:

```bash
claude mcp add ekos -- ekos --config /path/to/ekos.toml mcp serve --workspace /path/to/workspace
```

The server also honors `EKOS_WORKSPACE` and `EKOS_CONFIG` environment variables, so a
registration can be path-free: `claude mcp add ekos --env EKOS_WORKSPACE=/path/to/workspace -- ekos mcp serve`.

#### TCP transport — one server, multiple clients (RFC 0115)

Stdio mode spawns a fresh `ekos mcp serve` process (and a fresh cached ledger handle) per client,
which is fine for one tool but wasteful the moment a second one wants to talk to the same
workspace — a second Claude Code session, PyCharm's AI chat, or any other MCP-speaking tool.
`--tcp <addr>` starts a second, additive transport on the same command: a plain NDJSON-over-TCP
socket that any number of clients can connect to concurrently, each getting its own
`std::thread::spawn`'d connection and its own independent cached ledger handle (not shared across
connections — RFC 0115's Concurrency model section explains why). Stdio stays the default and is
completely unaffected when `--tcp` is omitted; passing it just adds the second transport alongside.

**Local — multiple tools on the same machine.** Bind loopback and point every local tool at it:

```bash
ekos mcp serve --workspace /path/to/workspace --tcp 127.0.0.1:7331
```

Any MCP client on that machine that supports connecting to a raw TCP socket (rather than spawning
its own subprocess) points at `127.0.0.1:7331` instead of a spawn command. Verify the server is
actually answering before wiring up a client:

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}\n' \
  | nc 127.0.0.1 7331
```

A one-line JSON-RPC response (`"serverInfo":{"name":"ekos", ...}`) confirms the server is up and
speaking the protocol correctly.

**Remote — a client on a different machine.** There is **no authentication or TLS** on this
transport (RFC 0115's explicit v1 scope) — binding an externally-reachable address exposes the same
read surface stdio gives a spawning parent process, plus the two write-capable tools, to anyone who
can reach it. Two safe ways to do this:

- **Trusted private network only**, if the workspace machine and every client already sit on one
  (e.g. a home LAN, a VPN, a locked-down VPC): bind the interface facing that network instead of
  loopback, e.g. `ekos mcp serve --workspace /path/to/workspace --tcp 0.0.0.0:7331`, and firewall
  the port to that network explicitly — never expose it to the open internet.
- **SSH tunnel (recommended for anything crossing an untrusted network)**, keeping the server itself
  bound to loopback on its own machine:
  ```bash
  # on the workspace machine
  ekos mcp serve --workspace /path/to/workspace --tcp 127.0.0.1:7331

  # on the client machine
  ssh -N -L 7331:127.0.0.1:7331 user@workspace-host
  ```
  The client then connects to its own `127.0.0.1:7331`, tunneled over SSH's encrypted, authenticated
  channel — the EKOS server itself never has to bind or trust anything beyond loopback.

Both `EKOS_WORKSPACE`/`EKOS_CONFIG` env vars and `--config` still apply the same way they do for
stdio mode; `--tcp` only changes how clients connect, not which workspace is served.

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
grows into its own product surface remains a further, explicitly **not-yet-committed** idea, not a
decided roadmap direction — revisited RFC by RFC rather than assumed to keep expanding.

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

File-based ClickHouse DDL (`.sql` files routed to the `"clickhouse"` dialect via RFC 0031's
`[[recover.sql.dialect-rules]]`) goes through the same `sqlparser::dialect::ClickHouseDialect` this
connector uses for its live SELECT-only gate. `sqlparser` never supported several real ClickHouse
`CREATE TABLE` clauses at all — `CODEC(...)` (RFC 0057), and `INDEX ... TYPE ... GRANULARITY`,
`PARTITION BY`, `SAMPLE BY`, `SETTINGS`, and whole `CREATE DICTIONARY` statements (RFC 0058) — found
and closed while using EKOS to document a real open-source repo's ClickHouse schema
(Plausible Analytics). `ClickHouseDialectParser::preprocess` strips each, well-formed occurrences
only, before the SQL reaches `sqlparser`; live-verified against that real repo's full,
unmodified `structure.sql`, which now compiles into real `Table` KIR objects with zero parse
warnings.

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

Deck, verified live against a real ClickHouse container: [ClickHouse: Compiled Metadata + Live
NL-to-SQL](https://alexeyban.github.io/EKOS/presentations/clickhouse-connector.html).

A second deck covers the same connector's file-based DDL path, run cold against a real
open-source repo (Plausible Analytics) EKOS had never seen: [EKOS Cold Against Plausible's
ClickHouse Layer](https://alexeyban.github.io/EKOS/presentations/analytics-clickhouse-cold-run.html)
— git/CI/dependency knowledge compiled cleanly, and real gaps surfaced in `sqlparser`'s
`ClickHouseDialect` (`CODEC`, `INDEX`, `PARTITION BY`, `SAMPLE BY`, `SETTINGS`, `CREATE
DICTIONARY`), all since closed (RFC 0057, RFC 0058) — that same repo's full `structure.sql` now
compiles cleanly into real `Table` objects.

A third deck, [ClickHouse Extraction, After the
Fix](https://alexeyban.github.io/EKOS/presentations/analytics-clickhouse-after.html), re-analyzes
the same repo after both RFCs: 15/15 real tables now recover with zero parse warnings, but
re-analyzing surfaced a second, unrelated finding in a different pipeline stage — identity
resolution (`crates/identity`) over-merging 6 of those 15 real `imported_*` tables into one
identity at confidence 0.93, because they share both a name prefix and a common 8-column "spine."
Reported the same way the parser gap was, not silently fixed.

The same case study also produced real generated documentation and two live demos, all against
[github.com/plausible/analytics](https://github.com/plausible/analytics), a real unmodified
open-source repo:

- [ClickHouse Component — Generated Documentation](https://alexeyban.github.io/EKOS/generated/analytics-clickhouse-component.html)
  — full schema, write/read paths, and data-migration framework for the event store, researched
  using EKOS's compiled ledger plus direct source verification.
- [Top Referrers Dashboard](https://alexeyban.github.io/EKOS/generated/analytics-referrers-dashboard.html)
  — a real analytics dashboard reproduced from a screenshot; every number is a live query, built by
  a local Ollama model against EKOS's compiled schema and run against a real ClickHouse server.
- [Why That Day Spiked](https://alexeyban.github.io/EKOS/generated/analytics-why-high-day.html) —
  an open-ended "why" question answered by chaining real `ekos_clickhouse_query` MCP calls over
  stdio JSON-RPC (including a real failure and retry), plus a technical breakdown of how Claude,
  MCP, and EKOS's pipeline fit together.

A fourth deck, [Proving the Core Loop, Cold, on a Real
Repo](https://alexeyban.github.io/EKOS/presentations/analytics-full-loop.html), goes past the
ClickHouse slice: a genuinely cold `init → build → recover → resolve → compile → commit` run over
the *whole* 2,045-file repo, timed stage by stage (~107s end to end), plus a real `ekos ask` + MCP
question set graded against ground truth read from the repo itself. It found three new gaps in one
sitting — a previously-unknown Postgres `sqlparser` failure (`INCREMENT`), identity resolution
over-merging real people and unrelated documents (not just ClickHouse tables — a real contributor's
own commit becomes unfindable under their own name), and a retrieval-brittleness bug in `ekos ask`
itself (full-sentence questions return no context even when the object is trivially findable by
keyword) — all reported the same honest way, not silently patched or hidden, and all fixed the same
day (RFC 0059, RFC 0060, RFC 0061 — see `devlog_61.md`), each with a live re-verification against
the same real repo, not just a passing unit test.

### Demo: skills + custom subagents (archived)

An earlier twelve-act scripted demo of EKOS's Claude Code integration (two skills, six custom
subagents) is archived under `archive/demo/` for historical reference — no longer actively
maintained against current CLI behavior, so treat it as a record of what once worked rather than
a runnable walkthrough.

### Compact storage (RFC 0015)

Workspaces created before RFC 0015 can be shrunk in place (both commands verify before
touching anything and leave backups):

```bash
ekos ledger status --storage   # per-component size report (or the shorter `ekos status --storage`)
ekos ledger migrate            # ledger v1 → v2: dictionary-zstd payloads (~2.5x smaller)
ekos artifact repack           # loose JSON files → packed segments (~7x smaller on disk)
```

`ekos status [--storage]` (RFC 0116) is a top-level alias for `ekos ledger status` — same output,
shorter to type; both forms stay supported.

### Fact-segment engine (RFC 0016) — the default for new workspaces

A **brand-new** workspace (`ekos init`, nothing written yet) now runs on the fact-segment engine
(EAV facts, immutable segments, tantivy search, mmap'd reads) by default, as of 2026-08-21 — every
version is signature-verified, and it's real, not aspirational: the RFC's storage gate was amended
with measurements in hand (≤2× of the v2 ledger at equal-or-better read latency — it passes at
1.66× with 19× faster search), and the default switch itself waited on a real month-long soak
period on a live, actively-used multi-project estate before flipping (RFC 0016's own dated
section has the evidence). Any **pre-existing** SQLite-backed workspace is completely unaffected —
it keeps serving from SQLite forever unless explicitly migrated. `ekos ledger migrate --v3`
migrates an existing SQLite workspace onto the fact engine — the SQLite source is left untouched,
and deleting `.ekos/ledger/facts/` rolls back.

Since that default switch, the fact engine has picked up three further hardening passes, all
opt-in-free and automatic on any fact-engine workspace:

- **Concurrency safety** (RFC 0104) — writes take a real cross-process file lock (`fs4`) instead of
  assuming a single writer, and multi-step writes run inside a transaction that rolls back cleanly
  on failure rather than leaving a half-written segment.
- **Self-healing search + `ledger repair`** (RFC 0103/0105) — a stale or corrupted tantivy schema
  is detected and rebuilt automatically on open; `ekos ledger repair` additionally re-verifies every
  sealed segment's signature and reports (or fixes) any that fail.
- **Version-chain checkpoints** (RFC 0106) — periodic checkpoints into `checkpoints.jsonl` bound how
  far back a version-chain read has to walk, keeping `object_at`/point-in-time reads fast as ledger
  history grows.

### Partitioned storage (RFC 0111 Phase A) — opt-in

A **brand-new** workspace can opt into a partitioned store by setting `[storage.partition]` in
`ekos.toml`:

```toml
[storage.partition]
dimension = "entity-kind"   # partition by ObjectKind (Table, File, …); relationships by kind
time-bucket = "monthly"     # "daily" | "weekly" | "monthly"
```

Data then splits across many independent fact-segment ledgers keyed by kind + time bucket, with a
persisted catalog and a run-file index so a reopened store resolves any object/relationship with no
partition scan; aged partitions tier to cold (handle evicted, promoted back on read). It is a
drop-in for the single-ledger backend — every command, the MCP server, and `docs generate` work
unchanged. Existing SQLite or fact-engine workspaces are **never** switched implicitly, same rule
as the fact-engine default.

**Multi-machine distribution (Phase B, RFC 0113)** is being built incrementally. Landed so far:

- a `SegmentBackend` seam — `LocalFsBackend` (default) or `ObjectStoreBackend` (S3 / Azure /
  in-memory, behind a feature flag);
- a **coordinator** (`ekos coordinator serve`) that hands out fencing-tokened write leases and
  tracks per-partition commit watermarks over newline-delimited JSON-RPC;
- a **compile worker** (`ekos compile-worker run`) that runs the real
  `build → recover → resolve → compile → commit` pipeline under a coordinator lease, then
  registers the partitions it wrote and commits the new generation;
- **self-describing object-storage partitions** — `[storage.partition] segment-backend-url =
  "s3://…"` routes each partition's sealed segments, `manifest.json`, `dict.bin`, and search index
  to S3/Azure; only the active segment and a small `HEAD` watermark stay local to the writer;
- **query workers** (`ekos query-worker serve`) that pull a partition into a local cache and serve
  reads for it, and a **`DistributedLedger` gateway** that implements the same `KnowledgeStore`
  trait every command already uses — fanning reads across the workers and merging — so pointing a
  workspace at a cluster is just `[storage.distributed]` in `ekos.toml`:

  ```toml
  [storage.distributed]
  coordinator   = "coordinator.internal:7333"
  query-workers = ["qw1.internal:7334", "qw2.internal:7334"]
  ```

- **distributed search** — the gateway fans each shard's BM25 top-*k* to a worker and merge-sorts
  the results (shard-local term statistics, the standard query-then-fetch approximation);
- **a pooled, concurrent, pruned gateway** — `DistributedLedger` reuses one connection per
  coordinator/worker instead of reconnecting per call, fans a multi-partition read out
  concurrently instead of one partition at a time, and prunes id-scoped reads (`get_object` and
  friends) to the few partitions the coordinator's index says actually hold that id.

That completes Phase B at its v1 scope, with no tracked follow-ons remaining. None of this affects
Local mode, which stays the default.

## Development Process

All significant architectural decisions begin as RFCs in `docs/rfcs/`. No feature is implemented until its RFC is accepted. See `CLAUDE.md` for the full mandatory development workflow.

## Presentations

Live decks at [alexeyban.github.io/EKOS](https://alexeyban.github.io/EKOS/presentations.html) — every claim in them is reproduced live against real repos, not staged:

- [Claude Code + EKOS](https://alexeyban.github.io/EKOS/presentations/claude-code-with-ekos.html) — how Claude Code searches and analyzes a codebase through EKOS's MCP server instead of raw grep/Read, with a measured with-vs-without comparison and real token/usage numbers.
- [The AI-Native Enterprise Knowledge Compiler](https://alexeyban.github.io/EKOS/presentations/ai-native-knowledge-compiler-pitch.html) — the startup pitch, audited live by Claude Code using EKOS's own MCP server.
- [ClickHouse: Compiled Metadata + Live NL-to-SQL](https://alexeyban.github.io/EKOS/presentations/clickhouse-connector.html) — the one explicit, audited exception to "AI never touches raw enterprise systems directly," verified live against a real ClickHouse container, honest failures included.
- [GitHub, Live, End to End](https://alexeyban.github.io/EKOS/presentations/github-live-cross-system.html) — the GitHub connector's first live run, 1,600 real issues/PRs from a real repo: two known gaps fixed before the run, a third (96% of items collapsing into one identity) found only at real scale and fixed the same session, and the residual limitation reported honestly, not hidden.
- [Vision & Token Utility](https://alexeyban.github.io/EKOS/presentations/vision-and-token-utility.html) — why the EKOS token's relevance is designed to grow as a consequence of platform adoption, not a promise of price.

See [alexeyban.github.io/EKOS/presentations.html](https://alexeyban.github.io/EKOS/presentations.html) for the full list.

## Token & Community

EKOS has a community token whose utility is designed to grow as the platform is adopted — a
consequence of usage, not a promise of price. Network, contract address, and full allocation are
the canonical facts in [TOKENOMICS.md](TOKENOMICS.md); the phased utility roadmap is in
[VISION.md](VISION.md). Release announcements post to X: [@ekosproject](https://x.com/ekosproject)
(via `ekos marketing publish`, RFC 0030).

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

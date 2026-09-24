# EKOS — Enterprise Knowledge Operating System

[![Maintainability Rating](https://sonarcloud.io/api/project_badges/measure?project=alexeyban_EKOS&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=alexeyban_EKOS)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=alexeyban_EKOS&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=alexeyban_EKOS)
[![Security Rating](https://sonarcloud.io/api/project_badges/measure?project=alexeyban_EKOS&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=alexeyban_EKOS)
[![Quality gate](https://sonarcloud.io/api/project_badges/quality_gate?project=alexeyban_EKOS)](https://sonarcloud.io/summary/new_code?id=alexeyban_EKOS)

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
`scheduler`, `sql-dialect-sdk`, `ledger`, `runtime`, `identity`, `recovery`, `ekl`, `semantic`,
`marketing`, `docs-gen`, `dbt-gen`, `common`, `cli`, `demo-server`, `segment-backend` / `cluster` /
`distributed` (RFC 0111/0113's partitioned + horizontally-distributed storage — see below),
`simulation` (RFC 0047-0055's opt-in World Engine — see below), `clickhouse-query` (RFC 0056's
opt-in live NL-to-SQL query engine — see below), and `evals` (RFC 0138's answer-grading harness — see
below). The same workspace also builds `ekos` with a generic extension seam (RFC 0149) that out-of-tree
builds plug extra connectors and MCP tools into.

**Connectors (`ekos/plugins/`):** File, Git, GitHub issues/PRs (live-verified against a real
repo, 1,600 real issues/PRs — RFC 0062), Confluence, local documents
(PDF/DOCX/text/Markdown/HTML/email — text, tables, image OCR), Pentaho Kettle (`.ktr`/`.kjb` —
RFC 0027), Python/PySpark source (real AST parsing, DataFrame chains recovered into the
Transformation IR — RFC 0038/0040), Rust source (real AST parsing, real function-call graph —
RFC 0041), Elixir and JavaScript/TypeScript source (real AST + `Calls` recovery — RFC 0081/0082),
Perl source (`.pl`/`.pm`/`.t`/`.psgi`, plus `.cgi` with a real `perl` shebang — packages, subs,
`use`/`require` dependencies, `use parent`/`use base`/`@ISA` inheritance, POD descriptions and
source spans; RFC 0147), **compiled .NET and JVM binaries with no source available**
(`.dll`/`.exe`/`.class`/`.jar`/`.war`/`.ear` — types, methods, signatures, fields, the real call
graph, branch structure, string/numeric constants and external I/O boundaries, read in-process
from CLI metadata and bytecode with no .NET SDK or JRE required; for .NET, per-method statement
recovery — `if`/loop/`switch`/`try` with the IL offset of every line, labelled with the fidelity actually
reached — plus a per-method migration spec, a Python-rewrite parity check and sandboxed characterization tests that
run the original and check the rewrite against what it did, RFC 0150; RFC 0148 — shipped
as a separately licensed extension build, not part of this open-source repository, RFC 0149), ClickHouse (real HTTP client, schema metadata plus an opt-in live query engine — RFC
0056), crypto/DeFi export, plus scaffolded proof-of-concept clients for Salesforce, SAP, Oracle,
Microsoft Fabric, and Snowflake (real API shapes, mock-tested — none yet exercised against a live
account). SQL in the PostgreSQL, SQL Server (T-SQL), MySQL, Snowflake, Databricks and ClickHouse dialects is
parsed from files (RFC 0031); live PostgreSQL / SQL Server database connectors and a Jira connector remain planned.

## Installation

### Prebuilt binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh
```

Installs a single `ekos` binary into `~/.local/bin` (override with `EKOS_INSTALL_DIR`). No Rust
toolchain, no compiler, nothing else to install. The script resolves the latest release, verifies
the download against the release's `SHA256SUMS` **before** unpacking it, and never uses `sudo`.

Piping a script into a shell is a real supply-chain decision, so the script is short enough to
read first:

```bash
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh -o install.sh
less install.sh && sh install.sh
```

The script also takes `EKOS_VERSION` to pin a release (`EKOS_VERSION=v1.0.3 sh install.sh`) and
`EKOS_INSTALL_DIR` to change where it lands.

### Prebuilt binary, downloaded by hand

If you would rather not pipe a script into a shell, every release is a plain archive on the
[releases page](https://github.com/alexeyban/EKOS/releases). Pick the one for your machine:

| Your machine | Asset |
|---|---|
| Linux, Intel/AMD 64-bit | `ekos-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux, Intel/AMD, old or unusual distro | `ekos-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Linux, ARM 64-bit (Raspberry Pi 4+, Graviton, Ampere) | `ekos-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS, Apple Silicon (M1 and later) | `ekos-<version>-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `ekos-<version>-x86_64-apple-darwin.tar.gz` |
| Windows, 64-bit | `ekos-<version>-x86_64-pc-windows-msvc.zip` |

`uname -sm` tells you which you are on if you are not sure: `Linux x86_64`, `Darwin arm64`, and
so on.

**glibc or musl?** Take the `gnu` build first — it is the faster of the two. If it refuses to
start with something like `version 'GLIBC_2.x' not found`, your distribution is older than the
one the release was built on: take the `musl` build instead, which is fully static and depends on
nothing. (`install.sh` does this fallback automatically.)

#### Linux and macOS

```bash
VERSION=v1.0.3
TARGET=x86_64-unknown-linux-gnu          # from the table above
ASSET="ekos-${VERSION#v}-$TARGET.tar.gz"
BASE="https://github.com/alexeyban/EKOS/releases/download/$VERSION"

curl -fsSLO "$BASE/$ASSET"
curl -fsSLO "$BASE/SHA256SUMS"

# Verify before unpacking. Feeding one line to -c keeps it quiet about the five assets
# you did not download, and works with both GNU sha256sum and macOS's shasum.
grep "$ASSET" SHA256SUMS | sha256sum -c -        # macOS: shasum -a 256 -c -

tar xzf "$ASSET"
mkdir -p ~/.local/bin                    # install(1) will not create it for you
install -m 755 "ekos-${VERSION#v}-$TARGET/ekos" ~/.local/bin/ekos
```

That prints `<asset>: OK` and exits 0. Anything else — `FAILED`, or nothing matched — means do
not unpack it; open an issue instead.

Worth being precise about what this proves: releases are checksummed but **not** signed — there
is no Sigstore/cosign signature or SLSA provenance yet. `SHA256SUMS` tells you the archive
arrived intact and matches what the release publishes; it does not, on its own, prove who built
it.

`~/.local/bin` needs to be on your `PATH`; add `export PATH="$HOME/.local/bin:$PATH"` to your
shell profile if `ekos --version` comes back "command not found". Any directory on `PATH` works —
`/usr/local/bin` needs `sudo`, which is the only reason it is not the default here.

**macOS, one extra step.** These binaries are not code-signed or notarised. A file downloaded
with a *browser* is quarantined by Gatekeeper and refuses to run ("cannot be opened because the
developer cannot be verified"). Clear the flag on the extracted binary:

```bash
xattr -d com.apple.quarantine ~/.local/bin/ekos
```

Downloading with `curl` — as above, and as `install.sh` does — does not set that flag, so this
step only applies if you clicked the link in a browser.

#### Windows

```powershell
$Version = "v1.0.3"
$Asset   = "ekos-$($Version -replace '^v','')-x86_64-pc-windows-msvc.zip"
$Base    = "https://github.com/alexeyban/EKOS/releases/download/$Version"

Invoke-WebRequest "$Base/$Asset" -OutFile $Asset
Invoke-WebRequest "$Base/SHA256SUMS" -OutFile SHA256SUMS

# Verify before unpacking.
$actual = (Get-FileHash $Asset -Algorithm SHA256).Hash.ToLower()
$line   = Select-String -Path SHA256SUMS -SimpleMatch $Asset
if (-not $line) { throw "$Asset is not listed in SHA256SUMS - do not use this file" }
$expected = $line.Line.Split()[0]
if ($actual -ne $expected) { throw "checksum mismatch - do not use this file" }

Expand-Archive $Asset -DestinationPath .
```

`ekos.exe` is inside the extracted folder. Move it somewhere on your `PATH`, or add that folder
to `PATH`. SmartScreen may warn the first time you run it, for the same reason as macOS: the
binary is unsigned.

(The Linux/macOS block above was run verbatim against the real v1.0.3 release. This PowerShell
one was not — no Windows machine was available — so treat it as carefully written rather than
proven, and please open an issue if it misbehaves.)

### First run

However you installed it:

```bash
ekos --version
cd /path/to/your/repo
ekos init --detect     # writes an ekos.toml that matches what's actually in this repository
ekos doctor
ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
ekos coverage          # confirms every input kind actually produced objects
ekos ask "what does this system do?"
```

`ekos init --detect` is worth using rather than plain `ekos init`: it detects the SQL dialect your
schema is written in, excludes third-party and generated directories that would otherwise be
compiled as if they were your own code, and prints an inventory of what it found. See
[RFC 0152](ekos/docs/rfcs/0152-first-run-self-verification.md).

### Upgrading and removing

Re-running the install command replaces the binary in place with the latest release; there is no
package database and nothing else to update. To remove it, delete the binary
(`rm ~/.local/bin/ekos`) — EKOS keeps no files outside the `.ekos/` directory inside each
workspace you compiled, which you can delete separately.

### From source

**The Cargo workspace root is `ekos/`, not the repo root** — there is no top-level `Cargo.toml`,
so `cargo` commands must be run from inside `ekos/` (or with `--manifest-path ekos/Cargo.toml`).
The toolchain is pinned by `rust-toolchain.toml`; rustup installs the right version automatically.

Prerequisites on both platforms:
- **Rust**, stable channel, via [rustup](https://rustup.rs) — 2024 edition needs rustc 1.85+;
  installing the latest stable is fine.
- **A C/C++ toolchain** — `rusqlite`'s bundled SQLite and the `zstd` crate both compile native C
  source at build time, so a working `cc` is required even though the project itself is pure Rust.
- **Git**, to clone the repo.

#### macOS

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

#### Windows 11

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

#### Verify the install

```bash
ekos --help                # or: cargo run -p ekos -- --help, from ekos/
ekos init                  # creates .ekos/ in the current directory
cargo test --workspace     # optional: run the test suite (1,500+ tests)
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
review via the `ekos_identity_review` MCP tool. **DAO treasury compliance (RFC 0032).** "Was this payment approved by governance?" is the same shape of
problem: two independently observed records with no link between them, where a wrong auto-decision is
worse than none. `EKOS_TREASURY_ADDRESS` + `EKOS_TREASURY_CHAIN_ID` observe a treasury's outgoing
transfers through an Etherscan-family explorer (each sub-transfer of a Safe multi-send is its own
payment); `EKOS_SNAPSHOT_SPACE` observes a Snapshot space's proposals and their outcomes.
`ekos treasury scan` scores payment↔proposal candidates on recipient, amount, text reference and timing
and writes each as an `unconfirmed` `AuthorizedBy` relationship with its evidence — a payment made
*before* its approval is flagged and heavily penalised, a rejected proposal is never offered — then
lists the payments with no candidate. Review with `ekos_identity_review`. **Status:** verified against
mock clients and a full-pipeline fixture; the real explorer and Snapshot clients have not been run live.

Same-source duplicates (`ekos resolve`/`ekos
compile`, e.g. two `Table` objects both literally named `customers`) auto-merge only when the
match is an exact normalized name; anything fuzzy goes through that same `unconfirmed`/review
flow instead of an irreversible merge (RFC 0063) — no confidence threshold on the underlying
scoring formula reliably separates real correct fuzzy merges from real incorrect ones, so an
irreversible auto-merge isn't a safe default for that case.

### Real schema and class structure from source (RFC 0091/0092)

**File-based SQL schema needs a dialect.** `.sql` files parse under the `generic` (ANSI) dialect
unless `ekos.toml` routes them somewhere real:

```toml
[recover.sql]
default-dialect = "generic"

[[recover.sql.dialect-rules]]           # first path-glob match wins
path-glob = "priv/repo/**"
dialect = "postgres"
[[recover.sql.dialect-rules]]
path-glob = "priv/ingest_repo/**"
dialect = "clickhouse"
```

This matters for any real schema **dump** — `pg_dump` output, a ClickHouse `structure.sql`, or
hand-written DDL past plain ANSI. `parse_ddl_structural` parses the whole file in one pass, so a
single statement the dialect can't handle (`CREATE TYPE … AS ENUM`, `CREATE SEQUENCE`,
`ENGINE = MergeTree`, `CODEC(...)`, …) discards *every* table in that file — reported only as a
buried `SQL001: no tables found` warning, never an error. The `postgres` / `clickhouse` dialects
carry the preprocessing (RFC 0057/0058/0059) that makes real dumps parse; `generic` does not. If
`ekos ekl "FIND Object WHERE kind = 'Table' COUNT"` returns 0 for a repo that clearly has a
schema, this rule is missing.

**Hand-written schemas need more than the right dialect (RFC 0146).** Everything above was tuned
against `pg_dump`, which emits a narrow, mechanical subset of PostgreSQL. A human-maintained schema
does not: measured on LedgerSMB, a correctly-configured `dialect = "postgres"` still recovered
**0 of 158 tables**, because the file's first `COMMENT ON TABLE … IS $$…$$` failed the whole-file
parse and took every table with it. The `postgres` dialect now preprocesses `COMMENT ON`,
`INHERITS`, `SECURITY DEFINER`, `RETURNS SETOF`, `:=` named arguments, `DO` blocks, `CREATE RULE`,
psql meta-commands and `COPY … FROM stdin` payloads out of the way; the same file yields 158 tables
and 214 foreign keys.

`COMMENT ON TABLE` / `COMMENT ON COLUMN` text is no longer discarded either — it becomes an
evidence-backed `description` on the `Table` object (and on the matching column), with the source
line recorded. **Author-written text outranks the LLM's:** `description` is the schema's own words,
the model's version is kept alongside as `llm_description`, and `sql_comment` marks the provenance.

`[llm] max-tokens` caps compiler-pass LLM calls — distinct from `[ai] max-tokens`, which governs the
read-side `ekos ask` runtime. Leave it unset and each file's budget is computed from its own schema
size; a new `SQL004` diagnostic reports when enrichment named fewer tables than the file declares,
and says so explicitly when the response hit its ceiling.

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

**Markdown is split by heading (RFC 0144).** Each heading becomes its own searchable `Section` named by
its heading path (`docs/rfcs/0013-mcp-server.md § RFC 0013 — MCP Server › Motivation`) with a real
line range, `doc_type` (`rfc`/`devlog`/`readme`/`claude_md`/`doc`) and, for RFCs, `rfc_number`/`rfc_status`.
`compile` also links docs deterministically: an `RFC NNNN` mention becomes a `References` edge to that RFC,
and a backticked identifier that names exactly one code symbol becomes an edge to it. `ekos ask "what does
RFC 0013 …"` resolves the RFC by number. These edges show up in neighbourhoods but never count as
dependents in `ekos_dependents`/`ekos_impact`.

**Using a hosted OpenAI-compatible model (RFC 0145).** Any Chat Completions host works — OpenCode Zen,
OpenRouter, DeepSeek, Groq, vLLM:

```toml
[llm]
provider = "openai"
base-url = "https://opencode.ai/zen/v1"
model = "deepseek-v4-flash"
api-key-env = "OPENCODE_API_KEY"

[ai]
max-tokens = 8192            # reasoning models spend hidden tokens; 1024/2048 truncate or empty answers
```

For local Ollama, `[llm] context-window = 8192` (the default; also `OLLAMA_NUM_CTX`) is sent as `num_ctx` —
without it Ollama silently truncated long prompts — and EKOS warns when a prompt fills the window.
`ekos doctor` shows the effective window or custom endpoint. Hosted models bill per token; a full
101-scenario `ekos eval run` on DeepSeek V4 Flash costs about $0.08. DeepSeek V4 Flash is a reasoning model whose
hidden reasoning counts against `max-tokens` — use `[ai] max-tokens = 8192` or some answers come back empty.

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

### Compiled-knowledge query engine (RFC 0118 — SEARCH → QUERY → REASON)

Traditional RAG searches documents; EKOS queries **compiled knowledge**. The retrieval stack is
three operations, built as RFCs 0119–0126 (`devlog_143`–`devlog_149`):

- **SEARCH** — one `KnowledgeStore::retrieve` seam behind every consumer, fusing a BM25 arm, an
  exact-name arm, and (opt-in) a vector arm with Reciprocal Rank Fusion (Cormack RRF, `k=60`). BM25
  indexes object names and content with English stemming, so a singular mention ("the customer
  table") resolves the same object a plural exact mention ("the Customers table") already did.
  `ekos query find "<text>" --mode lexical|vector|hybrid`; `--explain` prints the compiled plan
  and per-arm timings (populated on partitioned and distributed stores too, not just a single
  `FactLedger`). The same fused path runs on the single `FactLedger`, the partitioned
  store, and the distributed gateway.
- **QUERY** — direct, zero-LLM answers over the compiled graph: `fact(entity, attr)` for one
  attribute of one object, named ops (`dependents`, `callers`, `path`, …) for the graph. Exposed
  as MCP `ekos_query` (a typed list of source-cited claims) and `ekos_retrieve` ("show your
  work": the plan + evidence set + how the question was understood).
- **REASON** — `ekos ask "<question>"` now *compiles* the question: a rules planner routes it to
  fact lookups and graph traversals, executes them into a typed `EvidenceSet` where every item
  carries its provenance, and the model's job shrinks to "explain this evidence, cite each item."
  `--explain` prints the plan and evidence; `--classic` selects the pre-0123 retrieve-and-dump
  path. A citation not backed by a source id in the evidence set is a reported finding, not a
  formatting detail. Claims cite **a place in a file, not just a file** (RFC 0140): every
  Rust/Python/Elixir symbol with a `source_span` carries real evidence naming its path, its line,
  and the source text itself, rendered as `path:start-end` — so asking what a function does shows
  the model the function body, not only its name. Each Rust/Python/Elixir function or method also
  carries a real `signature` (RFC 0141) — the discriminating term for "what builds an
  `Arc<dyn LlmProvider>`" often lives in a return type, not the function's own name — and a Rust
  `Calls` edge carries `call_count`, `call_site_line`, and `caller_is_test`, so impact analysis can
  weigh 40 production call sites differently from 40 mostly-test ones instead of returning an
  undifferentiated list. For the top few entities in each answer's evidence set, `ekos ask` also
  reads the entity's real, on-demand source text straight from the artifact store — never the live
  filesystem, so it never bypasses redaction (RFC 0140 §3) — giving the model the actual code
  behind a claim, not just a name and a short excerpt. An opt-in `[retrieval] rerank = "llm"`
  (off by default) asks the model to reorder the top candidate evidence items by real relevance
  before it answers (RFC 0140 §4) — never on the deterministic retrieval-ranking path RFC 0126's
  CI gate checks, only inside the REASON answer pipeline itself.

EKL gains `FIND Object SEMANTIC 'text' [LIMIT k]` — the retriever as a candidate-set strategy.
Retrieval quality is CI-gated: `ekos_runtime::retrieval_eval` holds a checked-in graded query set
and a reference estate, and a workspace test fails the build if Recall@10 / MRR / nDCG@10 or
intent-classification accuracy drops more than 2% below the recorded baseline;
`cargo bench --bench retrieval_eval` prints the current scoreboard (RFC 0126).

### The vector arm — semantic search (RFC 0125, opt-in)

A question phrased with none of the target object's words ("the thing that sends welcome emails" →
a function called `dispatch_signup_notification`) has no lexical hook. `[embeddings]` in
`ekos.toml` adds a vector retrieval arm: `ekos commit` embeds every compiled object (its name,
kind, and `ai_overview` prose if present) into a derived `<ledger-dir>/vectors/` index, and
retrieval fuses cosine-nearest hits with the BM25 + exact-name arms (Reciprocal Rank Fusion, the
same fuser as RFC 0120).

```toml
[embeddings]
enabled = true
provider = "ollama"          # "ollama" | "openai" | "mock"; falls back to [llm] provider
model = "nomic-embed-text"   # optional override
api-key-env = "OPENAI_API_KEY"
cache = true                 # content-addressed .ekos/embed-cache/, on by default
```

```bash
ekos query find "sends welcome emails" --mode vector   # semantic only
ekos query find "welcome email" --mode hybrid          # semantic + BM25, fused
```

Opt-in and off by default — with no `[embeddings]` table nothing is embedded and retrieval is the
pure BM25 + exact-name path. Embeddings are cheap and disk-cached, so unlike `[llm-description]`
there is no spend prompt. Single-node only this phase (a no-op on a SQLite or partitioned
workspace); a vector/hybrid search with no index built yet degrades to lexical with a visible
note. The MCP `ekos_search` tool takes the same `mode` and reports `arms_run`. Per-arm wall-clock
timings (`arm_timings`, RFC 0126) ride along on the `ekos_search` / `ekos_retrieve` results, into
`.ekos/query-log.jsonl`, and in `ekos query find --explain`.

### Eval harness (RFC 0138, on-demand)

`ekos eval run` grades whole `ekos ask` answers against a checked-in, 101-scenario suite spanning
seven categories — Architecture, Code, Dependencies, Lineage, History, Security, Adversarial
(`evals/datasets/*.yaml`) — not just retrieval ranking (that's RFC 0126's separate, narrower,
CI-gated `ekos_runtime::retrieval_eval`). Every score is deterministic keyword/id matching — no
LLM judge: did the answer state the expected facts, cite real (non-hallucinated) evidence, cover
what was asked, surface the right objects in the top-10, route to the right REASON planner query
type, and — for the Adversarial category — actually say **"Insufficient evidence"** for a question
with no grounded answer instead of inventing one. That last category is the harness's strictest:
every scenario in it expects a refusal, and an answer that invents a plausible-sounding detail
(a database name, a version number, a person) counts as a hallucination regardless of how
confidently it reads.

Recall@10 is graded against the exact keyword-only query `reason::plan()` actually searches with
(RFC 0139), not the raw natural-language question a `reason`/`ask` scenario asks — the two can rank
differently, so grading the wrong one could pass or fail a scenario for a reason unrelated to what
the model was shown. Lexical search itself also treats a bareword `and`/`or` as connector noise,
not a literal word a document must contain — the same two words this codebase already treats as
English stopwords everywhere else — so an "A or B" style query can match on either real term
without silently requiring the word "or" itself to appear.

```bash
cd ekos
cargo run -p ekos -- eval run --dataset ekos-full   # every category, "evals/" at the repo root
cargo run -p ekos -- eval run --dataset architecture --agent ollama
cargo run -p ekos -- eval run --dataset hallucination   # Category G only
cargo run -p ekos -- eval history                       # every past run, as a trend table
```

```
EKOS EVALUATION
─────────────────────────────

Dataset: architecture
Agent: ollama (llama3:latest)
Runtime: local

Scenarios:                    3
Passed:                       0
Failed:                       3

Answer correctness:       11.1%
Evidence groundedness:   100.0%
Completeness:             11.1%
Recall@10:                  n/a
Hallucination rate:        0.0%

Avg tokens:               1,221
P95 latency:              49.4s

Cache hits:                 0/3
Tokens saved:               n/a
Peak RSS:               69.7 MB
CPU time:                 14.7s

Status: FAIL
```

That example is a real run against this repo's own live self-analysis ledger with the local
`llama3:latest` model — not a simulated one. It is also the *floor*: a workstation-sized local model.

**Full-suite results — `ekos-full`, 101 scenarios, graded deterministically.** The answering model
went from a local llama3 8B on Ollama to **DeepSeek V4 Flash through [OpenCode Zen](https://opencode.ai/zen)**
(an OpenAI-compatible cloud endpoint), and the knowledge EKOS hands it was improved in two further steps. The same
checked-in suite, one change at a time:

| Step | Passed | What changed |
|---|---|---|
| Baseline, local `llama3:latest` (8B) — report `20260914T154459Z` | 42/101 | — |
| + evidence text kept, citation parsing, refusal wording (devlog_183) | 53/101 | Grader and evidence-rendering bugs, not the model |
| **+ DeepSeek V4 Flash** (model swap only) | **70/101** | A larger model, and the local one had been reading a **truncated prompt** — EKOS never sent `num_ctx`, so Ollama dropped the *start* of long prompts, where the instructions live (RFC 0145) |
| + document structure (RFC 0144) | **79/101** | Markdown cut by heading instead of blind 2,500-char chunks, fully indexed; docs link to code and to each other; output limit raised to 8,192 |
| + query-planner fixes (devlog_186) | **87/101** | A dependency phrase in a descriptive clause no longer triggers a graph walk; `crate::path` names resolve as one entity; generic words stop matching unrelated objects |

Seventeen points came from the model and seventeen from engineering; 53 → 87 is **+34**. Per category, before → after:

| Category | Before | After |
|---|---|---|
| Architecture | 9/20 | 18/20 |
| Code | 8/15 | 13/15 |
| Dependencies | 10/12 | 10/12 |
| Lineage | 5/12 | 10/12 |
| History | 3/12 | 11/12 |
| Security | 7/12 | 10/12 |
| Adversarial | 11/18 | 15/18 |
| **Total** | **53/101** | **87/101** |

| Metric | llama3 8B (53/101) | DeepSeek V4 Flash (87/101) |
|---|---|---|
| Answer correctness | 50.1% | **85.7%** |
| Evidence groundedness | 65.9% | **95.6%** |
| Hallucinated answers on adversarial questions | 7 | **3** |
| Recall@10 | 64.7% | 55.9% — *fell* |
| Latency | up to 79 s p95, killed three times for lack of memory on a 15 GB workstation | median 5 s per fresh call |
| Cost of a full 101-question run | free (local) | about **$0.05–0.08** |

Recall@10 fell because documents are now many heading-sections that outrank the crate or file a
retrieval-only scenario expects; answers improved anyway, and section-vs-document ranking is a tracked
follow-up. These numbers were re-verified against the per-category reports saved in
`evals/reports/zen-*/` (`zen-base` 70, `zen-final-8k` 79, `zen-planner2` 87). The full account, including the model-swap
run's 2,048-token output limit that cut off 17 answers (DeepSeek V4 Flash reasons, and its hidden reasoning
counts against the limit — so the model's own contribution is, if anything, understated), is in
[EKOS from 53 to 87](https://alexeyban.github.io/EKOS/presentations/deepseek-53-to-87.html) and
devlog_183 / 185 / 186.

**What still fails — 14 of 101, named rather than averaged away:**

| Cause | Scenarios |
|---|---|
| The model correctly rejects a false premise but not in refusal wording the grader accepts | adv-004, adv-011, adv-015 |
| The answer never reaches the top of the retrieved evidence | code-002, lin-008, dep-004 |
| The fact isn't recorded yet (a workspace-inherited Rust edition, struct fields as claims, no caller reached the evidence) | code-004, lin-007, dep-005 |
| Retrieval-only scenarios graded on recall@10 (see above) | arch-009, hist-007 |
| Right in substance, wrong keyword | arch-020, sec-002, sec-010 |

**Reproduce it, with your own model:**

```bash
export OPENCODE_API_KEY=...        # or any OpenAI-compatible endpoint: set base-url/model in the config
cargo run -p ekos -- --config ../ekos.zen.toml eval run --dataset ekos-full --save-answers
cargo run -p ekos -- eval history  # trend table across every saved run
```

```toml
# ekos.zen.toml — hosted, or a self-hosted OpenAI-compatible server (vLLM, llama.cpp) inside the network
[llm]
provider    = "openai"
base-url    = "https://opencode.ai/zen/v1"
model       = "deepseek-v4-flash"
api-key-env = "OPENCODE_API_KEY"

[ai]
max-tokens  = 8192   # a reasoning model: hidden reasoning counts against this

# local Ollama: set the window explicitly — a silent default truncation cost 17 points
# [llm]
# provider = "ollama"
# context-window = 32768
```

**Closed environments.** Compiling needs no GPU, no network and no model; answer quality tracks the model you
can serve. A workstation-sized local model lands near the llama3 column; reaching the DeepSeek column without a
cloud API means self-hosting a much larger open-weights model on dedicated GPU servers, with a context window of
at least 32k tokens and enough throughput to answer in seconds. Prompts contain real source excerpts — EKOS
redacts secrets and PII before anything reaches the ledger, but a hosted model still sees your code (Zen's paid
models are zero-retention; its free models may use prompts for training).

A local model is real and free to run, but it is not the reference: `ekos eval run` refuses outright rather than
silently grading against the stub `MockLlmProvider` when the configured provider's API key isn't set (there is no
`--agent mock` option). Grading is unchanged across every step above — keyword and id matching at temperature 0,
unchanged prompts replayed from the LLM cache — so a score that moved is attributable to a real change.

*Earlier history.* With llama3 held fixed the suite went 39 → 43 (RFC 0140/0141), then 42 → 53 against the
`20260914T154459Z` baseline (devlog_183), mostly because the harness found something unflattering: an adversarial-question fabrication count that got *worse* (3 → 6) once
third-party noise was gone and real objects made a wrong answer read more convincingly; a bit-identical re-run
after RFC 0139/0140/0141 that confirmed `temperature: 0` makes an unchanged prompt reproducible; and
recall@10 grading that had silently graded the wrong query (RFC 0139, devlog_182). Details: devlog_182, devlog_183.

Those numbers moved mostly because the harness found something unflattering: **94% of what this
repo was compiling wasn't its own code.** A Python virtualenv and two `.scannerwork/` directories
had never been excluded from the observation walk — `.gitignore` does not filter it. Excluding
them took the corpus from ~13,700 files to 834 and the model from 53,830 objects to 12,283, while
`recover` produced *identical* symbol counts (2,623 Rust symbols, 1,734 `Calls` edges) — all
signal kept, all noise gone. See [devlog_174](devlogs/devlog_174.md) and the
[eval comparison report](https://alexeyban.github.io/EKOS/presentations/eval-comparison-report.html). Beyond the five headline scores, every run also captures real resource usage: **tokens
saved** is genuine cache-hit attribution (`CachedLlmProvider` tracks hits/misses; a scenario whose
answer came from the disk cache instead of a fresh network call is diffed and counted, not
estimated), and **CPU time / peak RSS** are best-effort, Linux-only, read straight from
`/proc/self/{stat,status}` — honestly `n/a` off-Linux or when unavailable, never fabricated.

Every run saves a timestamped JSON report to `evals/reports/` and exits non-zero when the
aggregate gate (`ekos_evals::report::GateThresholds`) misses — deliberately **not wired into CI**
(real LLM calls against a real workspace, not free/fast/deterministic enough for every PR yet).
`ekos eval history` reads every saved report back and renders one line per run, newest last — the
"run history" a saved-JSON-per-run design gives for free. See `evals/README.md` for the scenario
schema and category breakdown, and `ekos/docs/rfcs/0138-eval-harness.md` for the full design.

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
server over stdio (RFC 0013), a raw TCP socket (`--tcp`, RFC 0115), or Streamable HTTP
(`--http`, RFC 0143) — tools: `ekos_search` (`limit` param — RFC 0124; `mode`
`lexical`/`vector`/`hybrid` for semantic matching, plus `arms_run` in the response — RFC 0125),
`ekos_query` /
`ekos_retrieve` (compiled fact + graph answers and the inspectable query plan / evidence set, no
LLM — RFC 0124), `ekos_ekl` (EKL supports point-in-time `AS OF <timestamp>` queries,
`COUNT`/`GROUP BY` aggregation — RFC 0096, and `SEMANTIC 'text'` retrieval candidate sets — RFC
0124), `ekos_neighborhood`,
`ekos_state`, `ekos_dependents` (single-hop impact analysis), `ekos_impact` (directed,
kind-filtered, multi-hop impact tracing — RFC 0018), `ekos_graph_export` (the whole compiled
graph as nodes + edges in one call — filtered, optionally collapsed to super-nodes, truncation
reported; RFC 0127), `ekos_diff` (raw ledger-entry changes since
T), `ekos_audit` (the write history of one object/relationship with the pipeline run, stage, and
source artifact behind each version — RFC 0135; the source artifact is the object/relationship's
own recovered `KnowledgeArtifact` id(s) where `compile` tracked one, falling back to the run's CKM
content hash for compiler-synthesized objects like rollups and risks), `ekos_status`,
`ekos_transformation_explain`/`ekos_transformation_diff` (Transformation IR
explanation and migration diffing — RFC 0028), `ekos_architecture_evaluate`/
`ekos_architecture_drift`/`ekos_architecture_diff` (real completeness/evidence-coverage scoring,
documentation drift, and a real architecture-level diff between two points in time — technologies,
crate role classifications, risks, open questions — distinct from `ekos_diff`'s raw entry report;
RFC 0065/0068 §55/RFC 0107-0108), and `ekos_identity_review`/`ekos_architecture_review` (confirm or
reject a cross-system identity match, or an LLM-classified crate role claim — RFC 0029/RFC 0109,
the two write-capable tools; every other tool reads only the local ledger, except the opt-in
`ekos_session_note`, RFC 0151 — it writes a redacted note to a local inbox file, never to the ledger). Long-lived server
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

**Optional bearer-token auth (RFC 0128).** `--token-file <path>` (or, if that flag is absent,
the `EKOS_MCP_TOKEN` env var; `--tcp-token-file` is a back-compat alias) requires every TCP
connection's **first** message to be an `initialize` request carrying a matching
`params._meta.token` — anything else gets a single `-32001 unauthorized` and the socket closes
before any tool is reachable. The comparison is constant-time. Token-less `--tcp` is unchanged
(RFC 0115 back-compat); stdio is never gated. This is a plaintext-socket bearer token — defence
against a second local process connecting casually, **not** against a network attacker who can
read the wire; use the SSH tunnel below for that.

```bash
ekos mcp serve --workspace /path/to/workspace --tcp 127.0.0.1:7331 \
  --token-file /run/secrets/ekos-mcp-token
```

**Remote — a client on a different machine.** There is **no TLS** on this transport, and auth is
only the optional plaintext bearer token above (RFC 0115/0128's explicit v1 scope) — binding an
externally-reachable address exposes the same read surface stdio gives a spawning parent process,
plus the two write-capable tools, to anyone who can reach it. Two safe ways to do this:

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

#### HTTP transport — for clients that only take a URL (RFC 0143)

Many MCP clients (VS Code / GitHub Copilot agent mode, Visual Studio 2022, ChatGPT Developer
Mode, `mcp-remote`) only offer *stdio* (spawn a command) or *Streamable HTTP* (a URL) — they
cannot speak the raw TCP socket above. `--http <addr>` serves MCP's HTTP transport at one
endpoint, `POST /mcp`:

```bash
ekos mcp serve --workspace /path/to/workspace --http 127.0.0.1:7331
```

`--http` and `--tcp` are mutually exclusive (each *replaces* stdio). The `POST` response is
`application/json`, or a single-shot `text/event-stream` when the client's `Accept` header asks
for it (ChatGPT's connector requires the latter); either way the response bytes are the same
JSON-RPC. There is **no server push** — `GET /mcp` returns `405`, no sessions. Auth is the same
token (`--token-file` / `EKOS_MCP_TOKEN`), presented over HTTP as an `Authorization: Bearer
<token>` header checked on every request. The `Origin` header, when present, must be loopback or
an explicit `--http-allow-origin <origin>` (DNS-rebinding defence); a request with no `Origin`
(the normal case for editors) is allowed. All HTTP requests are serialized through one worker
thread — a slow `tools/call` blocks the next request, matching the stdio loop.

VS Code (`.vscode/mcp.json`) or Visual Studio (`.mcp.json`):

```json
{
  "servers": {
    "ekos": { "type": "http", "url": "http://127.0.0.1:7331/mcp" }
  }
}
```

Verify it is answering:

```bash
curl -s http://127.0.0.1:7331/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}'
```

For a remote client, tunnel it the same way as the TCP transport (`ssh -N -L 7331:127.0.0.1:7331 …`);
there is no TLS on `--http` — terminate it at a reverse proxy if a deployment needs it.

**ChatGPT** (Developer Mode connector, Plus/Pro) is cloud-hosted and cannot reach `127.0.0.1`, so
it needs a public HTTPS URL — put a tunnel in front (`cloudflared tunnel --url
http://127.0.0.1:7331`, then use `https://<name>.trycloudflare.com/mcp`). The tunnel URL is
world-reachable and exposes the two write tools too, so protect it (Cloudflare Access / Tailscale,
or a short supervised session). ChatGPT's *Deep Research* connector won't work — it requires
tools named exactly `search`/`fetch`; only Developer Mode (full MCP) does.

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
ekos ledger audit <id>         # write history of one object/relationship — which run, stage and
                               # source artifact produced each version (RFC 0135; --json).
                               # source artifact is the recovered KnowledgeArtifact id(s) for a
                               # tracked object, or the run's CKM hash for synthesized ones
ekos artifact repack           # loose JSON files → packed segments (~7x smaller on disk)
```

`ekos status [--storage]` (RFC 0116) is a top-level alias for `ekos ledger status` — same output,
shorter to type; both forms stay supported. `--json` (RFC 0127) emits one machine-readable object
instead of the text report — entry/object/relationship/evidence counts, the backend tag, a
per-component storage breakdown, and an mtime-proxy `last_write`; the text output is unchanged.
`ekos doctor --json` (RFC 0129) does the same for the environment checklist
(`{ok, checks:[{name, status, detail}]}`), and `ekos ledger timeline [--bucket day|week|month]`
(RFC 0129) emits cumulative object/relationship counts bucketed by mint time — the growth series
behind the web-console dashboard. All the `--json` / export commands send their logs to stderr so
stdout is a clean document.

`ekos config validate [--json] [--file <path>]` (RFC 0130) parses `ekos.toml` and reports syntax
errors plus `[observe]` mistakes — most usefully, an `ignore-patterns` entry like `*.lock` or
`src/fixtures` that *looks* like a glob or path but is matched as a bare **directory name**, so it
matches nothing. `ekos config preview-scan [--json]` counts what `ekos build` would observe under
the current `paths` / `ignore-patterns` (files, by extension, and how many directories each
ignore pattern actually pruned) without reading or compiling anything.

**`ignore-patterns` is the only filter — `.gitignore` excludes nothing from the observation walk.**
A git-ignored virtualenv, build cache, or scanner working directory is still fully observed unless
you name it here. Measured on this repo (devlog_174): a `.venv/` and two `.scannerwork/` directories
made **94%** of the observed corpus third-party code — over half the compiled knowledge model
described numpy and pytest internals, and every query ranked the project's own code against 27k
foreign objects. Run `preview-scan` before the first `build` on a new workspace, and prefer specific
directory names over generic ones like `build`/`dist`, which match a bare path component and can
prune real source.

### Bulk graph export (RFC 0127)

`ekos graph export` writes the whole compiled graph — every object and relationship — as one JSON
(or NDJSON) document. It's the first non-per-object, non-`LIMIT`-capped read path in EKOS:

```bash
ekos graph export                                  # the whole graph, JSON on stdout
ekos graph export --kind Table --kind File         # filter to object kinds
ekos graph export --exclude-rel-kind CoupledWith --min-degree 1
ekos graph export --level aggregate --group-by kind   # one super-node per kind
ekos graph export --format ndjson --output graph.ndjson
```

Node ids are real object ids (feed them straight to `ekos_state` / `ekos_neighborhood`), degree is
computed over the post-filter edge set, and when the graph exceeds `--max-nodes`/`--max-edges` the
export keeps the most-connected core and says so in a `truncated` block rather than silently
returning a prefix. Output is deterministic modulo its `generated_at` timestamp. The
`ekos_graph_export` MCP tool exposes the same function to agents.

### Web console (RFC 0127/0128/0129/0130/0131/0132/0133/0134/0136/0138)

`web/` is a browser surface over one or more compiled workspaces — a FastAPI app (`web/api/`) plus
a Vite + React app (`web/ui/`).

- **Phase 1** (RFC 0129): a persisted workspace registry, a supervisor that spawns and restarts
  one `ekos mcp serve --tcp` per workspace on its own, and a statistics dashboard
  (entry/object/relationship/evidence counts, storage breakdown, objects by kind, a growth
  timeline, query-log stats, `doctor`).
- **Phase 2** (RFC 0130): an `ekos.toml` editor with `validate` + `preview-scan` and the
  append-only warning when you narrow `[observe]`.
- **Phase 3** (RFC 0131): run EKOS pipeline commands from the browser and watch them stream. A
  hardcoded command allowlist (`build`/`recover`/`resolve`/`compile`/`commit`, `pipeline`,
  `doctor`, `status`, `ekl`, `ledger repair`, `docs generate`, …), a per-workspace job queue
  (SIGTERM→SIGKILL cancel, chained `pipeline` with per-stage status), and an SSE log tail. This is
  the first browser mutation, so it brings the **read/write role split**: auth is **OIDC**
  (Authorization Code + PKCE, a claim → the write role) or, when `OIDC_ISSUER` is unset, **two
  static tokens** — `CONSOLE_TOKEN` (read) and `CONSOLE_WRITE_TOKEN` (read + write).
- **Phase 4** (RFC 0132): scheduled runs. A `Schedule` (workspace + command + cron or interval
  trigger + a required `notify_url`) fires the same job runner; APScheduler is rebuilt from the
  SQLite table on start. A non-`succeeded` run POSTs `{schedule_id, run_id, status, …}` to the
  `notify_url`; the UI shows each schedule's last-run status.
- **Phase 5** (RFC 0133): the graph view (`react-force-graph-2d`, lazy-loaded). An overview as one
  super-node per object kind → click to expand a kind into its 500 most-connected real objects,
  kind / relationship-kind filter toggles (`CoupledWith` / `FeedsInto` off by default), a search
  that flies the camera to a node, and an object panel showing `ekos_state` — properties,
  relationships, and one evidence row per claim (path · analyzer · confidence · fragment).
- **Phase 6** (RFC 0134, RFC 0136): a **timelapse slider** under the graph — drag it back and the
  diagram redraws as the knowledge existed at that instant; a clicked node's panel then shows the
  evidence it had *then*. Backed by `ekos graph export --as-of <rfc3339> --first-seen` (and the
  same `as_of` / `include_first_seen` on `ekos_graph_export`); the browser fetches the latest graph
  once with per-element first-seen stamps and filters it client-side against a **frozen layout**,
  so scrubbing never refetches and nodes never move. Ticks + the activity histogram come from
  `ekos ledger timeline`. The slider is only as deep as the ledger's retained history — rich on a
  workspace committed incrementally, near-flat on a freshly-rebuilt one. **Graph v2** (RFC 0136)
  adds: **neighbourhood isolation** (an object's real BFS sub-graph, real edges included, at a
  depth of 1-3 — `ekos_neighborhood`, unmodified); **impact mode** (`ekos_impact`'s hop-distance
  trace rendered as node coloring source-outward plus highlighted edges between two impacted
  nodes already on screen — the differentiating screen RFC 0127 called out as "the visual form of
  the claim that currently has none"); a **server-side ForceAtlas2 layout** (`networkx` +
  `fa2_modified`) for graphs past ~2,000 nodes, where the browser's own force simulation stops
  keeping up; and one-click **PNG/glTF export** of the current view, entirely client-side (whatever
  zoom, filters, and isolate/impact state are on screen, not a server-rendered reproduction).
- **Eval history** (RFC 0138): every saved `ekos eval run` report, browsable under a workspace's
  **Evals** tab — a history table (dataset, agent, PASS/FAIL, answer-correctness/hallucination-rate)
  and a detail view (all eleven metrics — the five headline scores plus tokens/latency/cache/RSS/
  CPU — and the full per-scenario pass/fail/hallucinated breakdown). Triggering a new run reuses
  the existing command runner (`eval-run` in the allowlist) rather than a bespoke UI — it shows up
  on the **Run** tab like any other command, with `dataset`/`agent`/`category`/`limit` params, and
  its progress streams through the same job log every other command already uses.

```bash
cd ekos && cargo build --release -p ekos && cd ..
EKOS_BIN=$PWD/ekos/target/release/ekos \
EKOS_CONSOLE_CONSOLE_TOKEN=dev-read EKOS_CONSOLE_CONSOLE_WRITE_TOKEN=dev-write \
EKOS_CONSOLE_SESSION_SECRET=$(openssl rand -hex 16) \
uv --directory web/api run uvicorn --factory app.main:create_app --port 8000 &
cd web/ui && npm install && npm run dev        # http://localhost:5173 — sign in with a token
```

`web/docker-compose.yml` runs the same thing (`api` on :8000, `ui` on :5173). A hardening pass
(Phase 7 — performance, theming, packaging) is still to come, authored just-in-time.

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
drop-in for the single-ledger backend — every command (`ekos status`/`ekos ledger status` report
it as `(partitioned, RFC 0111)` with real counts), the MCP server, and `docs generate` work
unchanged. Retrieval fuses each partition's ranked hits **plus a cross-partition exact-name arm**
(RFC 0120), so an exact-name query still promotes the named object even when a strong lexical hit
lives in another partition. Existing SQLite or fact-engine workspaces are **never** switched
implicitly, same rule as the fact-engine default.

**Multi-machine distribution (Phase B, RFC 0113)** is feature-complete at v1 (2026-08-30) and
validated end-to-end: two autonomous fault-injection soak runs — the second against a real S3
endpoint (MinIO) and a 95-partition workspace — found and fixed 8 defects, each with a regression
test (`devlog_144`). What it provides:

- a `SegmentBackend` seam — `LocalFsBackend` (default) or `ObjectStoreBackend` (S3 / Azure /
  in-memory, behind a feature flag);
- a **coordinator** (`ekos coordinator serve`) that hands out fencing-tokened write leases and
  tracks per-partition commit watermarks over newline-delimited JSON-RPC;
- a **compile worker** (`ekos compile-worker run`) that runs the real
  `build → recover → resolve → compile → commit` pipeline under a coordinator lease, then
  registers the partitions it wrote and commits the new generation;
- **self-describing object-storage partitions** — `[storage.partition] segment-backend-url =
  "s3://…"` routes each partition's segments (sealed **and** active), `manifest.json`, `dict.bin`,
  `HEAD`, and search index to S3 / Azure / any S3-compatible store (MinIO included; provider
  credentials come from the standard `AWS_*` / `AZURE_*` env vars). A query worker can then serve
  the partition from its URL alone — including committed-but-unsealed rows, which under
  fine-grained partitioning is most of the data;
- **query workers** (`ekos query-worker serve`) that pull a partition into a local cache and serve
  reads for it, and a **`DistributedLedger` gateway** that implements the same `KnowledgeStore`
  trait every command already uses — fanning reads across the workers, merging, and **failing over
  to another worker** when one is unreachable — so pointing a workspace at a cluster is just
  `[storage.distributed]` in `ekos.toml`:

  ```toml
  [storage.distributed]
  coordinator   = "coordinator.internal:7333"
  query-workers = ["qw1.internal:7334", "qw2.internal:7334"]
  ```

- **distributed search** — the gateway fans each shard's BM25 top-*k* to a worker and merge-sorts
  the results (shard-local term statistics, the standard query-then-fetch approximation);
- **a pooled, concurrent, pruned, fault-tolerant gateway** — `DistributedLedger` reuses one
  connection per coordinator/worker instead of reconnecting per call, fans a multi-partition read
  out concurrently, prunes id-scoped reads (`get_object` and friends) to the few partitions the
  coordinator's index says actually hold that id, and rotates to the next worker on a connection
  failure;
- **adaptive leases** — `ekos compile-worker` derives its heartbeat from the lease's real TTL, so
  `ekos coordinator serve --ttl-seconds 5` (fast failover) is safe; `ekos compile-worker run
  --force` is the Service-A equivalent of `ekos resolve --force`; `ekos compile-worker run
  --retry-lease-seconds <N>` keeps retrying (every 3s) if the shard's lease is already held by
  another worker instead of failing immediately — 0 (default) preserves the original fail-fast
  behavior, and only an "already leased" conflict is ever retried, never a genuinely failed run.

That completes Phase B at its v1 scope. Known follow-on: interrupt-of-in-flight-work on lease
loss (a fenced worker currently runs its pipeline to the end, then its commit is rejected) — needs
a cancellation signal threaded through the whole pipeline, materially bigger than the acquire-retry
loop above. None of this affects Local mode, which stays the default.

## Development Process

All significant architectural decisions begin as RFCs in `docs/rfcs/`. No feature is implemented until its RFC is accepted. See `CLAUDE.md` for the full mandatory development workflow.

## Presentations

Live decks at [alexeyban.github.io/EKOS](https://alexeyban.github.io/EKOS/presentations.html) — every claim in them is reproduced live against real repos, not staged:

- [Full-Stack Test Run](https://alexeyban.github.io/EKOS/presentations/full-stack-test-run.html) — an autonomous 22-act pass over the distributed storage stack (RFC 0111/0113), the compiled-knowledge query engine (RFC 0118/0119–0126), and the MCP protocol (RFC 0013/0115): coordinator fencing, gateway failover, RRF fusion, REASON citation checks, the vector arm on a real embedding model, and the write-vs-read-only-gateway safety assertion. Three partitioned-store bugs it surfaced, fixed and re-verified in the same run.
- [Claude Code + EKOS](https://alexeyban.github.io/EKOS/presentations/claude-code-with-ekos.html) — how Claude Code searches and analyzes a codebase through EKOS's MCP server instead of raw grep/Read, with a measured with-vs-without comparison and real token/usage numbers.
- [The AI-Native Enterprise Knowledge Compiler](https://alexeyban.github.io/EKOS/presentations/ai-native-knowledge-compiler-pitch.html) — the startup pitch, audited live by Claude Code using EKOS's own MCP server.
- [ClickHouse: Compiled Metadata + Live NL-to-SQL](https://alexeyban.github.io/EKOS/presentations/clickhouse-connector.html) — the one explicit, audited exception to "AI never touches raw enterprise systems directly," verified live against a real ClickHouse container, honest failures included.
- [GitHub, Live, End to End](https://alexeyban.github.io/EKOS/presentations/github-live-cross-system.html) — the GitHub connector's first live run, 1,600 real issues/PRs from a real repo: two known gaps fixed before the run, a third (96% of items collapsing into one identity) found only at real scale and fixed the same session, and the residual limitation reported honestly, not hidden.
- [EKOS from 53 to 87](https://alexeyban.github.io/EKOS/presentations/deepseek-53-to-87.html) — the same 101-question eval, graded deterministically, moving from a local llama3 8B to DeepSeek V4 Flash (OpenCode Zen) and then to better document structure and query planning: 53 → 70 → 79 → 87, what each step bought, the 14 that still fail and why.
- [Eval Comparison: Fixing a Self-Contaminated Ledger](https://alexeyban.github.io/EKOS/presentations/eval-comparison-report.html) — how the eval harness found that 94% of what EKOS was compiling was not its own code, and three bugs that made fixes silently do nothing.
- [TSD System Documentation](https://alexeyban.github.io/EKOS/presentations/tsd-documentation.html) and [How TSD Works](https://alexeyban.github.io/EKOS/presentations/tsd-how-it-works.html) — a Windows CE barcode terminal and its desktop server documented from compiled .NET binaries alone (RFC 0148), every claim traced to a metadata token.
- [Distributed Storage Under Fire](https://alexeyban.github.io/EKOS/presentations/distributed-storage-under-fire.html) — two end-to-end runs of the RFC 0111/0113 distributed engine and the eight defects they found.
- [EKOS Web Console](https://alexeyban.github.io/EKOS/presentations/web-console.html) — the web console, RFC 0128–0133.
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

The workspace is versioned `1.0.0`, released 2026-09-23 — the `v1.0` row above. What has actually
shipped — RFCs up to 0153 and 201 devlogs so far — is tracked phase by phase in [TODO.md](TODO.md)
and the devlogs, not by this table. See [CHANGELOG.md](CHANGELOG.md) for what 1.0.0 commits to.

## Team

### Alexey Banaev — Founder & Lead Developer

<img src="docs/assets/team-alexey-banaev.jpg" alt="Portrait of Alexey Banaev" width="140" align="left" hspace="16" vspace="4">

Alexey is a Data Engineer and Solution Architect with extensive experience designing data
platforms, analytics solutions, and distributed systems. He founded EKOS to address a fundamental
challenge in modern data environments: making complex, fragmented data ecosystems understandable,
traceable, and usable.

As EKOS's founder and main developer, Alexey leads the product architecture and engineering,
combining data engineering, software development, and AI-driven approaches to build a practical
solution for modern data teams.

<br clear="left">

### Omid Ahmadi — Core Team, Ecosystem & Growth

**AI Researcher & Agentic Systems Engineer**

<img src="docs/assets/team-omid-ahmadi.jpg" alt="Portrait of Omid Ahmadi" width="140" align="left" hspace="16" vspace="4">

Researcher focused on sustainable AI applications across the oil, gas, and petrochemical
industries, with a focus on intelligent agents, autonomous systems, and reliable AI infrastructure
for industrial environments.

<br clear="left">

## License

MIT — see [LICENSE](LICENSE).

# EKOS

**A compiler for enterprise knowledge.**

EKOS observes the systems you already have — source code, Git history, SQL schemas, GitHub
issues, Confluence, local documents — compiles them through deterministic passes into a canonical
knowledge model, and serves that model to AI agents over MCP, with the evidence attached to every
conclusion.

It is not a database, a document store, or a RAG index. It is a compiler: `build → recover →
resolve → compile → commit`, each stage writing artifacts the next consumes, into an append-only
ledger where nothing is ever modified in place.

## Install

A prebuilt binary needs no Rust toolchain:

```sh
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh
```

Or from this crate:

```sh
cargo install ekos
```

## Use

```sh
cd /path/to/your/repo
ekos init --detect     # writes an ekos.toml matching what is actually in this repository
ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
ekos coverage          # confirms every input kind actually produced objects
ekos ask "what does this system do?"
```

Expose the compiled ledger to an AI agent, read-only:

```sh
ekos mcp serve --workspace .        # stdio; also --tcp <addr> and --http <addr>
```

## What it compiles

Rust, Python/PySpark, JavaScript/TypeScript, Elixir, Perl, SQL (Postgres, MySQL, MSSQL, Oracle,
Snowflake, Databricks, ClickHouse), dbt projects, Pentaho Kettle jobs, Git history, GitHub
issues/PRs, Confluence spaces, and local documents (PDF, DOCX, HTML, email, with OCR).

## Guarantees

- The ledger is **append-only**; the runtime is **read-only**.
- Every semantic conclusion traces to **evidence** — one of four primitives: Object,
  Relationship, Event, Evidence.
- Compiler passes are **deterministic** and side-effect-free.
- **Secrets and PII are never observed or stored.** A built-in redaction baseline runs at every
  raw-content entry point and configuration can only extend it, never disable it.

## Links

- Repository, full documentation and RFCs: <https://github.com/alexeyban/EKOS>
- Capabilities reference: <https://alexeyban.github.io/EKOS/generated/ekos-self-documentation.html>
- Stability promise and known limitations:
  [CHANGELOG.md](https://github.com/alexeyban/EKOS/blob/main/CHANGELOG.md)

Licensed under the MIT License.

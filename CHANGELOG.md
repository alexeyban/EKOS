# Changelog

## v0.1.0

The first tagged release of EKOS, and the first one you can install without a Rust toolchain.

EKOS is a **compiler for enterprise knowledge**. It observes the systems you already have —
source code, Git history, SQL schemas, GitHub issues, Confluence, local documents, compiled
binaries — compiles them through deterministic passes into a canonical knowledge model, and
serves that model to AI agents over MCP, with the evidence attached to every conclusion.

### Install

```bash
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh
```

Then, in any repository:

```bash
ekos init --detect
ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
ekos coverage
ekos ask "what does this system do?"
```

Prebuilt binaries: Linux x86_64 (glibc and static musl), Linux aarch64, macOS on Apple Silicon
and Intel, Windows x86_64. Every asset is listed in this release's `SHA256SUMS`, and the
installer verifies the download against it before unpacking.

### What's in it

- **The compiler pipeline** — `init → build → recover → resolve → compile → commit`, each stage
  writing artifacts the next consumes, into an append-only ledger where every conclusion carries
  the evidence it came from.
- **Connectors** for files, Git, GitHub, Confluence, local documents (PDF/DOCX/HTML/email),
  Pentaho, dbt, ClickHouse, and source analysis for Rust, Python, JavaScript/TypeScript, Elixir
  and Perl — real ASTs, not regex.
- **MCP server** (`ekos mcp serve`) over stdio, TCP or HTTP, exposing the ledger read-only to
  Claude Code, VS Code, Visual Studio and any other MCP client.
- **Query surfaces** — `ekos ask` (grounded, cited answers), `ekos query find` (BM25 + vector,
  RRF-fused), `ekos ekl` (a real query language), `ekos impact` (multi-hop blast radius).
- **Documentation generation** — `ekos docs generate` renders README/architecture/API/sequence
  diagrams deterministically from the compiled ledger, with no LLM in the path.
- **First-run self-verification** (new) — `ekos init --detect` writes a config that matches your
  repository, and `ekos coverage` reports which input kinds actually compiled into objects, so a
  pipeline that silently produced nothing says so.

### Known limitations

- Identity resolution merges only exact normalized-name matches; fuzzy candidates become
  reviewable `unconfirmed` relationships rather than silent merges. This is deliberate
  (RFC 0060), and it means some real duplicates stay unmerged until a human confirms them.
- SQL DDL parsing is whole-file all-or-nothing per dialect: an unsupported construct anywhere in
  a file loses the whole file. `ekos coverage` now reports this rather than hiding it, but the
  parser behaviour itself is unchanged.
- The Salesforce, SAP, Oracle, Fabric and Snowflake connectors are scaffolded proofs of concept
  against mock API shapes — they have not been exercised against live accounts.
- Not published to crates.io yet: `cargo install ekos` requires publishing 30 internal crates in
  dependency order, which is deliberately a separate, careful piece of work.
- No release signing beyond `SHA256SUMS`.

Full engineering history is in `devlogs/` (199 entries) and the design record in `docs/rfcs/` and
`ekos/docs/rfcs/` (153 RFCs).

# Changelog

## v1.0.0 — 2026-09-23

The first tagged release of EKOS, and the first one you can install without a Rust toolchain.

EKOS is a **compiler for enterprise knowledge**. It observes the systems you already have —
source code, Git history, SQL schemas, GitHub issues, Confluence, local documents, compiled
binaries — compiles them through deterministic passes into a canonical knowledge model, and
serves that model to AI agents over MCP, with the evidence attached to every conclusion.

This is the `v1.0` row of the versioning roadmap in the README: the Enterprise Knowledge
Compiler, end to end.

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

### What 1.0.0 commits to

Version numbers are a promise, so here is the exact one being made.

**Stable, and a breaking change means 2.0.0:**

- The pipeline verbs and their order — `init → build → recover → resolve → compile → commit` —
  and their existing flags.
- The MCP tool names and their argument shapes. An agent configured against 1.0.0 keeps working.
- The on-disk ledger format. A ledger written by 1.x is readable by every later 1.x. The legacy
  SQLite backend keeps serving existing workspaces and is never switched implicitly.
- The four semantic primitives — Object, Relationship, Event, Evidence — and the rule that every
  conclusion carries its evidence.
- The `ekos.toml` schema: existing keys keep their meaning, and new ones are additive with
  defaults that preserve current behaviour.

**Explicitly not covered by that promise:**

- The Rust crate APIs. The crates are not published, and `compiler-sdk` / `observation-sdk` are
  still moving as connectors are added. Out-of-tree builds use the RFC 0149 extension seam, which
  *is* covered.
- Output text and formatting of human-readable reports. `--json` surfaces carry a
  `schema_version` and are the stable form for machines.
- LLM-generated content. Different models and prompts produce different prose; only the grounding
  and citation contract is stable.
- Anything gated behind an opt-in flag that this file calls scaffolded or experimental.

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
- **First-run self-verification** — `ekos init --detect` writes a config that matches your
  repository, and `ekos coverage` reports which input kinds actually compiled into objects, so a
  pipeline that silently produced nothing says so.
- **Storage that scales down and up** — a single-file fact-segment engine by default, opt-in
  partitioned storage, and an opt-in distributed tier with object-store backends.

### Known limitations

These are real and shipped as-is; 1.0.0 is a stability promise, not a claim of completeness.

- Identity resolution merges only exact normalized-name matches; fuzzy candidates become
  reviewable `unconfirmed` relationships rather than silent merges. This is deliberate
  (RFC 0060), and it means some real duplicates stay unmerged until a human confirms them.
- SQL DDL parsing is whole-file all-or-nothing per dialect: an unsupported construct anywhere in
  a file loses the whole file. `ekos coverage` now reports this rather than hiding it, but the
  parser behaviour itself is unchanged.
- The Salesforce, SAP, Oracle, Fabric and Snowflake connectors are scaffolded proofs of concept
  against mock API shapes — they have not been exercised against live accounts.
- Call-graph recovery is intraprocedural and within-file for most languages; cross-file call-chain
  tracing is not implemented. Perl has no call graph at all (dispatch is fully dynamic).
- Not published to crates.io: `cargo install ekos` requires publishing 30 internal crates in
  dependency order, which is deliberately a separate, careful piece of work.
- Releases are checksummed (`SHA256SUMS`) but not signed. No Sigstore/cosign, no SLSA provenance.
- The compiled .NET/JVM binary decompiler is a separately licensed extension build, not part of
  this release (RFC 0148/0149).

Full engineering history is in `devlogs/` (201 entries) and the design record in `docs/rfcs/` and
`ekos/docs/rfcs/` (153 RFCs).

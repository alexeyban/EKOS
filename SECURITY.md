# Security Policy

EKOS observes and compiles knowledge from source code, SQL schemas, documents, and other
enterprise systems, then serves that knowledge to AI agents through a read-only MCP server. Given
that scope, security issues in EKOS can affect the confidentiality of a user's codebase, database
schemas, and documents — please report responsibly rather than opening a public issue.

## Supported Versions

EKOS is pre-1.0 and has no tagged releases yet. Only the `main` branch is currently supported.
Once tagged releases begin, this table will be updated to reflect which versions receive
security patches.

| Version | Supported |
| ------- | --------- |
| `main`  | ✅        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report privately using [GitHub Security Advisories](https://github.com/alexeyban/EKOS/security/advisories/new)
for this repository. This creates a private draft advisory visible only to maintainers until a
fix is ready.

If you're unable to use GitHub Security Advisories, you can reach the maintainer via the official
X account [@ekosproject](https://x.com/ekosproject) to request an alternate private channel.

**Please include, where possible:**

- A clear description of the vulnerability and its potential impact
- Steps to reproduce (a minimal `ekos.toml` / workspace setup is ideal)
- The affected crate(s) or MCP tool(s), if known
- Whether the issue requires local access, a malicious observed repository/document, or a
  malicious MCP client to trigger

**Response times:** we aim to acknowledge new reports within 48–72 hours and to provide an
initial assessment (severity, expected timeline) within 7 days. Fix timelines depend on
severity and complexity, but critical issues (see Scope below) are prioritized above all other
work.

## Disclosure Policy

EKOS follows coordinated disclosure:

1. The report is triaged and confirmed privately.
2. A fix is developed and tested, following the project's normal RFC-first process where the
   issue's scope requires an architectural change.
3. The fix is released, and a public security advisory is published at the same time,
   describing the issue and crediting the reporter (unless they prefer to remain anonymous).
4. Reporters are asked to avoid public disclosure until a fix is available. If a fix is taking
   an unreasonable amount of time, reach out via the reporting channel to discuss a disclosure
   timeline together rather than disclosing unilaterally.

## Scope

### In scope

Given EKOS's architecture — an Observation Layer that reads enterprise systems, a deterministic
Knowledge Compiler, an append-only ledger, and a read-only Runtime exposed to AI agents via MCP —
the following are treated as security issues:

- **Bypassing the read-only Runtime boundary.** Any way for an MCP tool call, a crafted
  observation input, or a compiler pass to write to, modify, or delete ledger data through a
  path other than the intended `commit`/compile pipeline.
- **Bypassing PII/secrets redaction (RFC 0043).** Any secret shape (API keys, tokens, private
  keys, JWTs, credentials) or PII that should be redacted by the built-in baseline but ends up
  stored in the ledger, returned by an MCP tool, or written to generated documentation.
- **SQL injection or unsafe query construction** in the ClickHouse live NL-to-SQL path
  (`ekos_clickhouse_query`, RFC 0056) — including any way to get a non-`SELECT` statement,
  multi-statement batch, or unintended data exposure past the hard-rejection gate.
- **Path traversal or arbitrary file access** during observation/parsing of a workspace,
  document, or repository (e.g., a maliciously crafted file causing EKOS to read or write
  outside the intended workspace boundary).
- **Remote code execution or arbitrary command execution** via any observation, compiler, or
  connector pass — including through maliciously crafted source files, documents, Git history,
  or SQL/DDL input.
- **Identity resolution abuse** that could be used to deliberately merge or split identities to
  hide or misattribute evidence (distinct from the known, openly-tracked over-merging *accuracy*
  issue — see Known Issues below; this scope item covers deliberate, adversarial manipulation).
- **Authentication/authorization gaps** around the gated `ekos_clickhouse_query` MCP tool or any
  other opt-in, credentialed connector (e.g., a way to reach a gated tool without the workspace
  explicitly enabling it).
- **Supply-chain issues** in the Cargo workspace (malicious dependency, compromised build step).

### Out of scope

- **The EKOS token / Solana smart contract.** The token is a separate system from this codebase.
  For token-contract or tokenomics-related concerns, see [TOKENOMICS.md](TOKENOMICS.md) or reach
  out via [@ekosproject](https://x.com/ekosproject). This repository's security policy covers
  the Rust codebase and its runtime behavior only.
- **Denial of service via resource exhaustion on self-hosted instances** you control (e.g.,
  feeding EKOS an intentionally huge repository on your own machine) — this is a
  performance/robustness concern, not a security vulnerability, unless it demonstrates an
  exploitable issue in a multi-tenant or shared deployment context.
- **Issues requiring physical access** to a machine already running EKOS.
- **Best-practice suggestions without a demonstrated exploit** — these are welcome as regular
  GitHub issues or discussions, not security reports.

## A Note on MCP Tool Access

`ekos mcp serve` exposes the compiled ledger to AI agents (e.g., Claude Code) over the Model
Context Protocol. Per the project's key invariants, every MCP tool is read-only except
`ekos_identity_review` (confirms/rejects an identity match — never a raw data write) and the
explicitly gated, off-by-default `ekos_clickhouse_query`. If you find any other MCP tool capable
of mutating the ledger, exfiltrating redacted secrets, or reaching a live external system without
explicit opt-in configuration, please treat it as a **critical-severity** report.

## Known Issues (Tracked Openly, Not Security-Sensitive)

In keeping with the project's practice of reporting findings honestly rather than hiding them,
the following are known *accuracy* issues, tracked via RFCs and devlogs rather than security
advisories, since they don't expose data to unauthorized parties:

- **Identity resolution over-merging** — in real-world testing, structurally similar but
  distinct entities (e.g., tables sharing a name prefix and column structure) have been
  incorrectly merged into a single identity at high confidence. This affects correctness, not
  confidentiality — see the relevant devlogs for status.
- **Retrieval brittleness in `ekos ask`** — some full-sentence natural-language questions have
  returned no context even when the target object was trivially findable by keyword. Tracked and
  partially addressed via RFC 0059–0061.

If you believe either of these has a security-relevant angle (e.g., over-merging used to hide
evidence, or retrieval gaps used to bypass redaction), please report it through the vulnerability
process above rather than as a regular issue.

## Security Design Summary

For reference, the security-relevant architectural guarantees this project is built around:

- The **Observation Layer** collects facts only — it never interprets business meaning.
- The **ledger is append-only** — knowledge is never modified in place.
- The **Runtime is read-only** — AI agents never touch raw enterprise systems directly, with the
  single explicit, gated, audited exception of `ekos_clickhouse_query` (RFC 0056).
- **Secrets and PII are never observed or stored** (RFC 0043) — a built-in baseline redacts known
  secret shapes and excludes sensitive files (`.env`, `*.pem`, `id_rsa`, etc.) entirely.
  `ekos.toml`'s `[security]` section can only extend this baseline, never disable it.
- Every compiler pass is **deterministic and side-effect-free**.
- Every artifact is **content-addressable** (id + checksum + metadata + dependencies + version),
  enabling integrity verification.

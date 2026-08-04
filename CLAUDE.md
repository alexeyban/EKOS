# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

EKOS has an implemented Rust (2024 edition) Cargo workspace — this is not a design-phase repo.
Read `devlog_*.md` (root, numbered chronologically, `devlog_30.md` is latest) before starting
non-trivial work: they are the project's long-term memory and record what shipped, why, and what
was learned. `TODO.md` tracks the phase-by-phase roadmap; RFCs are split across two locations for
historical reasons, not a meaningful distinction — `docs/rfcs/` (repo root) has `0001`–`0024`,
`ekos/docs/rfcs/` has `0025`+. **Check both directories for the highest existing number before
picking one for a new RFC** — two RFCs have already collided on the same number once (0027) from
sessions that only checked one location.

**The Cargo workspace root is `ekos/`, not the repo root** — there is no top-level `Cargo.toml`.
`benchmark/` and `tests/integration/` are separate Cargo workspaces that depend on the `ekos/`
crates by path.

## Commands

```bash
# Main workspace (run from ekos/, or pass -p <crate>/--manifest-path from elsewhere)
cd ekos
cargo build --workspace
cargo test --workspace
cargo test -p ekos-ledger                    # one crate, e.g. ekos-ledger, ekos-plugin-git
cargo test -p ekos-ledger some_test_name     # one test
cargo clippy --workspace -- -D warnings      # CI fails on any warning
cargo fmt --check                            # CI checks formatting, doesn't fix it

# Integration tests (separate workspace, depends on ekos/ crates by path)
cd tests/integration && cargo test

# Benchmarks (separate workspace, Criterion)
cd benchmark && cargo bench
cargo bench --bench ledger_write             # one benchmark file

# CLI (binary crate name is `ekos`, package is crates/cli)
cargo run -p ekos -- init
cargo run -p ekos -- build && cargo run -p ekos -- recover && cargo run -p ekos -- resolve \
  && cargo run -p ekos -- compile && cargo run -p ekos -- commit   # full pipeline, in order
cargo run -p ekos -- doctor
cargo run -p ekos -- ask "<question>"
cargo run -p ekos -- identity scan          # cross-system candidate matches (RFC 0029)
cargo run -p ekos -- marketing publish      # devlog -> tweet -> approval -> X (RFC 0030)
cargo run -p ekos -- mcp serve --workspace <dir>
```

CI (`.github/workflows/ci.yml`) runs build+test+clippy+fmt from `ekos/` and `cargo bench` from
`benchmark/` on every push/PR to `main`. Match these locally before pushing.

## High-Level Architecture

EKOS is a **compiler for enterprise knowledge**, not a database or document store. It observes
enterprise systems without interpreting them, compiles those observations through deterministic
passes into a Canonical Knowledge Model, and stores the result in an append-only ledger where
every conclusion carries the evidence it was derived from.

```
Enterprise Systems → Observation Layer → Knowledge Compiler → Canonical Knowledge Model (CKM)
                                                                          ↓
                              AI/Apps ← Knowledge Runtime (read-only) ← Semantic Knowledge Ledger
```

This maps directly onto the `ekos init/build/recover/resolve/compile/commit` CLI pipeline: each
verb is a compiler stage, run in that order, writing artifacts the next stage consumes.

### Crate map (`ekos/crates/`)

| Crate | Role |
|---|---|
| `compiler-core` | `Compiler`, `PassManager`, `Scheduler`, `Diagnostics`, `EkosConfig` — the pipeline that drives every pass over a `PassContext` |
| `compiler-sdk` | Public traits for extending the compiler |
| `observation-sdk` | `Observer` trait — the contract every connector implements, returning an `ObservationPackage` of content-addressable `ObservationArtifact`s |
| `artifact` | Artifact types + `ArtifactStore` (loose JSON, or packed segments post RFC 0015) |
| `kir` | Knowledge Intermediate Representation — the typed output of knowledge-recovery passes, input to the semantic compiler |
| `recovery` | Knowledge-recovery passes: one analyzer per source kind (`sql_analyzer` — DDL, `sql_transform_analyzer` — SELECT/VIEW/procedures into the Transformation IR, `pentaho_analyzer`, `git_analyzer`, `github_analyzer`, `confluence_analyzer`, `local_docs_analyzer`, `document_semantics_analyzer`, `crypto_analyzer`, `dependency_analyzer`), plus LLM provider glue (`anthropic.rs`, `ollama.rs`, `llm.rs`) |
| `identity` | Identity Resolution — `DefaultResolver` merges same-source-kind duplicates before the CKM exists (RFC 0007); `cross_system.rs` separately scores cross-system candidate matches (RFC 0029), written as reviewable `unconfirmed` relationships, never auto-merged |
| `semantic` | Semantic compiler: KIR + resolved identities → CKM; `transform_ir.rs` is the shared Transformation IR (RFC 0027) every legacy-format parser (Pentaho, SQL) compiles into |
| `ledger` | Append-only Semantic Knowledge Ledger — `fact.rs`/`fact_ledger.rs` (facts), `index.rs`, `search.rs`; SQLite-backed by default, RFC 0016 fact-segment engine (tantivy + mmap) is an opt-in `--v3` migration |
| `runtime` | Read-only state reconstruction and context projection; `ai.rs` is the surface AI agents query |
| `ekl` | Enterprise Knowledge Language — `parser.rs` + `interpreter.rs` for the `ekos ekl` query command |
| `marketing` | Devlog → tweet → human approval → X publish (RFC 0030) — auxiliary tooling outside the compiler pipeline, not a `CompilerPass`/`Observer` |
| `cli` | `commands/` — one file per CLI subcommand, dispatched from `crates/cli/src/bin/ekos.rs`; also hosts the MCP server (`commands/mcp.rs`) |
| `common`, `scheduler` | Shared utilities; pass scheduling primitives |

### Connectors (`ekos/plugins/`)

Each plugin implements `Observer` from `observation-sdk` and is registered independently in
`ekos/Cargo.toml`'s workspace members. Real/tested: `file`, `git`, `github`, `confluence`,
`localdocs` (PDF/DOCX/text/Markdown/HTML/email), `pentaho` (`.ktr`/`.kjb`, RFC 0027), `crypto`.
Scaffolded proof-of-concept only (mock API shapes, not exercised against live accounts):
`salesforce`, `sap`, `oracle`, `fabric`, `snowflake`.

### Key invariants (enforced by review, not just convention)

- The ledger is **append-only** — knowledge is never modified in place.
- Every semantic conclusion must be traceable to **Evidence** (one of the four primitives:
  Object, Relationship, Event, Evidence).
- The Runtime is **read-only** — it reconstructs state, never modifies it.
- AI systems consume knowledge through the Runtime only (in practice, through `ekos mcp serve`,
  RFC 0013) — they never touch raw enterprise systems directly.
- Compiler passes must be **deterministic** and **side-effect-free**.
- Artifacts are **content-addressable** (id + checksum + metadata + dependencies + version).

### AI agent access (MCP)

`ekos mcp serve --workspace <dir>` exposes the Runtime read-only over stdio via newline-delimited
JSON-RPC 2.0. Tools: `ekos_search`, `ekos_ekl`, `ekos_neighborhood`, `ekos_state`,
`ekos_dependents`, `ekos_impact` (multi-hop, RFC 0018), `ekos_diff`, `ekos_status`,
`ekos_transformation_explain`/`ekos_transformation_diff` (Transformation IR, RFC 0028), and
`ekos_identity_review` (confirm/reject a cross-system identity match, RFC 0029 — the one
write-capable tool; every other tool is read-only, going through `Runtime` only). This repo's
own `.claude/skills/ekos-knowledge` and `.claude/skills/memory` skills consume this server —
`demo/` has a rehearsable scripted walkthrough (`demo/DEMO.md`, `demo/headless.sh`) if you need
to see the whole pipeline exercised end to end.

### LLM-backed passes

`recovery`'s analyzer passes that call an LLM (document semantics, some SQL/Git recovery) go
through the `LlmProvider` trait (`recovery/src/llm.rs`), with `anthropic.rs` and `ollama.rs`
implementations selected via `[llm]` in `ekos.toml`. Treat this as a Claude-relevant provider
boundary — check `recovery/src/llm.rs` before assuming request/response shapes.

## Mandatory Development Workflow

Every task follows this sequence — do not skip steps:

1. **Design** — write an RFC in `docs/rfcs/NNNN-<topic>.md` before any code
2. **Architecture Review** — validate against the compiler model above
3. **Interfaces** — define public traits and types first
4. **Tests** — write tests before implementation
5. **Implementation**
6. **Refactoring**
7. **Documentation** — every public API must be documented
8. **Integration**
9. **Benchmark** — required for performance-relevant changes (see `benchmark/`)
10. **Merge**

No feature is implemented until its RFC is accepted. RFCs are authored **just-in-time**, before
the phase that needs them, not all up front (see `TODO.md` Phase -1 for the rationale).

## PR Checklist

Every PR must satisfy: tests passing, documentation, benchmarks (performance-relevant changes
only), no public API break, compiler diagnostics, logging, examples.

## Coding Rules

- Rust 2024 edition
- Zero `unsafe` unless formally justified in an RFC
- No global mutable state
- Dependency injection through traits (`Observer`, `LlmProvider`, `CompilerPass`, `ArtifactStore`, …)
- Every artifact must be serializable
- Pure functions wherever possible
- Reproducible builds

## Devlog Rule

**`devlog_N.md` files are the project's long-term memory.** They are the primary source of truth for
project history, architecture decisions, production incidents, and non-obvious knowledge. Treat them
as the first thing to read, not the last thing to write.

**After any session with significant changes, generate a new `devlog_N.md` at the repo root.**

Significant = any of: new feature shipped, bug found and fixed, architecture decision made,
non-obvious knowledge captured, production incident, or a set of PRs merged in one session.
Minor chores (typo fixes, dependency bumps) alone do not warrant a devlog entry — fold them
into the next substantive one.

### Filename

Increment from the highest existing `devlog_N.md`: `devlog_30.md` → `devlog_31.md`, etc.

### Required sections

```markdown
# Devlog N — <short title>

**Date:** YYYY-MM-DD
**PRs:** #N, #N+1, …
**Branch:** <branch> → <target> (merged / squash-merged)

---

## Summary
<2–5 sentence overview: what changed and why it mattered>

---

## PR #N — <title>

### Problem / motivation
### What was built  (table of components if >3 items)
### Implementation details worth remembering
### Decisions (alternatives considered, why this choice)

(repeat for each PR)

---

## Knowledge Captured
<Non-obvious facts, gotchas, SDK quirks, production behaviour, or patterns
 that should not be re-discovered from scratch. Each item should answer:
 "What would a future developer need to know to avoid the same mistake?">

---

## Files Changed
| File | Change summary |
```

### What belongs in "Knowledge Captured"

- SDK/library quirks that aren't in the docs
- Production-only behaviour
- Decisions with non-obvious rationale
- Cost/latency benchmarks discovered in practice
- Debugging techniques that were hard to find

### After writing the devlog

1. Also update `TODO.md` to tick off completed items for the day's work
2. Update `README.md` if any user-facing behaviour changed
3. Commit everything in one PR: `chore: devlog_N, README + TODO update for <topic>`

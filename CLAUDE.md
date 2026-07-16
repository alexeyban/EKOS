# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

EKOS is in the design phase. No production code exists yet. The planned implementation language is **Rust (2024 edition)** using a Cargo workspace.

## Commands (once bootstrapped)

```bash
cargo build           # Build all crates
cargo test            # Run all tests
cargo test -p <crate> # Run tests for a single crate
cargo clippy          # Lint
cargo fmt             # Format
ekos --help           # CLI entry point
ekos init / build / clean / doctor
ekos mcp serve --workspace <dir>   # MCP server over stdio for AI agents (RFC 0013)
```

## Planned Workspace Structure

```
ekos/
  crates/
    compiler-core/    # Pass Manager, Scheduler, Diagnostics, Config
    compiler-sdk/     # Public SDK traits for extending the compiler
    observation-sdk/  # trait Observer { fn scan(...) } — connector interface
    artifact/         # Artifact types (Observation, Knowledge, Evidence, Diagnostic, Index)
    scheduler/
    ledger/           # Append-only Semantic Knowledge Ledger
    runtime/          # State reconstruction, context projection (read-only)
    identity/         # Identity Resolution — merges synonymous concepts
    recovery/         # Knowledge Recovery compiler passes
    semantic/         # Semantic compiler: Recovered Knowledge → CKM
    common/
    cli/
  plugins/            # Connectors: postgres/, sqlserver/, git/, confluence/, jira/
  docs/rfcs/          # RFC-per-feature before any implementation (see below)
  examples/
  tests/
  benchmark/
```

## Architecture

EKOS is a **compiler** for enterprise knowledge, not a database or document system.

Pipeline: `Enterprise Systems → Observation Layer → Knowledge Compiler → Canonical Knowledge Model (CKM) → Semantic Knowledge Ledger → Knowledge Runtime → AI/Apps`

Four semantic primitives stored in the ledger: **Object**, **Relationship**, **Event**, **Evidence**.

Key invariants:
- The ledger is **append-only** — knowledge is never modified in place.
- Every semantic conclusion must be traceable to **Evidence**.
- The Runtime is **read-only** — it reconstructs state, never modifies it.
- AI systems consume knowledge through the Runtime only; they never touch raw enterprise systems.
- Compiler passes must be **deterministic** and **side-effect-free**.
- Artifacts are **content-addressable** (unique id + checksum + metadata + dependencies + version).

## Mandatory Development Workflow

Every task must follow this sequence — do not skip steps:

1. **Design** — write an RFC in `docs/rfcs/NNNN-<topic>.md` before any code
2. **Architecture Review** — validate against the compiler model
3. **Interfaces** — define public traits and types first
4. **Tests** — write tests before implementation
5. **Implementation**
6. **Refactoring**
7. **Documentation** — every public API must be documented
8. **Integration**
9. **Benchmark**
10. **Merge**

No feature is implemented until its RFC is accepted.

## PR Checklist

Every PR must satisfy: tests passing, documentation, benchmarks (performance-relevant changes from Phase 4 onward only), no public API break, compiler diagnostics, logging, examples.

## Coding Rules

- Rust 2024 edition
- Zero `unsafe` unless formally justified in an RFC
- No global mutable state
- Dependency injection through traits
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

Increment from the highest existing `devlog_N.md`: `devlog_14.md` → `devlog_15.md`, etc.

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

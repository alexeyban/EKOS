# Devlog 72 — RFC 0068 Increment 1: System Context view + real documentation drift

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

First increment of continuous, automatic build-out against RFC 0068's full 67-section
documentation standard (filed last session, with explicit instruction not to cut anything —
TODO.md carries the full roadmap). This increment: System Context view (RFC 0068 §15) and real
Documentation Drift detection (§31-32) — the two RFC 0068 §61 MVP items closest to existing data.
Both real primitives this feature needed already existed in the codebase, unused; the actual new
code is small. Live verification needed no new pipeline run at all — this repo's own real,
already-committed ledger already contained genuine drift from earlier real work this session.

---

## RFC 0069 — System Context + real documentation drift

### Problem / motivation

RFC 0068 §15 wants a C4 System Context view (one level broader than the Container view RFC 0065
Phase 1 already shipped). §31-32 wants Documentation Drift: "a discrepancy between documented
architecture and architecture supported by current evidence." Both were unbuilt; investigated each
against the real codebase before writing anything, per this project's established practice this
whole session.

### What was built

| Component | What it does |
|---|---|
| `render_system_context` (`docs-gen`) | New `## System Context` section — one "System" node, edges to every `Technology` with a real compiled `DependsOn` edge |
| `drift_from_history` (`recovery/architecture_drift.rs`) | Pure comparison of a `Claim`'s oldest vs. newest recorded value |
| `detect_drift` (`cli/commands/architecture.rs`) | Fetches each crate's role-claim history from the store, calls the pure comparison, wired into the final report |

### Implementation details worth remembering

**Documentation Drift needed zero new storage — the primitive already existed, unused.**
`KnowledgeStore::object_history(id)` (RFC 0047) already returns every version of an object, oldest
to newest. `append_object`'s `(id, content_signature)` versioning (RFC 0015 — confirmed by reading
`append_versioned` directly, not assumed) already deduplicates identical content, so a claim's
history only grows when content genuinely changes. A role `Claim`'s id is already deterministic
per crate (RFC 0067's `role_claim_kir_id`). Put together: if `architecture-reasoning` is ever
re-run and classifies a crate differently, that difference is *already* sitting in the ledger's
version history, with no new field, table, or write path needed. RFC 0068's own drift definition —
documented claim vs. current evidence — turned out to just *be* "compare the first and last
version of something the ledger already versions." The plan's original sketch (extend `ekos diff`)
was the wrong primitive; `object_history` was the right one, found by reading the `KnowledgeStore`
trait directly before committing to the `ekos diff` approach the plan had guessed at.

**Kept `recovery` free of a new ledger dependency, on purpose.** `crates/recovery` has never read
the ledger — every existing pass only ever produces KIR flowing forward through compile→commit.
Adding `ekos-ledger` as a `recovery` dependency just for one drift-comparison function would be a
real, unusual architectural change for a small feature. Instead `drift_from_history` stays a pure
function over an already-fetched `&[KirObject]` history, and `cli` (which already has ledger
access via `open_store`) does the fetching. This split — `cli` does I/O, `recovery` does the
comparison — was a genuine, small architectural decision made during implementation, not something
the plan called out in advance; found by checking whether `recovery`'s `Cargo.toml` already
depended on `ekos-ledger` (it didn't) before adding new coupling.

**Drift findings are reported, not scored.** RFC 0068 §32 marks drift as high severity, but this
increment doesn't fold a drift count into `EvaluationReport.score` — there's no real
weight-calibration data yet for how much one drift finding should move a composite completeness/
evidence-coverage score, and inventing one would be exactly the "unsupported precision" this
project's own RFCs (0065 §4.6) already name as the thing not to do. Findings print separately in
`ekos architecture investigate`'s final report, in RFC 0068 §32's own human-readable
"DOCUMENTATION DRIFT DETECTED" shape.

**Real design question found and deliberately deferred, not glossed over.** The plan's next
increment was going to be System Context + Basic Component View + Technology Inventory together.
Investigating Basic Component View mid-planning found that `Custom("Crate")` (RFC 0042) and
`File`/`RustSymbol` (RFC 0041) objects **aren't linked in the graph at all** — `Contains` edges run
File→RustSymbol, never Crate→File. "Show a container's internal components" needs a real decision
(new extractor-side relationship vs. render-time path matching) before it's a render function.
Rather than rush a shallow version to hit a feature count in one turn, it's the explicit next
increment — logged in TODO.md, not dropped.

### Live verification — no new pipeline run needed

This repo's own real, already-committed ledger (from this session's earlier RFC 0067 work, which
ran `architecture-reasoning` more than once with genuinely different results for several crates)
already contained real drift. A one-off test (added, run, then removed — never meant to survive)
called `detect_drift` directly against the real `.ekos/` store:

```
REAL DRIFT FINDINGS: 7
  ekos-semantic : core library -> shared utility
  ekos-runtime : core library -> plugin/connector
  ekos-kir : shared utility -> core library
  ekos-marketing : shared utility -> test support
  ekos-scheduler : shared utility -> plugin/connector
  ekos-simulation : shared utility -> plugin/connector
  ekos-ledger : core library -> shared utility
```

Genuinely real: `ekos-kir` legitimately reads as either "shared utility" (it's a types crate,
depended on by nearly everything) or "core library" (it defines the foundational KIR types the
whole compiler pipeline is built around) — a small local model landing on different sides of that
real ambiguity across two runs is exactly the kind of honest signal drift detection exists to
surface, not a bug in either classification.

`ekos docs generate --layout curated` against the same real ledger rendered a real `## System
Context` section with real dependency names (anyhow, axum, clap, chrono, docx-rs, ...) as
System→Technology edges — confirmed directly in the generated `Architecture.md`.

---

## Knowledge Captured

- **Before extending an existing command (`ekos diff`) to solve a new problem, check whether the
  underlying store already has a lower-level primitive that solves it more directly.**
  `object_history` was already there, already exercised by RFC 0047's own tests, and needed zero
  new code in `ekos-ledger` at all — only found by reading the `KnowledgeStore` trait's full method
  list, not by assuming `ekos diff` was the only tool for a "what changed" question.
- **A feature whose real signal already exists in already-committed data doesn't need a new live
  pipeline run to verify — it needs a query.** Saved a genuine ~20-30 minute live-verification cost
  (this repo's real ledger is large enough that `compile`/`commit` take real time) by checking
  whether the real data already answered the question before generating more of it.
- **When a planned increment turns out to bundle a real design question with two simple ones,
  split it rather than rushing a shallow answer to the hard part.** Basic Component View's
  Crate↔File linkage gap was found mid-session, not in the original plan — deferring it explicitly,
  with the reason on record in TODO.md, is different from silently cutting it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0069-system-context-and-documentation-drift.md` | New RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Increment 1 status note |
| `ekos/crates/docs-gen/src/lib.rs` | New `## System Context` section + `render_system_context`; 3 new/updated tests |
| `ekos/crates/recovery/src/architecture_drift.rs` | New: `DriftFinding`, `drift_from_history`; 6 tests |
| `ekos/crates/recovery/src/architecture_reasoning.rs` | `role_claim_kir_id`: `pub(crate)` → `pub` |
| `ekos/crates/recovery/src/lib.rs` | Export `architecture_drift`'s public items |
| `ekos/crates/cli/src/commands/architecture.rs` | `detect_drift`, wired into the final report |
| `TODO.md` | RFC 0068 §61 MVP items ticked off; next increment scoped |
| `devlogs/devlog_72.md` | This file |

# Devlog 204 — EKOS Migrate: the full RFC set (0154–0167)

**Date:** 2026-09-24
**PRs:** none yet — design-only session, no code
**Branch:** main (working tree, uncommitted)

---

## Summary

An external solution plan for evidence-backed PostgreSQL → ClickHouse / Delta migration was reviewed
against the repository and turned into fourteen RFCs, 0154 through 0167. The review invalidated four
of the plan's cited prerequisites and two of its architectural assumptions; both of the latter
changed the shape of the work rather than its details. No code was written — per the Mandatory
Development Workflow, no phase starts before its RFC is accepted, and none of these is accepted yet.

The central design commitment is the ordering: the validator comes before the connector, and the
deterministic PL/pgSQL parser comes before any model is allowed to translate logic. Both orderings
exist for the same reason — the failure mode of getting them wrong is a *false green*, which is the
one outcome this system cannot ship.

---

## The review — what the source plan got wrong

The plan (`~/Downloads/ekos-migrate-plan.md`, 657 lines) was well-constructed and its positioning
was sound. Six claims about this repository were not.

| Claim | Reality |
|---|---|
| "Prerequisite: RFC 0137 audit-trail provenance — confirm status in Phase 0" | **RFC 0137 does not exist.** Genuine gap between 0136 and 0138. The real dependency is RFC 0135 Part B, already fully shipped (devlogs 158–161), so the plan's largest stated unknown was already resolved. |
| "RFC 0136 id-determinism conventions" | 0136 is `web-console-phase-6-graph-v2`. Determinism is RFC 0135 Part C. |
| "doc-vs-data conflicts emitted through the `ConflictingEvidence` diagnostic path" | No such path. The only conflict machinery is `identity::ConflictKind::SameNameDifferentKind`, a different problem. New work, and one of the plan's more distinctive features. |
| "builds on RFC 0150's characterization discipline (`ekos-characterize`)" | That crate is in the **private** `alexeyban/ekos-binary` workspace. Public crates must never depend on it (RFC 0149), so the planted-control discipline is re-implemented publicly in RFC 0156. |
| "crates plug in through the RFC 0149 extension seam" | **They cannot.** `EkosExtension` has four hooks — `observers`, `recovery_passes`, `after_commit`, `mcp_tools`/`call_mcp_tool` — no CLI-subcommand hook, and the MCP hook is deliberately read-only. |
| "PL/pgSQL functions parse into the Transformation IR" | They do not, and `sql_transform_analyzer.rs` says so in its own comments. |

---

## Decision 1 — in-tree and public, not private behind the seam

The seam gap forced a choice the plan had deferred to an open question. Three options: ship in-tree
and public; write an extension-seam RFC first (a `subcommands()` hook plus a write-capable tool
variant) and ship privately; or split public fact model from private executor, which needs the
second option anyway.

**Chosen: in-tree and public**, with the reversal path documented. Shipping privately would mean
paying a design cost up front for a monetization split nobody has decided on. If Migrate is later
sold separately, the seam RFC plus moving the crates is the same mechanical move RFC 0149 already
performed once for the binary decompiler, and nothing in the fact model or CLI shape depends on the
answer.

---

## Decision 2 — the validator ships before the connector

The source plan scheduled the validation engine fifth, behind Discover, Profile, Assess and Map —
all of which are commoditized elsewhere. But canonical serialization, bucketed checksums and the
planted-defect suite need only two engines and hand-written fixture tables. They do not need the
catalog introspector, the profiler or the DQ engine.

So RFC 0155 (canonical serialization) and RFC 0156 (tiers, bisect, controls) come before RFC 0157
(the live connector). If cross-engine hash canonicalization is subtly wrong — the single largest
technical risk in the whole design, because its failure mode is two different databases agreeing —
that surfaces in weeks against fixtures rather than in month five against real data.

---

## Decision 3 — full PostgreSQL coverage is binding, which forces a real PL/pgSQL parser

The first draft of RFC 0154 listed PL/pgSQL logic migration as a non-goal. The maintainer rejected
that: **it must support all features of PostgreSQL.** The RFC was rewritten so that no part of the
PostgreSQL surface is a non-goal, with a precise definition of what "supported" means:

> Every source object is recovered into facts, classified, and given a disposition. It is either
> translated to the target with evidence, or it is a finding a named human dispositioned. Nothing is
> silently dropped, and nothing is silently approximated.

That definition is what makes the requirement achievable without lying. PostGIS geometry, custom C
functions, RLS and triggers have no ClickHouse or Delta equivalent — no tool translates them. Under
this definition they are still supported: recovered, reported with exact objects and affected rows,
and blocked behind an R3 disposition. RFC 0158 carries the mechanical check, and the asymmetry is
explicit: **"no rule matched this object" is a failure of the check, not a pass.**

The cost is RFC 0163: a real, deterministic, in-process PL/pgSQL parser. It cannot be skipped, and
it cannot come second. See *Knowledge Captured*.

---

## The RFC set

| RFC | Title |
|---|---|
| 0154 | EKOS Migrate: architecture, state machine and fact model |
| 0155 | Canonical value serialization and cross-engine checksums |
| 0156 | Validation tiers, bisect, divergence classification and planted controls |
| 0157 | Live PostgreSQL connector, profiling tiers, and redaction at the live entry point |
| 0158 | Data-quality rules, target compatibility, and the source-coverage completeness check |
| 0159 | Type mapping registry and ClickHouse target design |
| 0160 | Guarded execution: statement classifier, environments and data movement |
| 0161 | Risk classes, approval workflow and evidence snapshots |
| 0162 | Evidence-backed migration report and the groundedness gate |
| 0163 | PL/pgSQL → a procedural IR, deterministically |
| 0164 | Logic lowering, constrained reconstruction and differential execution |
| 0165 | Delta Lake / Spark SQL as a second target |
| 0166 | Incremental sync, CDC, parallel run and cutover |
| 0167 | Migration agent pack and MCP tool surface |

---

## Knowledge Captured

**An anti-invention check against an `Unmapped` node passes on everything.** This is the finding that
reshaped the roadmap. RFC 0164 wants to verify that generated logic invents nothing by requiring
every generated predicate to map to a source IR node. But `sql_transform_analyzer.rs` turns a
`CREATE FUNCTION … AS $$…$$` body into a single opaque string literal (lines 650–657), procedure
bodies fail whole-file structured parsing (lines 13–17), and the output is partial/duplicate
`Unmapped` fragments because "the procedure's control flow was never going to be modeled"
(lines 310–312). So *everything* maps, the check passes, and the system reports green on invented
logic — strictly worse than having no check, because it manufactures confidence. The fix is not to
narrow scope but to fix the order: build the deterministic parser first (0163), then let a model
near it (0164). Exactly the RFC 0148 → 0150 sequence, where a real in-process CIL decoder came
before any LLM reconstruction, and that ordering is why it worked.

**Don't force imperative semantics into a dataflow IR.** The obvious move for 0163 was adding
control-flow variants to `TransformNode`. Wrong: that enum is a dataflow graph shared by Pentaho and
plain SQL, and every existing consumer would inherit node kinds meaningless in its own domain.
Instead, a separate `ProcedureIr` owns order and condition, and each embedded SQL statement holds an
ordinary `TransformGraph`. `ekos_transformation_explain`, `ekos_transformation_diff` and the dbt
emitter keep working unchanged and gain procedure bodies for free.

**Hashing is not anonymization for top-k.** The source plan stored profile top-k values as hashes to
avoid persisting row data. On a low-cardinality column (`status`, `country`, `gender`) an attacker
with the hash and the domain recovers every value by enumeration instantly. RFC 0157 therefore
persists **no top-k at all** for PII-classified columns — not values, not hashes — and salts the
rest per project. This matters more here than elsewhere because the ledger is append-only: there is
no way to un-commit personal data.

**The live PostgreSQL reader is a third raw-content entry point.** CLAUDE.md documents two —
the `Observer` path and `recover.rs`'s direct file reads — each independently calling
`ekos_common::redaction`. A live profiler reading sample values is a third, and the source plan
never mentioned redaction at all. Any future live connector inherits this obligation.

**The identity CI guard does not cover new crates.** `every_pipeline_custom_kind_is_registered`
(`crates/identity/src/lib.rs:1680`) scans only `recovery/src` and `semantic/src`. A new
`ekos-migrate` crate emitting ~22 `Custom(_)` kinds is invisible to it, so the guard's whole purpose
is silently lost. `Session`/`SessionClaim` are the precedent — registered by hand, with no guard
behind them. The scan list must be extended in the same PR that adds the crate.

**`validate_select_only` is a statement classifier in miniature.** `clickhouse-query/src/validate.rs`
is 99 lines that parse through the dialect SDK and hard-reject anything but a single SELECT, with
tests for INSERT/UPDATE/DELETE/DROP/multi-statement/unparseable. RFC 0160 generalizes the return
from a bool to a `StatementClass` enum rather than writing a classifier from scratch. Its key
behaviour — **unparseable is a refusal, not a pass-through** — is preserved exactly.

**Absence beats a permission check.** RFC 0151's live MCP test (devlog_199) found session isolation
applied to 2 of ~20 Runtime read methods and six tools leaking claims. Nothing malicious; the
enforcement was simply in the wrong layer. So the migration surface has *no handler* for approve,
execute or sign-off, plus the source-scanning test copied from
`no_mcp_code_can_reach_the_lifecycle_module` — an absent capability is verifiable, a correct check
is only probable. The `Actor` enum deliberately has no `Agent` variant, for the same reason
`session::lifecycle::Actor` has none.

**A whole-list assertion, not a forbidden-name check.** The RFC 0167 tool-list test asserts the
*exact* tool set. An allowlist that only forbids what someone thought of is precisely how the
RFC 0151 leak happened.

**ClickHouse `MATERIALIZED VIEW` is a trigger, not a maintained projection.** It fires on insert to
its source table and never sees pre-existing rows or updates. Lowering a PostgreSQL matview to one
silently changes semantics, so RFC 0164 emits a refreshable MV where the version supports it and a
compatibility finding otherwise — never the convenient option.

**`ReplacingMergeTree` dedup is eventual, and that breaks naive validation.** A `SELECT` before a
merge returns duplicates, which makes "the merge has not happened yet" and "the load duplicated
rows" indistinguishable — and only one of those is acceptable. RFC 0156 requires dedup-aware reads,
and RFC 0159 makes the eventual-dedup compatibility finding mandatory whenever the engine is chosen.

**XOR is the wrong checksum combinator here.** It is order-independent and overflow-free, which
makes it tempting, but a duplicated row cancels itself out — hiding exactly the defect
`ReplacingMergeTree` makes likely. RFC 0155 uses a `(count, sum)` pair instead.

**Timestamp watermarks are silently lossy under clock skew.** With multiple writers, a wall-clock
watermark is not reliably monotonic and drops rows. RFC 0166 prefers `xmin`/snapshot bounds because
they are transactional rather than temporal, and the test suite *demonstrates* the timestamp
approach losing rows rather than merely asserting the better one works.

**Profiles describe the past, not the schema's future.** `narrowing-safe` type mapping is the
profiler's biggest payoff — an unconstrained `numeric` whose measured max scale is 2 becomes
`Decimal(18,2)`. But applying the same logic to an identity column with a small observed maximum
narrows a column that keeps growing. RFC 0159 distinguishes *bounded* domains from *growing* ones
and refuses to narrow the latter on profile evidence alone.

**Spark's calendar rebase silently shifts pre-1582 dates** when
`spark.sql.parquet.datetimeRebaseModeInWrite` and its read counterpart disagree. Trivial to prevent
by pinning both, miserable to diagnose later. RFC 0165 pins them per session and records them on
every execution fact.

**An unconsumed logical replication slot can fill a production disk.** RFC 0166 treats slot lag as a
first-class monitored metric, drops slots EKOS created on teardown, and reports orphans via
`ekos doctor`. It explicitly rejects ClickHouse's experimental `MaterializedPostgreSQL` because that
moves slot lifecycle inside the target engine, where EKOS cannot bound it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0154-ekos-migrate-architecture.md` | New — foundation: placement outside the compiler, in-tree/public decision, fact model, state machine, binding PostgreSQL coverage requirement |
| `ekos/docs/rfcs/0155-canonical-serialization-and-checksums.md` | New — per-type canonical form, row hash, bucket checksum, three-engine golden tests |
| `ekos/docs/rfcs/0156-validation-tiers-and-planted-controls.md` | New — V0–V6, independent oracle, bisect, divergence classification, control suite |
| `ekos/docs/rfcs/0157-live-postgresql-connector-and-profiling.md` | New — catalog introspection, drift, P0/P1/P2, redaction at the live entry point |
| `ekos/docs/rfcs/0158-data-quality-and-compatibility-rules.md` | New — DQ families, inferred FKs, compatibility matrix, completeness check |
| `ekos/docs/rfcs/0159-type-mapping-and-clickhouse-target-design.md` | New — lossiness classes, profile specialization, CH engine/ORDER BY/partitioning |
| `ekos/docs/rfcs/0160-guarded-execution-and-data-movement.md` | New — `StatementClass`, environments, artifact pinning, chunked resumable loads |
| `ekos/docs/rfcs/0161-risk-classes-and-approval-workflow.md` | New — computed risk, evidence snapshots, human-only enforcement, supersede |
| `ekos/docs/rfcs/0162-migration-report-and-groundedness-gate.md` | New — compiled report, citation verification, five sign-off preconditions |
| `ekos/docs/rfcs/0163-plpgsql-procedural-ir.md` | New — deterministic PL/pgSQL parser, `ProcedureIr`, fidelity labels, trigger classification |
| `ekos/docs/rfcs/0164-logic-lowering-and-differential-execution.md` | New — lowering, constrained reconstruction, V5 differential execution |
| `ekos/docs/rfcs/0165-delta-spark-target.md` | New — Delta type map, execution backends, delta-rs independent reader |
| `ekos/docs/rfcs/0166-incremental-sync-cdc-and-cutover.md` | New — watermarks, deletes, CDC, parallel run, cutover checklist |
| `ekos/docs/rfcs/0167-migration-agent-pack-and-mcp-surface.md` | New — MCP surface, layered enforcement, subagents, containment evals |
| `TODO.md` | New "EKOS Migrate" section: Migrate Phases 0–9 with per-phase checklists |

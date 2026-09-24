# RFC 0154 — EKOS Migrate: architecture, state machine and fact model

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0135 (provenance & determinism — Parts B/C/D are hard dependencies),
RFC 0151 (session memory — the human-only lifecycle and supersede precedent),
RFC 0149 (extension seam), RFC 0027/0028 (Transformation IR), RFC 0043 (redaction),
RFC 0056 (ClickHouse connector and the SELECT-only guard), RFC 0018 (`ekos_impact`),
RFC 0131 (console OIDC + role split), RFC 0138 (eval harness), RFC 0150 (planted-control discipline)

---

## Summary

EKOS Migrate moves a PostgreSQL database to an analytical target and **proves** the result: every
scope decision, data-quality finding, type mapping, generated artifact, execution, validation
result and human approval is a ledger fact with provenance, and the final report cites those facts
or does not ship.

It covers the **whole PostgreSQL surface** — including views, materialized views, triggers and
PL/pgSQL functions and procedures. Every source object is recovered, classified and dispositioned;
nothing is silently dropped or approximated. See *Requirement: the whole PostgreSQL surface* for
what that demands, including the deterministic PL/pgSQL parser it requires EKOS to build.

This RFC establishes only the foundation: where Migrate sits relative to the compiler, how it
ships, what its facts are, how an append-only ledger expresses a state machine and a retraction,
and who is allowed to approve. It deliberately does **not** specify validation tiers, canonical
serialization, the live PostgreSQL connector, type mappings or execution paths. Those are RFCs
0155–0167, listed in *Gated work* below.

**One-line positioning:** EKOS Migrate proves the new platform does what the old one did — or
differs exactly where, and only where, a named human decided it should.

## Motivation

DDL and SQL conversion is commoditized (Lakebridge, SnowConvert, AWS SCT/DMS, and others — verify
the landscape before publishing any comparison). What stays expensive and manual is evidence:

1. What exactly are we migrating, and what depends on it?
2. What is wrong with the source data, and what did we decide to do about each problem?
3. Is the target provably equivalent to the source, except where we deliberately decided otherwise?
4. Who approved each lossy decision, and on what evidence?

EKOS already owns the substrate for all four: an append-only ledger with per-write provenance
(RFC 0135 Part B), cross-system impact analysis over schema ↔ code ↔ ETL ↔ docs (RFC 0018), a
Transformation IR that diffs legacy against new logic (RFC 0027/0028), and the RFC 0150 discipline
that a checker which cannot catch a planted defect does not get to report green. What is missing is
a subsystem that drives a migration through those facts instead of around them.

## Position in the architecture

Migrate is an **auxiliary subsystem**, beside the compiler, not inside it — the same position
`simulation`, `marketing` and `session` occupy. This matters because Migrate is the first component
in EKOS that **writes to systems outside the workspace**, and every existing invariant is read-only
or side-effect-free. Stating the position explicitly is what keeps those invariants true rather than
quietly broken:

| Invariant | How Migrate honours it |
|---|---|
| Compiler passes are deterministic and side-effect-free | No part of Migrate is a `CompilerPass`. It runs *after* `commit`, over the compiled CKM. `build/recover/resolve/compile/commit` are unchanged. |
| The Runtime is read-only | Migrate never writes through `Runtime`. It reads the CKM through `Runtime` and writes its own facts through `&dyn KnowledgeStore`, the access level `commit.rs`, `simulation` and `session::commit` already have. |
| AI consumes knowledge through the Runtime only | MCP reaches Migrate's *read* and *propose* tools. No MCP tool reaches the executor, the approver or the sign-off. Enforced by a source-scanning test (below), not by prompt. |
| The ledger is append-only | State changes re-append the object and append an event. Nothing is deleted or edited in place. |
| Every conclusion traces to Evidence | Profile, finding, validation and approval facts carry `query_sha256`, `result_sha256`, engine and engine version alongside RFC 0135 provenance. |
| Secrets and PII are never observed or stored | The live source reader is a **third raw-content entry point** and calls `ekos_common::redaction` like the other two. See *Security*. |

The executor mutating a third-party database is therefore a property of an auxiliary tool the user
invokes, not of the knowledge compiler. A reviewer who reads only this section should come away
knowing that the compiler's guarantees are untouched.

## Non-goals

- Not a general-purpose data mover (Debezium, Airbyte, Fivetran, Spark JDBC). Migrate selects and
  drives engine-native paths and verifies the outcome.
- Not a scheduler or orchestrator. It can emit jobs for one.
- **No autonomous cutover.** The final switch is always a human action.
- **No row-level source data in the ledger, ever.** Aggregates, hashes and masked key references only.
- Not a BI or semantic-layer migration (dashboards, LookML).
- **Not two targets at once.** ClickHouse first; Delta/Spark is RFC 0165.

Note what is *not* on this list: no part of the PostgreSQL surface is a non-goal. See
*Requirement: the whole PostgreSQL surface*.

## Design

### Shipping decision: in-tree and public

Migrate ships as public crates in the `ekos/` workspace, alongside `simulation` and `marketing`.

The obvious alternative — shipping it privately through the RFC 0149 extension seam, as
`alexeyban/ekos-binary` does — **is not currently possible**. `EkosExtension`
(`crates/cli/src/extension.rs`) offers exactly four hooks: `observers`, `recovery_passes`,
`after_commit`, and `mcp_tools`/`call_mcp_tool`. There is no hook for new CLI subcommands, and
`call_mcp_tool` is deliberately read-only (`&dyn KnowledgeStore`). A subcommand tree with
write-capable proposal tools cannot plug in as the seam stands.

Making it possible would mean an extension-seam RFC (a `subcommands()` hook plus a write-capable
tool variant) before any migration work starts — paying a design cost up front for a monetization
split that has not been decided. The reversal path is cheap and stays open: if Migrate is later sold
separately, that seam RFC plus moving the crates is the same mechanical move RFC 0149 already
performed once, and nothing in this RFC's fact model or CLI shape depends on the answer.

**Consequence for RFC 0150 reuse:** `ekos-characterize` (`crates/characterize`) lives in the private
workspace and public crates must never depend on it. The planted-defect control discipline is
therefore **re-implemented publicly** in RFC 0156 against the same design, not imported.

### Crate layout (this RFC delivers the first row only)

| Crate | Responsibility | RFC |
|---|---|---|
| `ekos-migrate` | Project model, migration units, state machine, transitions, risk/approval records, `ekos migrate` CLI | 0154 |
| `ekos-migrate-validate` | Canonical serialization, tiers, bisect, planted controls | 0155/0156 |
| `ekos-pg-live` | Live catalog introspection, profiling tiers, redaction at the entry point | 0157 |
| `ekos-migrate-dq` | DQ rules, target compatibility, the completeness check | 0158 |
| `ekos-migrate-target-clickhouse` | Type mapping, target design, DDL emission, execution | 0159/0160 |

`crates/cli/src/commands/migrate.rs` dispatches from `app.rs`, one file per subcommand tree as every
other command does.

### Migration unit and state machine

A **migration unit** is a table, a view, a function, or a group migrated together.

| State | Entered when | Exit condition |
|---|---|---|
| `discovered` | Object found in catalog or CKM | Profile complete |
| `profiled` | Profile facts committed | Assessment complete |
| `assessed` | DQ + compatibility findings committed | Every blocking finding has a disposition |
| `planned` | Assigned to a wave with a target design | Mapping proposal exists |
| `mapped` | Type/schema/logic proposal committed | Approval granted where required |
| `approved` | Human approval fact committed | Artifacts generated |
| `generated` | DDL/load artifacts committed | Dry run passes |
| `dry_run_passed` | Sandbox run + structural/count tiers pass | Execution approved |
| `loaded` | Load finished | Validation completes |
| `validated` | Required tier reached, zero unexplained divergences | Included in the report |
| `diverged` | Unexplained divergence found | Reconciled → back to `mapped` or `generated` |
| `syncing` | Incremental sync active | Parallel-run window closes clean |
| `signed_off` | Human sign-off fact committed | terminal |

**How an append-only ledger holds mutable state.** A transition does two writes, copying
`session::lifecycle` exactly:

1. The `MigrationUnit` object is **re-appended** with the new `state` property — a new ledger
   version, the old one still readable through `ekos ledger audit`.
2. A `MigrationTransition` event is appended recording `from`, `to`, actor, reason and the fact ids
   the decision rested on.

Reading a unit's state is a point lookup, never a fold over the event log; the event log is the
audit trail, not the source of truth. This is the same split that makes session claim status cheap
to read and fully historical to audit.

### Fact model

Every entity is an `ObjectKind::Custom(_)` with a `Migration` prefix. The prefix is not cosmetic:
identity's `normalize()` lowercases, so bare names like `Disposition`, `Divergence` or
`ValidationRun` are collision bait against future analyzer kinds — the exact failure RFC 0147 hit
with `PerlPackage`/`PerlSymbol`.

| Kind | Key attributes | Produced by |
|---|---|---|
| `MigrationProject` | name, source ref, target ref, policy, created_by | CLI |
| `MigrationConnectionRef` | kind, host alias, database — **never credentials** | CLI |
| `MigrationUnit` | object refs, wave, state | Orchestrator |
| `MigrationTableProfile` | row count (exact/estimated), size, last analyze, write rate | Profiler (0157) |
| `MigrationColumnProfile` | null rate, approx distinct, min/max, length stats, top-k **hashes**, pattern classes | Profiler (0157) |
| `MigrationPiiClassification` | column, class, method, confidence | Profiler (0157) |
| `MigrationDqFinding` | rule, severity, object, evidence query, measured value, threshold | DQ (0158) |
| `MigrationCompatFinding` | target, rule, object, affected row estimate, lossiness | DQ (0158) |
| `MigrationDisposition` | finding, decision, actor | human, on an agent proposal |
| `MigrationTypeMapping` | source type, target type, lossiness class, rule id | Mapper (0159) |
| `MigrationTargetDesign` | engine, ordering/partitioning, rationale, evidence | Mapper (0159) |
| `MigrationProposal` | source IR ref, target IR, generated SQL hash, author, status | Mapper (0159) |
| `MigrationArtifact` | kind, dialect, content hash, path | Generator (0160) |
| `MigrationExecution` | artifact, environment, rows affected, duration, status | Executor (0160) |
| `MigrationValidationRun` / `MigrationValidationResult` | tier, engine paths, expected, actual, tolerance | Validator (0156) |
| `MigrationDivergence` | unit, bucket/key refs (masked), classification, status | Validator (0156) |
| `MigrationControlResult` | planted defect id, detected, tier | Validator (0156) |
| `MigrationRiskAssessment` | action, risk class, blast radius, reasons | Risk (0161) |
| `MigrationApprovalRequest` / `MigrationApprovalRecord` | action, risk, requester, approver, decision, evidence snapshot hash | Approval (0161) |
| `MigrationReport` | version, snapshot hash, groundedness score, signed_off_by | Report (0162) |

Events: `MigrationTransition`, `MigrationStatusChanged`.

**Identity obligations — do these in Phase 0, not when the bug appears.** Every kind above is
structurally keyed (project + unit + tier + engine); two instances are never the same real entity.
Each therefore needs a row in `ekos_kir::custom_kinds::REGISTRY` with `structurally_keyed: true`, or
`DefaultResolver`'s same-kind `structural_score` fallback of 1.0 will collapse a whole migration's
units into one canonical object. That over-merge has been re-diagnosed live roughly a dozen times.

The CI guard (`every_pipeline_custom_kind_is_registered`, `crates/identity/src/lib.rs:1680`) scans
only `recovery/src` and `semantic/src`, so a new `ekos-migrate` crate is **not covered**. The same PR
that adds the crate adds `migrate/src` to that scan list. `Session`/`SessionClaim` are the precedent:
registered although written outside the pipeline, but registered by hand, with no guard behind them.

### Write path and provenance

Migrate writes through `&dyn KnowledgeStore` with an RFC 0135 Part B `WriteContext`:

- `run_id` — one per `ekos migrate <verb>` invocation, shared by every write that verb makes.
- `stage` — `"migrate:profile"`, `"migrate:validate"`, `"migrate:approve"`, and so on, joining the
  existing `"build"` / `"commit:rollup"` / `"identity-review"` vocabulary.
- `source_artifact_id` — the observation artifact where one exists.

`ekos ledger audit` and the `ekos_audit` MCP tool then explain any migration fact for free.
Relationship determinism follows RFC 0135 Part C.

### Supersede, never delete

The ledger has no object-level delete or tombstone, so a wrong approval, a stale proposal or a
disposition made on evidence that has since changed cannot be removed. It is **superseded**: the
object is re-appended with `status: "superseded"` plus a pointer to its replacement, and a
`MigrationStatusChanged` event records who did it and why. `session::lifecycle` is the working
implementation of this shape.

An `MigrationApprovalRequest` freezes an evidence snapshot (fact ids plus a hash). If the underlying
evidence changes, the hash no longer matches and the request cannot be approved — it must be
re-requested. Approval is pinned to evidence, not to intent.

### Risk classes and who may approve

| Class | Examples | Gate |
|---|---|---|
| R0 | Catalog reads, cheap profiling, proposals | none |
| R1 | Sandbox writes, full scans within budget | automatic within policy, logged |
| R2 | DDL and loads in staging, starting incremental sync | one approver |
| R3 | Lossy mapping, dedup, NULL-handling change, schema rename/split, logic change | approver + evidence review |
| R4 | Anything touching a production target, `DROP`/`TRUNCATE`/`DELETE`, cutover, sign-off | two approvers, typed confirmation |

Risk is **computed**, not guessed: statement class × environment × lossiness × blast radius
(`ekos_impact` over views, functions, ETL and app code) × affected-row estimate. Full specification
in RFC 0161.

**Approval is human-only, and it is enforced in code.** Approving, executing outside the sandbox and
signing off live in a `lifecycle` module that `commands/mcp.rs` may not reference, checked by a
source-scanning test copied from `no_mcp_code_can_reach_the_lifecycle_module`
(`crates/cli/src/commands/session.rs:531`). MCP exposes read tools and propose/request tools only.
A tool allowlist in a subagent prompt is not a control; this is.

Approver identity reuses the console's RFC 0131 OIDC and role split rather than inventing one. A CLI
device flow stays an open question precisely because routing approvals through the console makes it
optional.

### Surfaces

```text
ekos migrate init | discover | profile | assess | plan | map
ekos migrate review | approve <request> | reject <request> --reason
ekos migrate generate | dry-run | load | validate | diff <unit>
ekos migrate sync start|status|stop | report | signoff <version> | status
```

MCP, read: `ekos_migration_status`, `ekos_migration_profile`, `ekos_dq_findings`,
`ekos_compat_findings`, `ekos_mapping_explain`, `ekos_validation_results`,
`ekos_divergence_explain`, `ekos_migration_report`.
MCP, propose/request only: `ekos_disposition_propose`, `ekos_mapping_propose`,
`ekos_target_design_propose`, `ekos_approval_request`.
Never over MCP: approve, execute outside the sandbox, sign off.

## Security

1. **Credentials** live in environment variables, the OS keychain or a cloud secret manager. The
   ledger holds only `MigrationConnectionRef` aliases. A CI test scans the ledger for secrets.
2. **Redaction.** The live source reader is a third raw-content entry point, after the `Observer`
   path and `recover.rs`'s direct reads, and like both of them it calls `ekos_common::redaction`
   before anything is hashed, logged or held. RFC 0043's baseline is not disable-able here either.
3. **Top-k hashes are not anonymization.** On a low-cardinality column a hash is trivially reversed
   by enumeration. Top-k is withheld entirely for columns classified as PII, not merely hashed.
4. **Source safety.** A dedicated read-only role, `default_transaction_read_only`, statement and
   lock timeouts, replica preferred, replica-lag guard, and an `EXPLAIN` cost estimate before any
   full scan.
5. **Statement classification.** Every generated or LLM-authored statement is parsed and classified
   before it can be executed; unparseable statements are rejected, never executed. This generalizes
   `validate_select_only` (`crates/clickhouse-query/src/validate.rs`), which already parses through
   the dialect SDK and hard-rejects anything but a single SELECT, with tests for INSERT, UPDATE,
   DELETE, DROP, multi-statement batches and unparseable input.
6. **Environments.** Sandbox, staging and production are first-class. Production credentials are
   unavailable to the executor unless an R4 approval is active for that exact action, and the
   executed artifact's hash must equal the approved artifact's hash.
7. **Audit.** Every action, approval and transition is a fact with actor identity — the OIDC subject
   for a human, the RFC 0151 session id for an agent.

## Requirement: the whole PostgreSQL surface

**Every PostgreSQL feature a source database uses is in scope.** Not "tables plus whatever views
happen to parse" — the whole surface: tables, partitioned tables, views, materialized views,
sequences and identity, constraints, indexes, enums, domains, composite and range types, arrays,
`jsonb`, extensions, comments, grants, RLS policies, triggers, and SQL, PL/pgSQL and PL/* functions
and procedures.

"Supported" has one precise meaning here, and it is stronger than "translated":

> Every source object is **recovered into facts, classified, and given a disposition**. It is either
> translated to the target with evidence, or it is a finding a named human dispositioned. Nothing is
> silently dropped, and nothing is silently approximated.

That distinction is load-bearing, because some PostgreSQL features have no target equivalent at all
— PostGIS geometry, custom C functions, RLS, deferred constraints, triggers as an enforcement
mechanism. For those, "support" is: recovered, reported with the exact objects and affected rows,
and blocked behind an R3 disposition until a human decides. A migration that reaches `signed_off`
with an unclassified source object is a defect in Migrate, not a limitation of it. RFC 0158 carries
the completeness check: **every catalog object reachable in the source resolves to a fact and a
disposition**, asserted against LedgerSMB and Pagila, which between them exercise PL/pgSQL, enums,
arrays, full-text, partitions and triggers.

### What this costs: PL/pgSQL has no IR yet, so one gets built

The Transformation IR handles `SELECT` and view bodies today. It does **not** handle PL/pgSQL, and
`crates/recovery/src/sql_transform_analyzer.rs` is explicit about it in its own comments: a
`CREATE FUNCTION … AS $$…$$` body is a single opaque string literal to `sqlparser` (lines 650–657),
procedure bodies fail whole-file structured parsing (lines 13–17), and the result is
partial/duplicate `Unmapped` fragments because "the procedure's control flow was never going to be
modeled" (lines 310–312).

This is a gap to close, not a reason to narrow scope — but it has to be closed in the right order,
for a specific reason. An anti-invention check of the form "every generated predicate must map to a
source IR node" is **vacuous** against `Unmapped`: everything maps, the check passes, and the system
reports green on logic a model invented. Shipping LLM translation of PL/pgSQL *before* the parser
would therefore produce exactly the failure this whole RFC exists to prevent — a confident,
cited-looking, wrong answer.

So the order is fixed, and both halves are required work, not optional:

1. **RFC 0163 — a real PL/pgSQL parser producing a procedural IR.** Deterministic, in-process,
   no LLM: statements, control flow (`IF`/`CASE`/`LOOP`/`FOR`/`WHILE`), exception blocks, cursors,
   `RETURN`/`RETURN QUERY`, variable assignment, dynamic `EXECUTE` (recovered as such, and flagged —
   a constructed statement is a boundary the IR must mark, not silently inline). Fidelity is labelled
   per object, exactly as RFC 0150 labels binary recovery levels, and nothing is labelled fully
   recovered that is not.
2. **RFC 0164 — lowering and constrained reconstruction on top of it.** Target-native views and
   materialized views where the IR lowers cleanly; IR-constrained LLM reconstruction where it does
   not, with the anti-invention check now meaningful because there is a real node set to map against;
   differential execution (same fixtures in PostgreSQL and the target) as the pass/fail oracle.

This is the same shape RFC 0148 → 0150 took for compiled binaries: structure first, then a real
in-process decoder for statements, and only then a model allowed anywhere near the output. It worked
there, and the reason it worked is that the decoder came first.

**Triggers** are recovered and classified (audit, derived column, validation, cascade, mixed) by RFC 0163
and proposed as a redesign item by RFC 0164 — never auto-translated, because neither target enforces
them and a silent translation would change when the logic runs, not just where it lives.

**Sequencing consequence:** RFC 0163 is on the critical path for a complete migration and is
scheduled accordingly in *Gated work*. Simple views, whose bodies the existing IR already handles,
are available earlier — but "we did the easy views" is a milestone, never a finished migration.

## Phase 0 scope and exit criteria

In scope: the `ekos-migrate` crate skeleton — project model, units, the state machine and its two
write shapes, `MigrationConnectionRef` and secret handling, environments, `[migrate]` in
`EkosConfig` (which is `deny_unknown_fields`, so the section must exist before any `ekos.toml` sets
it), REGISTRY rows for every kind above, the identity guard extended to scan `migrate/src`, and
`ekos migrate init` / `status`.

Exit criteria:

- [ ] `ekos migrate init` and `ekos migrate status` work against a PostgreSQL and a ClickHouse
      sandbox. Neither exists in `docker-compose.dev.yml` today; adding them is part of this phase.
- [ ] Every Migrate `Custom(_)` kind has a REGISTRY row, and `every_pipeline_custom_kind_is_registered`
      scans `migrate/src`.
- [ ] A ledger-scan test proves zero credentials and zero row values are persisted.
- [ ] A source-scanning test proves `commands/mcp.rs` cannot reach the approval lifecycle.
- [ ] State transitions are visible through `ekos ledger audit` with a `migrate:*` stage.
- [ ] **The executor's concurrency model is decided and written down.** `KnowledgeStore` is not
      `Sync`; `EkosExtension` is `#[async_trait(?Send)]` and commit "is only ever driven by
      `block_on`, never spawned" (`extension.rs:22-26`). A chunk-parallel executor holding a ledger
      handle across `.await`s collides with this directly. Decide before writing the executor, not
      during.

Explicitly **not** in Phase 0: any live query, any profiling, any generated SQL, any target write.

## Gated work

| RFC | Title |
|---|---|
| 0155 | Canonical value serialization and cross-engine checksums |
| 0156 | Validation tiers, bisect, divergence classification and planted controls |
| 0157 | Live PostgreSQL connector, profiling tiers, and redaction at the live entry point |
| 0158 | Data-quality rules, target compatibility, and the source-coverage completeness check |
| 0159 | Type mapping registry and ClickHouse target design |
| 0160 | Guarded execution: statement classifier, environments and data movement |
| 0161 | Risk classes, approval workflow and evidence snapshots |
| 0162 | Evidence-backed migration report and the groundedness gate |
| 0163 | **PL/pgSQL → a procedural IR, deterministically** — required for full source coverage |
| 0164 | **Logic lowering, constrained reconstruction and differential execution** — required for full source coverage |
| 0165 | Delta Lake / Spark SQL as a second target |
| 0166 | Incremental sync, CDC, parallel run and cutover |
| 0167 | Migration agent pack and MCP tool surface |

**0155 and 0156 come before the connector deliberately.** The validator is the differentiator, and
it needs only two engines and hand-written fixture tables — not the catalog introspector, the
profiler or the DQ engine. If cross-engine hash canonicalization is subtly wrong (the single
largest technical risk here, because its failure mode is a false green), that must surface in weeks,
against fixtures, not in month five against real data.

**0163 and 0164 are not optional.** A migration is not complete while any source object is
unclassified, and PL/pgSQL is where LedgerSMB — the primary corpus — keeps most of its logic. They
sit late in the list because each depends on the validator and the guarded executor being real
first, not because they are a stretch goal.

## Alternatives considered

- **Ship privately through the RFC 0149 seam.** Rejected for now: the seam has no subcommand hook
  and its MCP hook is read-only, so this costs a prerequisite seam RFC to buy an undecided
  monetization split. Reversible later at the same cost.
- **Model migration state as a projection over the transition event log.** Rejected: reading a
  unit's state would require folding every event, and every read path would need that fold. The
  re-append plus event pair gives cheap reads and a complete audit trail.
- **Reuse `ekos-characterize` from the private workspace.** Not possible — public crates must never
  depend on it (RFC 0149). Re-implemented publicly in RFC 0156.
- **Both targets from the start.** Rejected: doubles the typemap, dialect, execution and validation
  surface through every phase. ClickHouse has an existing plugin, dialect crate and HTTP client;
  Delta lands against a validator that already works.
- **Staged Parquet path in the MVP** (`COPY` → Arrow → Parquet → target). Deferred to RFC 0160: it
  is EKOS acting as a data mover, against this RFC's own non-goal, and it pulls in `arrow`,
  `parquet` and object-store credentials. Engine-native pulls only until it is justified.
- **Approval through MCP with a subagent tool allowlist.** Rejected: an allowlist in a prompt is not
  a control. RFC 0151 established the enforced pattern and this RFC copies it.

## Open questions

- [ ] Does a CLI approval need an OIDC device flow, or is routing approvals through the console
      sufficient for v1?
- [ ] Retention policy for per-chunk and per-bucket validation facts at scale — summarize at unit
      level and keep detail only for failed or diverged units?
- [ ] Where do the test corpora (Pagila, TPC-H, Stack Exchange) live, and how does CI get a live
      PostgreSQL? Every corpus in the repo today is static files.
- [ ] Is doc-vs-data conflict (documentation says never null, data says 3% null) its own finding
      kind, or an extension of identity's existing conflict reporting? No `ConflictingEvidence` path
      exists today; `ConflictKind::SameNameDifferentKind` is a different problem.

## Acceptance criteria

- [ ] All open questions resolved or explicitly deferred to a named RFC.
- [ ] Reviewed against the compiler architecture: no new `CompilerPass`, no `Runtime` write, no MCP
      path to approval or execution.
- [ ] The source-coverage requirement is accepted as binding: no PostgreSQL feature is a non-goal,
      and the completeness check (every catalog object → a fact and a disposition) is carried by
      RFC 0158 with RFC 0163/0164 on the critical path, not listed as future work.
- [ ] Phase 0 exit criteria above are all met.
- [ ] `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` clean; `cargo test
      --workspace` green, including the extended identity guard.

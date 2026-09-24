# RFC 0160 — Guarded execution: statement classifier, environments and data movement

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation), RFC 0159 (the DDL and designs executed here), RFC 0161 (risk and
approval, which consumes the statement class), RFC 0056 (`validate_select_only`, generalized here),
RFC 0157 (source read path and throttling), RFC 0156 (validates the result)

---

## Summary

The only component in EKOS that writes to a system outside the workspace. It defines the statement
classifier that decides what a statement *is*, the environment model that decides where it may run,
the artifact-hash pinning that guarantees what executed is what was approved, and the engine-native
data-movement paths.

The governing rule: **nothing executes that was not parsed, classified, approved and hash-matched.**

## Motivation

A migration tool that can run arbitrary generated SQL against a production database is a loaded
weapon, and one increasingly holding LLM-authored statements. "Review the SQL before running it" is
not a control when there are four thousand statements.

EKOS already has the seed of the real control. `crates/clickhouse-query/src/validate.rs` parses
LLM-generated SQL through the dialect SDK and hard-rejects anything that is not a single `SELECT`,
with tests for `INSERT`, `UPDATE`, `DELETE`, `DROP`, multi-statement batches and unparseable input.
That is exactly the right shape — it is simply too narrow, because Migrate legitimately needs to
write. This RFC generalizes it from a boolean to a classification.

## Design

### Statement classifier

```rust
pub enum StatementClass {
    Read,                 // SELECT, EXPLAIN, SHOW
    DdlCreate,            // CREATE TABLE/VIEW/DATABASE
    DdlAlter,             // ALTER, RENAME, ADD/DROP COLUMN
    DmlInsert,            // INSERT, INSERT ... SELECT, COPY INTO
    DmlMutate,            // UPDATE, DELETE, MERGE, ALTER TABLE ... UPDATE
    Destructive,          // DROP, TRUNCATE, DETACH, DELETE without WHERE
    Unknown,              // parsed, but not classified → rejected
}

pub fn classify(sql: &str, dialect: &dyn SqlDialectParser)
    -> Result<Vec<(StatementClass, String)>, ClassifyError>;
```

Rules:

1. **Parse through the dialect SDK**, never by regex or string matching. A classifier that can be
   fooled by a comment or a nested string is not a control.
2. **Unparseable is rejected**, never executed. This is the `validate_select_only` behaviour and it
   is preserved exactly: an unparseable statement is a refusal, not a pass-through with a warning.
3. **Multi-statement input is split and every statement classified.** The batch's class is the
   maximum of its parts, so one `DROP` hidden in forty `INSERT`s classifies the batch as
   `Destructive`.
4. **`Unknown` is rejected.** A statement the classifier parsed but cannot place is treated as
   dangerous, not as benign. Adding a new statement form to the classifier is a deliberate act.
5. **`DELETE` and `UPDATE` without a `WHERE` clause classify as `Destructive`**, not `DmlMutate`.

The class is an input to risk (RFC 0161) together with the environment, so the same `DROP TABLE` is
R1 in a sandbox and R4 in production, with no second code path.

### Environments

`sandbox`, `staging`, `production`, each with its own `MigrationConnectionRef` and credential
reference.

- The executor resolves credentials for an environment **only when an approval matching that exact
  action is active**. A production credential is not loaded into the process at all otherwise; it is
  not a check on a flag, it is an absent secret.
- Sandbox is a separate ClickHouse database or a separate catalog and schema — never a prefix
  convention inside the real one, which is a naming mistake away from disaster.
- Promotion sandbox → staging → production is per unit, by approval, and each promotion re-runs the
  required validation tier in the new environment. A passing sandbox run does not carry forward.

### Artifact pinning

Every generated artifact is a `MigrationArtifact` fact with a content hash. At execution:

```
assert hash(artifact_to_execute) == hash(artifact_named_in_the_approval)
```

A mismatch is a hard failure with no override flag. This closes the gap between "a human reviewed
some SQL" and "that SQL ran" — including the case where a regeneration between approval and
execution changed a statement for a perfectly good reason. The re-generated artifact needs its own
approval, which is cheap; a silently different execution is not.

### Data movement

EKOS is not a data mover (RFC 0154 non-goal). It selects, drives and verifies engine-native paths.

| Path | ClickHouse |
|---|---|
| **Pull from PostgreSQL** (default) | `postgresql()` table function / `PostgreSQL` engine, `INSERT … SELECT` in key-range chunks. The target engine does the reading; EKOS issues and tracks the chunks. |
| **Incremental** | Watermark-bounded chunks into `ReplacingMergeTree` (RFC 0166) |
| **Staged Parquet** | **Deferred.** See below. |

The staged Parquet path (`COPY` → Arrow → Parquet → object storage → `INSERT … FORMAT Parquet`) is
deliberately out of scope for v1. It is the point where EKOS becomes a data mover in fact if not in
name, and it pulls in `arrow`, `parquet` and object-store credentials for a benefit the pull path
already provides in most topologies. It returns when a real workload shows the pull path is
insufficient — a network boundary that forbids target→source connectivity being the likely trigger.

**Chunking.** By primary-key range where a suitable key exists, by `ctid` range otherwise. Each
chunk is a `MigrationExecution` fact with its bounds, row count, duration and status, which makes
the load resumable at chunk granularity: a re-run skips chunks already recorded complete. Chunk
bounds are half-open and recorded explicitly, because off-by-one at a chunk boundary is one of
RFC 0156's planted controls and the fact is what makes the control diagnosable.

**Throttling.** Maximum concurrent chunks, maximum source queries per second, and a replica-lag
guard checked between chunks — not only at the start. A load that pushes a replica past the policy
lag pauses rather than continuing, because the source is somebody's production database and this
tool is a guest there.

### Dry run

`ekos migrate dry-run` executes the full artifact set against the sandbox and runs V0–V2. It is a
required state transition (`generated` → `dry_run_passed`) before any staging execution may be
approved. A dry run that fails leaves the unit in `generated` with the failure attached.

## Testing

- Classifier: table-driven over every class, including the adversarial cases — a `DROP` inside a
  comment, a `DROP` inside a string literal, a `DROP` as statement forty of forty, `DELETE` without
  `WHERE`, an unparseable fragment, an empty statement. Ported from and extending
  `clickhouse-query/src/validate.rs`'s existing tests.
- Negative: no statement classified `Unknown` ever reaches an execute call, asserted by test.
- Pinning: a mutated artifact fails against its approval hash with no override.
- Environments: production credentials are unresolvable without a matching active approval.
- Resume: a load interrupted mid-way, re-run, produces the same row count and no duplicates; chunk
  facts show the skipped range.
- Throttle: a simulated replica lag spike pauses the load.

## Alternatives considered

- **Regex or prefix matching for statement classification.** Rejected: trivially fooled, and the
  existing `validate_select_only` already demonstrates the parse-based approach works.
- **Allowing unparseable statements through with a warning.** Rejected — it is the escape hatch that
  makes every other control decorative.
- **EKOS reading from PostgreSQL and writing to the target itself.** Rejected as the default: it
  makes EKOS a data mover, it is slower than engine-native paths, and it puts the row data in
  EKOS's process for no benefit. Retained only as the deferred staged path, for topologies that
  require it.
- **A `--force` flag for the hash check.** Rejected. Any such flag becomes the documented workaround
  within a month.

## Open questions

- [ ] `ctid`-range chunking is not stable under concurrent `VACUUM FULL`. Detect and refuse, or
      fall back to a full reload for tables with no usable key?
- [ ] Should dry-run be required per unit, or per wave once the unit shape is established?
- [ ] Concurrency ceiling defaults — derive from source `max_connections` and observed load, or
      require explicit configuration?

## Acceptance criteria

- [ ] Every classifier case, including the adversarial ones, is tested.
- [ ] `Unknown` and unparseable statements cannot reach execution.
- [ ] An artifact whose hash differs from its approval cannot execute, with no override path.
- [ ] A TPC-H-scale load is resumable after a forced interruption with no duplicates and no gaps.
- [ ] The replica-lag guard pauses a running load, asserted against a simulated spike.

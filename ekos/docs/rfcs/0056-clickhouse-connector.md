# RFC 0056 — ClickHouse Connector: Compiled Metadata + Live NL-to-SQL Query Engine

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-17

---

## Motivation

The user asked for direct EKOS access to ClickHouse: a natural-language question against a live
ClickHouse database should flow `question -> analyze context -> analyze metadata -> build query ->
run query -> return dataset [-> enrich with EKOS-compiled knowledge]`.

No ClickHouse code exists anywhere in this repo (confirmed by a repo-wide case-insensitive grep
before designing). This RFC is genuinely new ground, and it collides with a stated, load-bearing
EKOS invariant repeated in both `CLAUDE.md` and `README.md`: *"AI systems consume knowledge
through the Runtime only... they never touch raw enterprise systems directly."* Checked before
designing:

- Every existing MCP tool (`ekos_search`, `ekos_ekl`, `ekos_neighborhood`, `ekos_state`,
  `ekos_dependents`, `ekos_impact`, `ekos_diff`, `ekos_status`,
  `ekos_transformation_explain`/`ekos_transformation_diff`) opens the local ledger and calls
  `Runtime`/`EklInterpreter`/`ledger.diff` only — confirmed by reading `crates/cli/src/commands/
  mcp.rs`'s `call_tool` dispatch end to end. Only `ekos_identity_review` writes, and only to the
  local ledger.
- `AiRuntime::ask()` (`crates/runtime/src/ai.rs`) is retrieve -> expand -> ground -> ask, entirely
  over the ledger (`Runtime::find_objects` + `load_neighborhood` + `reconstruct_state`), never
  touching a live system.
- `crates/simulation` (RFC 0047-0055) is the one crate that already bypasses `Runtime`'s
  read-only contract, writing straight through `&dyn KnowledgeStore` — but even it only ever
  touches the *local* ledger, never a live external system.
- The Snowflake/Oracle connector scaffolds from RFC 0012 (`plugins/snowflake`, `plugins/oracle`)
  produce `ObservationArtifact`s but are never wired into `build.rs`/`recover.rs` — confirmed by
  grep, neither their `Observer` nor any consuming analyzer pass is invoked outside their own unit
  tests. No DB connector in this codebase has ever reached the ledger.

A live ClickHouse query engine is therefore a new kind of boundary this codebase has not built
before: an AI-built query hitting a live external system at request time, returning raw row data
that was never observed/compiled/evidenced. This RFC treats that explicitly as a scoped, audited
exception — not a quiet extension of "Runtime is read-only" — and splits the work into two stages
that can be validated independently.

Two decisions were made with the user before designing further:

1. The live-query capability ships as **both a CLI command and an MCP tool**, with the MCP tool
   **off by default** — `ekos mcp serve` only advertises it when a new, explicit
   `[clickhouse] enable-mcp-query = true` opt-in is set in `ekos.toml`.
2. ClickHouse schema metadata **is** compiled into the ledger as real KIR Objects, closing the
   integration gap the Snowflake/Oracle scaffolds never closed, rather than being fetched live and
   discarded on every question.

## Scope

- **Stage 1** — a real `Observer`-based connector (`plugins/clickhouse`) that scans ClickHouse's
  own `system.tables`/`system.columns` metadata over its stock HTTP interface, plus a deterministic
  recovery pass (`ClickHouseAnalyzerPass`) mapping that metadata into `KirObject(ObjectKind::Table)`
  — reusing the exact `properties["columns"]` shape `SqlAnalyzerPass` already uses for
  file-based SQL DDL, so ClickHouse tables participate in the same identity-resolution
  (`structural_score`/`similarity::column_names`, RFC 0007/0029) as every other SQL source, with no
  new exclusion-list entry required. Wired into `build.rs` (env-var gated, same soft-skip pattern
  as GitHub/Confluence) and `recover.rs` (a `collect_clickhouse_artifact_ids` collector, same shape
  as the existing git/github/confluence collectors).
- **Stage 2** — a new, auxiliary `crates/clickhouse-query` crate implementing the six-stage
  question -> dataset pipeline, exposed via a new `ekos clickhouse ask "<question>"` CLI command
  and the gated `ekos_clickhouse_query` MCP tool. SELECT-only, hard-enforced by parsing the LLM's
  generated SQL with a new `plugins/sql-dialect-clickhouse` crate (RFC 0031's `SqlDialectParser`
  pattern, wrapping `sqlparser::dialect::ClickHouseDialect` — already available in the pinned
  `sqlparser 0.53`, confirmed present at `sqlparser-0.53.0/src/dialect/clickhouse.rs`, no new
  dependency) and rejecting anything but a single `Statement::Query`.

## Non-goals

- **No write access to ClickHouse.** SELECT-only, hard-enforced at the dialect-parse gate before
  execution — matches the "Observation Layer collects facts only" spirit for the one path that
  touches a live system.
- **No cross-source joins in one query.** V1 targets a single configured ClickHouse endpoint;
  joining against other live SQL sources in the same live query is out of scope.
- **No result-set streaming/pagination.** A row-count cap (`LIMIT 1000` injected if the LLM's
  query doesn't already have one) keeps this from becoming a data-export pipe.
- **No multi-turn clarification loop.** One question -> one query -> one result, the same shape
  `ekos ask` already has.
- **No automatic row-level ledgering.** Only the fact that a query ran (SQL text, timestamp, row
  count, content-hash of the result) is appended to the ledger as Evidence/Event — the row data
  itself is not ledgered, to avoid turning live analytical output into permanent ledger bloat.
- **No LLM-based business-meaning enrichment of ClickHouse table/column names in Stage 1** (the
  `sql_analyzer.rs`-style optional second stage) — structural mapping only for v1; real, deferred.
- **No native ClickHouse driver.** ClickHouse's stock HTTP interface (`POST /?query=...FORMAT
  JSON`) is plain REST/JSON, so — unlike Oracle/SAP — there is no reason to reach for a native
  client at all; this keeps `plugins/clickhouse`'s real client on the same dependency footing as
  `plugins/snowflake`.

## Design

### Stage 1 — `plugins/clickhouse` (Observer) + `ClickHouseAnalyzerPass` (recovery)

New leaf crate `ekos-plugin-clickhouse`, same shape as `plugins/snowflake/Cargo.toml`
(`ekos-observation-sdk`, `ekos-artifact`, `async-trait`, `serde`/`serde_json`, `thiserror`,
`reqwest`, `tokio`):

```rust
pub struct ColumnMetadata { pub name: String, pub data_type: String }
pub struct TableMetadata {
    pub database: String,
    pub name: String,
    pub engine: String,
    pub order_by: Vec<String>,
    pub partition_by: Vec<String>,
    pub columns: Vec<ColumnMetadata>,
}

#[async_trait]
pub trait ClickHouseClient: Send + Sync {
    async fn list_tables(&self) -> Result<Vec<TableMetadata>, ClickHouseClientError>;
}
```

`ClickHouseHttpClient` — real `reqwest`-based implementation, `POST` (or `GET ?query=`) to
`<url>/?query=<url-encoded SQL> FORMAT JSON` with Basic Auth or `X-ClickHouse-User`/
`X-ClickHouse-Key` headers, querying `system.tables` joined with `system.columns` for the
configured database(s), following the exact `run_statement`-then-map-JSON-rows pattern
`SnowflakeApiClient::run_statement` already uses (`plugins/snowflake/src/lib.rs:67-87`). Engine
family and `sorting_key`/`partition_key` (from `system.tables`) are captured because they
materially affect query building in Stage 2 (a filter on the sort key is cheap in ClickHouse; one
on an unsorted column triggers a full scan).

`MockClickHouseClient` for unit tests, `ClickHouseObserver::scan` emitting one
`ObservationArtifact` per table: `{kind: "table", engine, order_by, partition_by, columns}`,
`connector_name = "clickhouse"`.

`crates/recovery/src/clickhouse_analyzer.rs` — `ClickHouseAnalyzerPass`, deterministic, no LLM
(matches `ConfluenceAnalyzerPass`/`GitHubAnalyzerPass`'s "pure structural, no LLM" shape). Reads
`ObservationArtifact`s with `connector_name == "clickhouse"` and maps each into:

```rust
let mut obj = KirObject::new(format!("{database}.{table}"), ObjectKind::Table);
obj.properties.insert("columns".into(), columns_json); // [{"name", "data_type"}, ...] — same
                                                         // shape SqlAnalyzerPass already uses
obj.properties.insert("engine".into(), json!(engine));
obj.properties.insert("order_by".into(), json!(order_by));
obj.properties.insert("source_system".into(), json!("clickhouse"));
```

Reusing `ObjectKind::Table` (not a new `Custom("ClickHouseTable")` kind) is the key design
decision here: `identity::structural_score` compares same-kind objects' `properties["columns"]`
via `similarity::column_names` + Jaccard overlap (`crates/identity/src/lib.rs:391-401`,
`similarity.rs:14-25`) whenever both sides carry a non-empty `columns` property — exactly what
this pass emits. A `Custom(_)` kind would instead need the blanket-exclusion treatment
`Section`/`TransformNode`/`RustSymbol`/etc. all needed (`identity/src/lib.rs:297`), which exists
specifically for objects with *no* reliable structural comparison — the opposite of what's true
here. Reusing `Table` means a ClickHouse `orders` table and a Postgres `orders` table with
overlapping columns are identity-resolution candidates the same way two file-based SQL sources
already are, with zero new exclusion-list code.

Wiring (closing the gap Snowflake/Oracle never closed):
- `crates/cli/src/commands/build.rs`: `CLICKHOUSE_URL_ENV`/`CLICKHOUSE_USER_ENV`/
  `CLICKHOUSE_PASSWORD_ENV`, same optional-both-or-neither soft-skip `match` used for GitHub/
  Confluence (`build.rs:99-133`) — absence is a normal state, not a misconfiguration.
- `crates/cli/src/commands/recover.rs`: `collect_clickhouse_artifact_ids` (same shape as
  `collect_confluence_artifact_ids`, `recover.rs:648-665`), registers `ClickHouseAnalyzerPass` when
  non-empty.

### Stage 2 — `crates/clickhouse-query` (live NL-to-SQL pipeline)

New auxiliary crate, same *posture* as `crates/simulation` (a sibling crate depending on
`ekos-runtime`/`ekos-ledger` directly, not a `CompilerPass`) but explicitly crossing a boundary
`simulation` never did: it reads a live external system at request time.

1. **Source question** — `ekos clickhouse ask "<question>"` (new CLI command,
   `crates/cli/src/commands/clickhouse_ask.rs`, mirrors `ask.rs`'s open-ledger/build-provider/
   print-result shape) and, gated on `[clickhouse].enable-mcp-query`, a new
   `ekos_clickhouse_query` MCP tool in `mcp.rs`.
2. **Analyze context** — retrieval scoped to `ObjectKind::Table` objects with
   `properties["source_system"] == "clickhouse"`, reusing `Runtime::find_objects` +
   `load_neighborhood` the same way `AiRuntime::gather_context` does
   (`crates/runtime/src/ai.rs:130-158`).
3. **Analyze metadata** — a structured schema summary (table/column/type/engine/sort-key) built
   from the *compiled* KIR, not a fresh live fetch. Fast and offline-capable, but can go stale
   until the next `ekos build`/`ekos recover` — named explicitly as a v1 tradeoff, not hidden.
4. **Build query** — one `LlmProvider::complete` call (`crates/recovery/src/llm.rs`, temperature 0,
   new `prompt_version` constant), system prompt constrained to ClickHouse SQL, SELECT-only, only
   tables/columns present in the supplied schema context.
5. **Run query** — hard-validate before execution: parse with the new
   `ekos-plugin-sql-dialect-clickhouse` crate's `ClickHouseDialect`, reject anything that isn't a
   single `sqlparser::ast::Statement::Query` (no `Insert`/`Update`/`Delete`/`AlterTable`/
   `Drop`/multi-statement batches), inject `LIMIT 1000` if absent. Execute via a sibling
   `ClickHouseQueryClient` (same HTTP shape/credentials as Stage 1's `ClickHouseHttpClient`, a
   second, read-execution-oriented method rather than a shared trait method, since `list_tables`
   and "run arbitrary validated SQL" are different enough operations to keep separate).
6. **Return dataset** — run RFC 0043's built-in redaction baseline (`ekos_common::redaction`) over
   the returned rows before they reach the LLM or the caller. This is a **new required
   integration point** — no existing redaction call site handles a live query result, only
   observed/recovered content — so the RFC treats it as a first-class step, not an afterthought.
   Optionally summarize via the LLM, citing the SQL used (mirrors `ask.rs`'s evidence-citation UX).
7. **Enrich with EKOS-loaded data** (the "if needed" step, scoped conservatively): if a returned
   column name matches a compiled KIR Object's identity key, attach that object's
   `Runtime::reconstruct_state` as sidecar context — not a full join engine.

**Auditability:** every live query (SQL text, timestamp, row count, content-hash of the result)
is appended to the ledger as an Event/Evidence pair via `&dyn KnowledgeStore` directly — the same
access level `commit.rs`/`simulation` already have.

**Config:** `EkosConfig` gains an additive `[clickhouse]` section
(`enable_mcp_query: bool`, default `false`) — omitting the section preserves CLI-only behavior,
matching the `#[serde(deny_unknown_fields)]` additive-section pattern RFC 0031's `[recover.sql]`
already established. Credentials stay in env vars, not `ekos.toml`, matching every other live
connector in this codebase (GitHub, Confluence) — no `[connections]`/host-user-password block
exists anywhere today, and this RFC doesn't introduce one.

## Alternatives Considered

- **A new `Custom("ClickHouseTable")` KIR kind, blanket-excluded from identity resolution like
  `Section`/`TransformNode`.** Rejected — those exclusions exist for objects with no reliable
  structural comparison (no `columns` property, or a same-kind 1.0 fallback that over-merges).
  ClickHouse tables have real columns and should identity-resolve against same-named tables in
  other systems; reusing `ObjectKind::Table` gets this for free.
- **Fetching ClickHouse metadata live on every question, never ledgered** (the user's other
  option). Rejected per the user's explicit choice — ClickHouse would never become searchable/
  identity-resolvable through the rest of EKOS, defeating half the point of "analyze metadata"
  being a compiler concern, not a per-question live fetch.
- **A native ClickHouse Rust driver (`clickhouse-rs`, `ch-client`) instead of the plain HTTP
  interface.** Rejected — ClickHouse's HTTP interface is simple REST/JSON with no native-library
  dependency; there is no Oracle/SAP-style reason to reach for anything heavier, and `reqwest` is
  already a workspace dependency.
- **Exposing `ekos_clickhouse_query` over MCP unconditionally, same as every other read-only MCP
  tool.** Rejected per the user's explicit choice — every existing MCP tool reads only the local
  ledger; this one hits a live external system, a materially different risk profile that warrants
  an explicit opt-in rather than being on by default the moment `mcp serve` starts.
- **Sharing one `ClickHouseClient` trait method for both schema listing and query execution.**
  Rejected — `list_tables` (Stage 1, compile-time) and "execute this validated, LLM-built SQL"
  (Stage 2, request-time) have different call shapes and different safety requirements (the latter
  needs the SELECT-only gate in front of it); keeping them as separate client methods avoids a
  trait that quietly conflates a safe, unconditional operation with one that must never run
  without prior validation.

## Testing

- Stage 1: `plugins/clickhouse` unit tests against `MockClickHouseClient` (table/column mapping,
  same shape as `plugins/snowflake`'s test module); `clickhouse_analyzer.rs` unit tests (same shape
  as `confluence_analyzer.rs`'s: one artifact -> one `KirObject(Table)`, `properties["columns"]`
  present and correctly shaped, same-database/table across two runs yields the same `KirId`).
- Stage 1 identity-resolution regression: two `ObjectKind::Table` objects (one tagged
  `source_system: clickhouse`, one plain file-based SQL DDL) with overlapping column names produce
  a non-zero `structural_score` via the existing `column_names`/`jaccard` path — proves the "reuse
  `Table`, don't blanket-exclude" decision actually works, not just compiles.
- Stage 2: unit tests for the SELECT-only validation gate (`Insert`/`Update`/`Delete`/`Drop`/
  multi-statement all rejected; a plain `SELECT` with no `LIMIT` gets one injected; a `SELECT ...
  LIMIT 10` is left alone); a redaction test proving a recognizable secret pattern in a returned
  row is stripped before the dataset is returned, mirroring `world.sources`' own
  `redaction_strips_a_recognizable_secret_before_it_reaches_the_store` test shape (RFC 0055).
- Manual/live verification (flagged to the user before running, since it needs a local ClickHouse
  container — outside this session's default sandbox): `docker run clickhouse/clickhouse-server`,
  then `ekos build && ekos recover` confirming tables appear via `ekos_search`/`ekos ekl`; `ekos
  clickhouse ask "<question>"` end to end; confirm `ekos mcp serve` does **not** list
  `ekos_clickhouse_query` with the opt-in flag unset, and does with it set. This project's last two
  RFCs (0054, 0055) both found real bugs invisible to `cargo test --workspace` alone — live CLI
  verification is treated as load-bearing here too, not optional.
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.

## Acceptance Criteria

- [x] `ekos-plugin-clickhouse` crate exists with `ClickHouseObserver`/`ClickHouseHttpClient`/
      `MockClickHouseClient`, unit-tested (6 tests).
- [x] `ClickHouseAnalyzerPass` maps ClickHouse table artifacts into `ObjectKind::Table` KIR objects
      with `properties["columns"]` in the same shape `SqlAnalyzerPass` uses (4 tests).
- [x] `build.rs`/`recover.rs` wire the connector in end to end, env-var gated, soft-skip on
      absence.
- [x] No identity-resolution exclusion-list change required or made — verified by the
      cross-system `structural_score` regression test above; `crates/identity/src/lib.rs` was
      not touched by this RFC.
- [x] `ekos-plugin-sql-dialect-clickhouse` exists, wraps `sqlparser::dialect::ClickHouseDialect`
      (4 tests), registered in `sql_dialect_registry.rs`.
- [x] `crates/clickhouse-query` implements the six-stage pipeline; SELECT-only is hard-enforced
      before any live execution (16 tests across `schema`/`validate`/`client`/`audit`/`lib`).
- [x] Live query results are redacted (RFC 0043) before being returned or logged — verified by
      `full_pipeline_returns_redacted_audited_dataset` and `record_query_event`'s own tests.
- [x] Every live query is recorded as an Event/Evidence pair in the ledger; row data itself is not
      ledgered — verified by `records_evidence_and_event_but_not_row_data`.
- [x] `ekos_clickhouse_query` MCP tool is absent from `mcp serve`'s tool list unless
      `[clickhouse].enable-mcp-query = true` is set — verified by
      `clickhouse_query_tool_absent_without_opt_in`/`_present_with_opt_in`, plus a
      defense-in-depth rejection test for calling the tool by name with the gate off.
- [x] Full workspace `cargo build/test/clippy/fmt` clean, including the separate `benchmark/` and
      `tests/integration/` workspaces.
- [ ] **`ekos clickhouse ask` verified live against a real ClickHouse instance.** Not done this
      session — `ClickHouseHttpClient`/`ClickHouseHttpQueryClient` are written to ClickHouse's
      documented HTTP interface and `system.tables`/`system.columns` schema (same "real client,
      never run against a live server" posture RFC 0012 used for Snowflake/Fabric/SAP) but a
      local ClickHouse container needs an explicit decision to launch (new environment
      dependency, outside this session's default sandbox) — named explicitly rather than silently
      claimed, per this project's established "don't claim more than is true" convention.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0056-clickhouse-connector.md` | This RFC |
| `ekos/plugins/clickhouse/` (new crate `ekos-plugin-clickhouse`) | `ClickHouseObserver`, `ClickHouseClient`/`ClickHouseHttpClient`/`MockClickHouseClient` |
| `ekos/plugins/sql-dialect-clickhouse/` (new crate) | `ClickHouseDialectParser` |
| `ekos/crates/recovery/src/clickhouse_analyzer.rs` (new) | `ClickHouseAnalyzerPass` |
| `ekos/crates/recovery/src/sql_dialect_registry.rs` | Registers the `"clickhouse"` dialect |
| `ekos/crates/recovery/src/lib.rs`, `Cargo.toml` | Module wiring, new dialect-crate dependency |
| `ekos/crates/cli/src/commands/build.rs` | `ClickHouseObserver` construction, env-var gated |
| `ekos/crates/cli/src/commands/recover.rs` | `collect_clickhouse_artifact_ids`, `ClickHouseAnalyzerPass` registration |
| `ekos/crates/cli/src/commands/clickhouse.rs` (new) | `ekos clickhouse ask` CLI command, shared `run_query` helper |
| `ekos/crates/cli/src/commands/mcp.rs` | Gated `ekos_clickhouse_query` tool definition + dispatch, sync-to-async bridge |
| `ekos/crates/cli/src/bin/ekos.rs` | `ClickHouse { subcommand }` / `ClickHouseCommands::Ask` |
| `ekos/crates/compiler-core/src/config.rs` | `ClickHouseConfig { enable_mcp_query }`, `[clickhouse]` in `ekos.toml` |
| `ekos/crates/clickhouse-query/` (new crate `ekos-clickhouse-query`) | `ask_clickhouse` pipeline; `schema.rs`, `validate.rs`, `client.rs`, `audit.rs` |
| `ekos/Cargo.toml` | New workspace members + dependency entries |

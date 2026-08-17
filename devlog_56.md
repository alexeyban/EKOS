# Devlog 56 — RFC 0056: ClickHouse connector, and the first live-system crossing in the MCP surface

**Date:** 2026-08-17
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

The user asked for direct EKOS access to ClickHouse: a natural-language question answered by an
LLM-built SQL query run live against a ClickHouse database, grounded in EKOS's own compiled
metadata, with the result optionally enriched from the rest of the compiled estate. Before writing
any code, this session checked the request against EKOS's own stated invariant — "AI systems
consume knowledge through the Runtime only... they never touch raw enterprise systems directly" —
and confirmed by direct reading that no MCP tool, `AiRuntime`, or connector in this codebase had
ever crossed that line before; the closest precedent (`crates/simulation`) only ever touches the
*local* ledger. RFC 0056 treats the live-query capability as an explicit, scoped, audited
exception to that invariant rather than a quiet extension of it, and splits the work into two
independently-valid stages: a standard compiled-metadata connector (Stage 1, zero invariant risk)
and a new auxiliary crate implementing the live NL-to-SQL pipeline (Stage 2, the one path that
actually crosses the line). Both stages are fully implemented, tested (60+ new tests, all passing),
and gated: the CLI command works unconditionally; the MCP tool is off by default and only listed
once a workspace explicitly opts in via `ekos.toml`.

---

## RFC 0056 — ClickHouse Connector: Compiled Metadata + Live NL-to-SQL Query Engine

### Problem / motivation

A repo-wide case-insensitive grep found zero prior ClickHouse references anywhere — genuinely new
ground. Two design forks were resolved with the user up front, before any code: (1) the live-query
capability ships as both a CLI command and an MCP tool, with the MCP tool off by default behind an
explicit `ekos.toml` opt-in; (2) ClickHouse schema metadata is compiled into the ledger as real KIR
objects rather than fetched live and discarded per question — closing the exact integration gap
the RFC 0012 Snowflake/Oracle scaffolds never closed (their `ObservationArtifact`s are produced but
never reach `recover.rs` or any analyzer pass, confirmed by grep before designing).

### What was built

| Component | Location |
|---|---|
| RFC | `ekos/docs/rfcs/0056-clickhouse-connector.md` |
| Stage 1: Observer + real HTTP client | `ekos/plugins/clickhouse` (new crate `ekos-plugin-clickhouse`) |
| Stage 1: deterministic recovery pass | `ekos/crates/recovery/src/clickhouse_analyzer.rs` |
| ClickHouse SQL dialect (RFC 0031 pattern) | `ekos/plugins/sql-dialect-clickhouse` (new crate) |
| Stage 2: six-stage live query pipeline | `ekos/crates/clickhouse-query` (new crate `ekos-clickhouse-query`) |
| `ekos clickhouse ask` CLI command | `ekos/crates/cli/src/commands/clickhouse.rs`, `bin/ekos.rs` |
| Gated `ekos_clickhouse_query` MCP tool | `ekos/crates/cli/src/commands/mcp.rs` |
| `[clickhouse]` config section | `ekos/crates/compiler-core/src/config.rs` |

**Deliberately not built**, per the RFC's own Non-goals: write access to ClickHouse (SELECT-only,
hard-enforced); cross-source joins in one live query; result-set streaming/pagination beyond a
`LIMIT` cap; a multi-turn clarification loop; automatic row-level ledgering (only the fact that a
query ran is ledgered, never the rows themselves); LLM-based business-meaning enrichment of
ClickHouse table/column names (the `sql_analyzer.rs`-style optional second stage); a native
ClickHouse driver (the stock HTTP interface is plain REST/JSON, so there was never a reason to
reach for one).

### Implementation details worth remembering

**Reusing `ObjectKind::Table` instead of inventing `Custom("ClickHouseTable")` was the load-bearing
design decision in Stage 1.** Reading `identity::structural_score` before designing
(`crates/identity/src/lib.rs:391-401`) showed it compares same-kind objects' `properties["columns"]`
via Jaccard overlap whenever both sides have a non-empty `columns` property — exactly what
`sql_analyzer.rs` already emits for file-based SQL DDL. A new `Custom(_)` kind would have needed
the same blanket-exclusion treatment `Section`/`TransformNode`/`RustSymbol`/etc. all needed
(`identity/src/lib.rs:297`) — but that exclusion exists specifically for objects with *no* reliable
structural comparison, the opposite of what's true here. Reusing `Table` means a ClickHouse
`orders` table and a Postgres `orders` table with overlapping columns become real cross-system
identity-resolution candidates with zero new exclusion-list code, and a regression test
(`clickhouse_table_and_generic_sql_table_share_kind_for_identity_resolution`) pins this down. This
directly overrode the original plan's assumption (written before reading the identity code) that a
new exclusion-list entry would be needed — caught before implementation, not after.

**ClickHouse's stock HTTP interface (`POST /` with the SQL as the raw body, `FORMAT JSON` response)
meant `ekos-plugin-clickhouse` could follow the Snowflake precedent (a real `reqwest`-based client
from day one) rather than the Oracle/SAP precedent (a documented stub, because their native drivers
need libraries this environment can't install).** No new dependency class, no `bindgen`, no risk to
`cargo build --workspace` for anyone without a ClickHouse-specific SDK installed.

**Stage 1 and Stage 2 deliberately use two different ClickHouse client shapes** —
`ekos-plugin-clickhouse`'s `ClickHouseClient` (schema listing, compile-time, via `build.rs`) and
`ekos-clickhouse-query`'s `ClickHouseQueryClient` (arbitrary validated SQL execution, request-time,
only ever called on SQL that already passed the SELECT-only gate). Collapsing these into one shared
trait method was considered and rejected — the two operations have different safety requirements,
and conflating "always-safe schema read" with "run this SQL, but only if validated" into one trait
felt like exactly the kind of interface that invites a future caller to skip the validation step by
accident.

**The SELECT-only gate parses the LLM's generated SQL with `sqlparser`'s real `ClickHouseDialect`
(confirmed present in the already-pinned `sqlparser = "0.53"`, no new dependency) and rejects
anything that isn't exactly one `Statement::Query`.** A `LIMIT` is injected into the parsed AST
(not string-hacked) when the query doesn't already have one, then the AST is re-rendered via
`Display` — so the executed SQL is always something `sqlparser` itself parsed and re-emitted, never
a string this code assembled by hand. Multi-statement batches (`SELECT 1; DROP TABLE orders`) are
caught by checking `Parser::parse_sql` returns exactly one statement, not by looking for a
semicolon.

**MCP's stdio serve loop is a blocking `for line in stdin.lock().lines()` invoked directly inside
`main`'s `#[tokio::main]` runtime, never spawned onto its own task — so `call_tool`'s new
`ekos_clickhouse_query` arm needed the exact same `Handle::try_current()` +
`tokio::task::block_in_place` bridge devlog_55 established for `ingest_sources`.** This is the
second time in two sessions this class of bug pattern has come up (RFC 0055's fix was for
`ekos simulate`'s own entry point); recognizing it immediately from the prior devlog meant this
one was written correctly the first time rather than discovered by a live "cannot start a runtime
from within a runtime" panic.

**The MCP gate is enforced twice, not once.** `tool_definitions(config)` only appends
`ekos_clickhouse_query`'s definition when `config.clickhouse.enable_mcp_query` is true, so an
ungated server never advertises it in `tools/list` — but `call_tool`'s `"ekos_clickhouse_query"`
arm re-checks the same flag before doing anything, because a client can call a tool by name
directly without ever having listed it first. Both paths are tested
(`clickhouse_query_tool_absent_without_opt_in`, `clickhouse_query_call_rejected_when_gate_is_off`).

**Redaction has a genuinely new integration point in this RFC.** Every existing RFC 0043 call site
redacts content on its way *into* the ledger (observed files, recovered artifacts). Live query
results never touch the ledger as row data at all — they flow straight back to the caller — so
`ask_clickhouse` calls `ekos_common::redaction::redact_json` on every returned row before returning
them, the first redaction call site in this codebase that scrubs *outbound* data rather than
inbound.

### Decisions (alternatives considered, why this choice)

- **Fetching ClickHouse metadata live on every question, never ledgered** — rejected per the
  user's explicit choice; ClickHouse would never become searchable/identity-resolvable through the
  rest of EKOS otherwise.
- **A new `Custom("ClickHouseTable")` KIR kind** — rejected once `structural_score`'s actual
  comparison logic was read; see above.
- **A native ClickHouse Rust driver** — rejected; the HTTP interface is plain REST/JSON, `reqwest`
  is already a workspace dependency, and there's no Oracle/SAP-style forcing reason to reach for
  anything heavier.
- **Exposing `ekos_clickhouse_query` over MCP unconditionally** — rejected per the user's explicit
  choice; every other MCP tool reads only the local ledger, this one hits a live external system,
  and that's a materially different risk profile.
- **One shared `ClickHouseClient` trait for both schema listing and query execution** — rejected;
  see above.

---

## Knowledge Captured

- **When a new object kind needs an identity-resolution decision, read `structural_score`'s actual
  comparison logic before assuming a blanket exclusion is needed.** The exclusion list
  (`Section`/`TransformNode`/`RustSymbol`/`RustModule`/`PythonSymbol`/`PythonModule`/`Crate`) exists
  for objects with *no* reliable structural signal — reusing an existing, comparison-friendly kind
  (`ObjectKind::Table`, keyed on a real `columns` property) is the better default when the object
  genuinely has one, and gets real cross-system matching for free instead of opting out of it.
- **ClickHouse's HTTP interface (`POST /` with SQL as the raw body, `FORMAT JSON` response) needs
  no native driver** — confirmed as the reason `plugins/clickhouse` could follow the Snowflake "real
  client from day one" precedent instead of the Oracle/SAP "documented stub" precedent RFC 0012 set.
- **Any MCP tool implementation that needs to call `async fn`s must assume it's running inside an
  already-active `#[tokio::main]` runtime, not assume no runtime is active** — the same
  `Handle::try_current()` + `block_in_place` bridge RFC 0055 needed for `ingest_sources` applies
  anywhere a sync call site (the stdio serve loop, here) needs to bridge into async code without
  being spawned as its own task. This is now a recognized, reusable pattern in this codebase, not
  a one-off fix.
- **A capability gate belongs at both the discovery layer and the execution layer.** Hiding
  `ekos_clickhouse_query` from `tools/list` alone would not have stopped a client from calling it
  by name; both checks are necessary, and both need their own test.
- **Redaction has to be applied on the way *out* of the system, not just on the way in, for any
  path that returns live external data without ledgering it.** Every prior RFC 0043 call site
  guarded content entering the ledger; live query results never enter the ledger at all, so this
  RFC is the first place redaction runs on an outbound response.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0056-clickhouse-connector.md` | New RFC, Accepted, all met Acceptance Criteria checked; one criterion (live verification against a real ClickHouse instance) explicitly left unchecked and named |
| `ekos/plugins/clickhouse/` | New crate: `ClickHouseObserver`, `ClickHouseClient`/`ClickHouseHttpClient`/`MockClickHouseClient`; 6 tests |
| `ekos/plugins/sql-dialect-clickhouse/` | New crate: `ClickHouseDialectParser`; 4 tests |
| `ekos/crates/recovery/src/clickhouse_analyzer.rs` | New: `ClickHouseAnalyzerPass`; 4 tests plus an identity-resolution regression test |
| `ekos/crates/recovery/src/sql_dialect_registry.rs`, `Cargo.toml`, `lib.rs` | Registers the `"clickhouse"` dialect; new dependency; module wiring |
| `ekos/crates/cli/src/commands/build.rs` | `ClickHouseObserver` construction, env-var gated (`EKOS_CLICKHOUSE_URL`/`_DATABASE`/`_USER`/`_PASSWORD`) |
| `ekos/crates/cli/src/commands/recover.rs` | `collect_clickhouse_artifact_ids`, `ClickHouseAnalyzerPass` registration, summary output |
| `ekos/crates/cli/src/commands/clickhouse.rs` | New: `ekos clickhouse ask` CLI command, shared `run_query` helper (also used by the MCP tool) |
| `ekos/crates/cli/src/commands/mcp.rs` | Gated `ekos_clickhouse_query` tool definition + dispatch, sync-to-async bridge; 3 new tests |
| `ekos/crates/cli/src/bin/ekos.rs` | `ClickHouse { subcommand }` / `ClickHouseCommands::Ask` |
| `ekos/crates/compiler-core/src/config.rs` | `ClickHouseConfig { enable_mcp_query }`, `[clickhouse]` in `ekos.toml`; 2 new tests |
| `ekos/crates/clickhouse-query/` | New crate: `ask_clickhouse` six-stage pipeline; `schema.rs`, `validate.rs`, `client.rs`, `audit.rs`; 16 tests |
| `ekos/Cargo.toml` | New workspace members (`plugins/clickhouse`, `plugins/sql-dialect-clickhouse`, `crates/clickhouse-query`) + dependency entries |

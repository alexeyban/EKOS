# RFC 0250 — `ekos_sql_generate`: EKOS's SQL agent over MCP

**Status:** Accepted and implemented (2026-11-08, on the maintainer's instruction; caps 50/day global, 5/day per person)
**Date:** 2026-11-08
**Depends on:** RFC 0242/0243 (`ekos sql generate`, the agent and its probes), RFC 0235 (`[[databases]]`
`kind = "postgres"`), RFC 0182 (console MCP gateway), RFC 0249 (public-demo limits), RFC 0245 (Ossie)
**Consumer:** the EKOS Superset fork (`alexeyban/ekos-superset`, Ask (AI)) in the Azure public demo

---

## 1. Motivation

The public demo must answer a business question with SQL **written by EKOS**, as measured on BIRD
(RFC 0242: agent K2 76.2 on hold, +22.9 over raw DDL), not by a BI tool's own prompt. Today the agent is
CLI-only (`ekos sql generate --database <name>`). Superset's Ask (AI) currently drafts SQL with its own
model and has EKOS check it (`ekos_sql_check`); it should instead ask EKOS for the query and only chart it.

## 2. Design

### 2.1 The tool

`ekos_sql_generate { question, database?, dialect? }` (turns come from `[sql-generate] max-turns`, default 6) runs exactly `sql_check::generate`'s
job (same prompt `ekos-sql-generate-v1`, same tools, same `verify`), and returns its JSON:
`status` (`ok`, `open_findings`, `no_sql`, `limit`), `sql`, `dialect`, `findings`, `open_findings`,
`checked` (the live read-only probe label, or why the database was not queried), `turns`, `tools`,
token counts and today's `usage`. No `sql` is a normal result, not a tool error. The agent keeps no
prose explanation and the schema carries no ledger ids, so neither is returned: a client that wants
ids reads them from `ekos_ossie_export` (Superset does). With exactly one `[[databases]]` entry, its
live probes are used by default.

### 2.2 Opt-in, metered, capped

- Listed only with `[sql-generate] mcp = true` in `ekos.toml` (off by default). It is the first MCP
  tool that calls a model.
- A non-Ollama provider additionally needs `[sql-generate] allow-metered = true` (the CLI's
  `--allow-metered`, `migrate_logic::model_call_allowed`).
- A global daily cap inside EKOS: `[sql-generate] daily-calls` (default 50) and `daily-tokens` (default
  200 000), counted per UTC day in `<state>/sqlgen/usage.json` (not the ledger; a run-state file like
  `runs/`). The call is counted **before** the model is called, so a failing call cannot be retried
  past the cap. Over the cap → `status: "limit"`, and no provider is even built.
- Per caller, in the console gateway: `mcp_policy.METERED_TOOLS` (only this tool) needs the tenant's
  `allow_metered`, and spends `mcp_metered_per_person_per_day` (default 5) for a signed-in person or
  `mcp_metered_per_token_per_day` (default 50) for an API token, which stands for a service such as
  Superset (table `McpMeteredUsage`). The console chat never offers a metered tool.
- Per Superset user: the fork spends one question per Ask (SQL or dashboard) from
  `EKOS_AI_PER_USER_PER_DAY` (default 5) and `EKOS_AI_PER_DAY` (default 50, the whole instance,
  covering its own model calls for dashboards too), after EKOS has answered and before any model
  call.
- Every model call already emits `ekos_model_call` (`MeteredLlmProvider`).

### 2.3 Read-only and scope

The agent reads the ledger read-only and the database through the existing probe session (PostgreSQL:
read-only transaction **and** a role write check; `--allow-writable-role` is never offered over MCP).
Nothing is written to the ledger: the tool is not in `only_listed_functions_can_write_the_ledger`.
It is in `SCOPE_REFUSED_TOOLS` ("builds the whole compiled schema; a restricted reader would see tables
outside its scope"); the console's `mcp_policy` maps it to `read` plus the tenant's `allow_metered`.

### 2.4 Threads

The MCP worker is synchronous; the job runs on its own scoped thread with a current-thread runtime, as
the CLI does (the PostgreSQL driver panics inside an async runtime — `ProbeWorker`).

## 3. Superset (consumer)

Ask (AI) calls `ekos_sql_generate` when listed (`tools/list`), runs the returned SQL through its own
read-only `guard` (EKOS's tables only) and shows the `ekos` block (author, tables, evidence, open
findings, probe label, questions left). Without the tool, the draft-and-check path stays.
Dashboards are not routed through it yet: a dashboard of six charts would spend six generations, so
charts keep the draft-and-check path (Superset's model, every SQL checked by `ekos_sql_check`).

## 4. Tests

Rust (`commands/mcp.rs`): listed only with the flag; refused when off and, for a metered provider,
without `allow-metered`; a mock provider's SQL comes back and the second call over a cap of 1 is
`limit` with no provider built; no question or no tables refuse before anything is counted; the
read-scope decision test covers the tool. Console (`test_mcp_gateway.py`): the tenant's
`allow_metered` is needed; per-person and per-token caps; the chat never offers it. Superset
(`tests/unit_tests/ekos_ai`): EKOS's SQL is used when the tool is listed and still passes the guard;
`limit`/`no_sql` are plain answers; the quota is spent only after EKOS answered; per-user and
instance quotas on SQLite.

## 5. Open questions

1. Should the console be able to raise the cap per tenant (a console setting passed as env)?
2. Cache identical questions (same ledger digest + question) to spare the model — likely yes for the
   demo question set.

## Amendment 1 (2026-11-09): `tables`, the caller's allow-list

**Problem.** On the Azure demo, Superset lets questions use 25 iDempiere tables, but the agent saw
all 1,233 compiled ones. For "what does Joe Block buy the most?" it wrote SQL over `ad_user` (a
login, not the customer `c_bpartner`), and Superset's read-only guard refused it: the person got
"refused" for a question the allowed tables answer.

**Change.** `ekos_sql_generate` takes an optional `tables` (1–500 names, bare or
schema-qualified, any case). `generate_core` narrows the schema with `Schema::retain_tables`
(`sqlgen`, pure) before the prompt is built: the named tables, and only the foreign keys, declared
joins, indexed values and metrics among them. The agent therefore never sees — and its lint flags as
unknown — any other table. Names that match no compiled table come back in `tables_unknown`; a list
that matches none is refused before the cap is counted or a provider is built.

**Callers.** Superset's fork passes its `EKOS_AI_TABLES` when the tool's input schema lists
`tables`, and, against an older EKOS, falls back to its own draft-and-check path over the allowed
tables when the guard refuses EKOS's SQL.

**Tests.** `sqlgen::schema::retain_tables_keeps_the_named_tables_and_only_their_joins`;
`commands/mcp.rs::sql_generate_sees_only_the_tables_the_caller_allows` (a recording provider: the
allowed table is in the prompt, the other table and its columns are not; `tables_unknown`; no model
call for an allow-list that names nothing compiled).

## Amendment 2 (2026-11-09): never hand back SQL that writes

**Problem.** Recording the demo, "Delete all invoices of Joe Block" came back from
`ekos_sql_generate` as `DELETE FROM c_invoice WHERE c_bpartner_id IN (…)` with status
`open_findings` (lint `E001`). Superset's guard refused it and nothing ran, but a tool that
promises one read-only SELECT must not rely on every client having a guard.

**Change.** After the agent, `generate_core` checks the final SQL (`read_only_refusal`): parsed in
the dialect, it must pass `guard::single_select` (one query, no `SELECT … INTO`, no locking clause,
no data-modifying part); when the parser cannot read it, it counts as a write if it starts with a
writing keyword or names a strong writing verb (INSERT, UPDATE, DELETE, MERGE, DROP, TRUNCATE,
ALTER, GRANT, REVOKE, CREATE) outside string literals. A write is answered `status: "refused"`,
`sql: null` and a `reason`; the SQL itself is never returned. The call still counts toward the
daily cap (the model was asked).

**Callers.** Superset's fork answers EKOS's refusal plainly and no longer retries a refusal by its
own guard unless the reason is a table outside the allowed ones.

**Tests.** `sql_check::tests::read_only_refusal_names_writes_and_passes_queries` (DELETE, UPDATE, a
data-modifying CTE, two statements, unparsed writes, a writing word inside a literal);
`mcp.rs::sql_generate_never_hands_back_sql_that_writes` (a model that answers with a DELETE).


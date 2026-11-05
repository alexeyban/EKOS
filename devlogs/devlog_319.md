# Devlog 319 — Text-to-SQL agent with checks, and per-dialect SQL checking and generation (RFC 0242, RFC 0243)

**Date:** 2026-11-05
**PRs:** — (worked on main, local gates, pushed)
**Branch:** main

---

## Summary

RFC 0242 asked how to push BIRD Mini-Dev past RFC 0241's G2 (66.9% on the hold-out split). The
proposal was more repair rounds, ledger lookups between them, and a SQLite-specific checker.

The existing reports answered first. The judge saw nothing wrong in 92 of G2's 100 hold-out
failures, so more rounds could reach at most about 8 questions. RFC 0242 therefore built
gold-free checks (lint + probes) and a tool-using agent in a new crate, `ekos-sqlgen`.

- **The checks moved nothing on their own:** cell J, the detector loop, equalled its baseline.
- **The agent did.** On tune, K2 (agent + verify) scored 75.8% against 68.7%: +7.1, p = 0.007.
  On the hold-out it scored **76.2% against 71.5%: +4.6, CI [0.0, +9.3], p = 0.06**, the same
  direction but not shown at 5%.
- **Against raw DDL with the same model:** 76.2% against 53.3%, **+22.9 points**.

RFC 0243, at the user's request, made every SQL plugin able to **check and help generate** SQL
for its own engine, not only read it:

- every dialect states its facts and generation notes;
- the PostgreSQL, ClickHouse and Oracle plugins run read-only probes behind a single-SELECT guard;
- new commands `ekos sql check` and `ekos sql generate`, and the MCP tool `ekos_sql_check`;
- a `text-to-sql` skill for Claude Code.

---

## RFC 0242 — gold-free checks and a tool-using agent

### Problem / motivation
G2's hold-out failures:

| Category | Count |
|---|---|
| Executed, wrong values | 79 |
| Wrong number of rows | 20 |
| Wrong number of columns | 1 |
| No SQL or execution error | 0 |

The repair loop could see none of these. The errors are about meaning, not syntax:

- a value spelled differently in the data (`Germany` vs `Berlin, Germany`);
- sentinel zeros averaged in;
- joins that fan out;
- integer division;
- ties.

### What was built
| Component | Where |
|---|---|
| SQLite dialect plugin | `plugins/sql-dialect-sqlite` |
| Schema model (ledger, profiles, value index) | `crates/sqlgen/src/schema.rs` |
| Scope-resolving SQL analysis | `crates/sqlgen/src/analyze.rs` |
| Lint `L001`–`L008` | `crates/sqlgen/src/lint.rs` |
| Probes `E000`, `P001`–`P005` | `crates/sqlgen/src/probe.rs` |
| Agent: text tool protocol, checks on submit, `verify` | `crates/sqlgen/src/agent.rs` |
| Profile v3: sentinels (`0` / `-1` for unknown) | `evals/src/bench/context.rs` |
| Cells `strategy = loop / agent`, `max-turns`, `checks`, `verify` | `evals/src/bench/{mod,bird}.rs` |
| Free detector replay | `ekos eval bench checks <report>` |
| Cells G3, G3-full, J, K, K2 | `evals/bench/bird-mini-dev/configs/zen-haiku/` |

### Results
| Cell | Tune (198) | Hold-out (302) | Cost per run |
|---|---|---|---|
| G2 (old pack) | 67.7 | 66.9 | $0.13 / $0.19 |
| G3 (pack with descriptions) | 68.7 | 71.5 (G3-full) | — / $0.35 |
| J (detector loop) | 68.2 | — | $0.23 |
| K (agent) | 74.2 | — | $0.25 |
| K2 (agent + verify) | **75.8** | **76.2** | $0.51 / $0.78 |

RFC 0242 spend: **$2.13** of the approved $3.

### Decisions
- **The hold-out was used for the final pair.** BIRD dev overlaps Mini-Dev by construction, so it
  is no fresh set. The report states that the hold-out was inspected on 2026-11-05.
- **L002 is out of the cells.** Its replay precision was 1 wrong answer for every 2 right ones.
- **New fields serialise only when set.** `strategy`, `max-turns`, `checks` and `verify` are
  skipped at their defaults, so existing cells keep their hashes. A fixture test checks this.

---

## RFC 0243 — every SQL plugin checks and generates for its engine

### What was built
| Component | Where |
|---|---|
| `DialectFacts` gains `row_limit`, `identifier_quote`, `double_quote_is_string`, `backtick_identifiers`; `generation_notes()` | `sql-dialect-sdk` |
| `Probe`, `Preview` and the single-SELECT `guard`, moved here so plugins don't depend on `recovery` | `sql-dialect-sdk/src/{probe,guard}.rs` |
| Facts and notes for all 10 dialects | `plugins/sql-dialect-*`, `GenericDialectParser` |
| Lint `L009` (row-limit form), `L010` (`ILIKE`), `L011` (quoting), `E001` (parse) | `sqlgen/src/lint.rs` |
| `PgProbe`: read-only session plus the role's write check | `plugins/postgres/src/probe.rs` + `PgSource::preview` |
| `ChProbe`: `readonly=1`, `JSONCompact` | `plugins/clickhouse/src/probe.rs` |
| `OraProbe`: `SET TRANSACTION READ ONLY` (feature `oracle`) | `plugins/oracle/src/probe.rs` + `OraSource::preview` |
| `ekos sql check [--database/--sqlite]`, `ekos sql generate` | `cli/src/commands/sql_check.rs` |
| MCP `ekos_sql_check`: static only; `sql` optional, which returns the dialect notes | `cli/src/commands/mcp.rs`, console `mcp_policy.SCOPED_TOOLS` |
| Skill | `.claude/skills/text-to-sql/SKILL.md` |

### Live verification
On EKOS's testbed, with schema `sqlgen` and read-only role `sqlgen_ro`, and ClickHouse database
`sqlgen`:

- **Value spelling:** P003 found `'Germany'` in `'Berlin, Germany'`.
- **Integer division:** L005 caught `COUNT(*)*100/(SELECT COUNT(*)…)`.
- **Ties:** P005 flagged a tie on ClickHouse.
- **Writable role:** a role that can write was refused.
- **Writes:** `SELECT 1; DELETE …` was refused by the guard, and the row count was unchanged.
- **Generation:** `sql generate` ran end to end with a local 3B model on both engines.

---

## Knowledge Captured

- **Most wrong text-to-SQL answers run and have the right shape.** Gold-free detectors (unknown
  names, value spellings, fan-out, ties, sentinels) fired on about 10 of 198 tune answers. What
  helped was letting the model look at the data (`find_values`, `run_sql`) and at its own result
  once (`verify`).
- **Appending a protocol to a prompt changes single-turn answers too.** In K, 14 of 19 flips came
  from questions that never called a tool. Attribute an agent's gain per question (tools used or
  not) before crediting the tools.
- **The BIRD ledgers changed under the cells.** After the SqlAnalyzerPass v6 fix the pack carries
  column descriptions. That alone was +4.6 on the hold-out (+1.0 on tune). Check
  `ledger_fingerprints` in reports before comparing a new cell with an old one.
- **A cell's `budget-usd` stops a run early, and the report says so in its `n =` line.**
  - G3's $0.30 cap ended its hold-out run at 242 of 302.
  - A copy with a new name re-runs from the cache for pennies.
  - Changing the cap itself would change the config hash, and the run would be marked as
    changed after a hold-out run.
- **The `postgres` crate panics inside a tokio runtime** ("Cannot start a runtime from within a
  runtime"). A session that an async agent queries must live on its own plain thread, behind a
  channel (`ProbeWorker`).
- **A PostgreSQL simple query accepts several statements**, and a role can turn
  `default_transaction_read_only` off itself. Live probes therefore need all three:
  - a parse guard: one `Statement::Query`, no `INTO`, no locking clause, no data-modifying CTE;
  - the read-only session;
  - a role-level write check.
- **Probe SQL must be portable:**
  - `SUM(col = 'x')` works only on SQLite and MySQL;
  - an inner `LIMIT` breaks Oracle and SQL Server;
  - a live table must be named as the query names it (`sqlgen.customers`).
- **Gold SQL near the 30-second limit can time out at scoring** under load, which shows as
  `unscored`. Pair-compare with that in mind (q40, q229).
- **The build cache filled `/home`.** `ekos/target` had grown to 628 GB. A daily user timer now
  runs `cargo sweep --time 3` over `~/PycharmProjects`
  (`~/.config/systemd/user/cargo-sweep.{service,timer}`).

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0242-text-to-sql-agent.md`, `0243-dialect-sql-check-and-generation.md` | New RFCs with results |
| `ekos/crates/sqlgen/` | New crate |
| `ekos/plugins/sql-dialect-sqlite/` | New dialect plugin |
| `ekos/crates/sql-dialect-sdk/src/{lib,probe,guard}.rs` | Facts, notes, probe trait, guard |
| `ekos/plugins/sql-dialect-*/src/lib.rs`, `crates/recovery/src/sql_dialect_registry.rs` | Facts and notes for every dialect; sqlite registered; registry test |
| `ekos/plugins/{postgres,clickhouse,oracle}/src/probe.rs`, `crates/{pg-live,ora-live}/src/session.rs` | Live read-only probes |
| `ekos/crates/evals/src/bench/*`, `crates/evals/tests/bench_fixture.rs` | Strategies, verify, sentinels, finding counts, replay helper, J/K fixture cells |
| `ekos/crates/cli/src/commands/{sql_check,eval_bench,mcp,catalogue}.rs`, `app.rs` | `sql check/generate`, `eval bench checks`, MCP tool, catalogue rows |
| `web/api/app/mcp_policy.py` | `ekos_sql_check` scoped |
| `evals/bench/bird-mini-dev/configs/zen-haiku/{g3,g3-full,j,k,k2}.toml` | New cells |
| `.claude/skills/text-to-sql/SKILL.md` | New skill |
| `CLAUDE.md`, `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Docs |

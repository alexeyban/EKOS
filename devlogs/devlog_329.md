# Devlog 329 — Superset answers from EKOS; `ekos_sql_generate` over MCP (RFC 0250)

**Date:** 2026-11-08
**PRs:** none (direct commits: EKOS main, `alexeyban/ekos-superset` `ekos-4.1.1`, `alexeyban/ekos-demo-public` main)
**Branch:** main

---

## Summary
The Superset fork's Ask (AI) now answers only from EKOS over MCP: its schema, meanings and joins come
from `ekos_ossie_export`, every drafted SQL is checked by `ekos_sql_check`, and nothing is answered
when EKOS cannot be reached. RFC 0250 then puts EKOS's own text-to-SQL agent (the BIRD one) behind a
new opt-in MCP tool, `ekos_sql_generate` — the first MCP tool that calls a model — with a daily cap in
EKOS, a per-caller allowance in the console gateway, and per-user limits in Superset. Caps, as the
maintainer set them: 50 a day for the project, 5 a day per person.

---

## Superset fork — Ask (AI) grounded on EKOS (ekos-superset 0647aa1, image 4.1.1-ekos.2)

### Problem / motivation
Ask (AI) read `information_schema` and let a model write SQL over bare names. The requirement: Superset
must query EKOS via MCP and answer from EKOS.

### What was built
| Piece | File |
|---|---|
| MCP Streamable HTTP client (JSON or SSE, bearer token, UTF-8 forced) | `superset/ekos_ai/ekos_mcp.py` |
| Catalog from `ekos_ossie_export`, ranking with `ekos_search` + synonyms + FK neighbours, prompt schema, grounding block | `superset/ekos_ai/knowledge.py` |
| Flow: EKOS grounding → model → guard → `ekos_sql_check` (one repair round) → run | `superset/ekos_ai/service.py` |
| "From EKOS" panel (tables, ledger ids, evidence, joins, warnings) | `templates/ekos_ai/ask.html` |
| `information_schema` reader removed | `schema.py` (deleted) |

### Decisions
- **No fallback** to the database's catalog: an EKOS outage is a plain "not answered".
- Ossie export over EKL + `ekos_state`: one call carries descriptions, synonyms, keys, joins, ids and
  evidence. A truncated export fetches each missing allowed table and the answers are **merged before
  the catalog is built** (built per answer, a join to a late table was dropped — caught by a test).

---

## RFC 0250 — `ekos_sql_generate`

### What was built
| Piece | File |
|---|---|
| `[sql-generate]` (`mcp`, `allow-metered`, `daily-calls` 50, `daily-tokens` 200 000, `max-turns` 6) | `compiler-core/src/config.rs` |
| `agent_generate[_with]`, `Usage` (`<state>/sqlgen/usage.json`) | `cli/src/commands/sql_check.rs` |
| Tool definition (listed only when on), dispatch, `SCOPE_REFUSED_TOOLS` entry, 4 tests | `cli/src/commands/mcp.rs` |
| `METERED_TOOLS`; gateway allowance per person (5) / per token (50), `McpMeteredUsage`; chat never offers it | `web/api/app/{mcp_policy,routes/mcp_gateway,routes/chat,models,settings,console_file}.py` |
| Superset uses it when listed; per-user 5 / instance 50 questions a day (`ekos_ai_usage` table) | `superset/ekos_ai/{service,quota}.py` |
| Demo Terraform: `superset_ekos_project` + Key Vault token; Ask (AI) off without it | `ekos-demo-public/terraform/{idempiere,variables}.tf` |

### Implementation details worth remembering
- The call is counted **before** the model is called; refusals (no question, no tables, off, metered)
  come before the count, and over the cap no provider is built (the test counts factory calls).
- One `[[databases]]` entry → its live read-only probes by default; `--allow-writable-role` is never
  offered over MCP.
- Superset spends its quota **after** EKOS answered and before any model call, so an EKOS outage costs
  a person nothing.
- An API token stands for a service (Superset) whose own users are limited there, hence a separate,
  larger gateway allowance; a person signed into the console gets 5.

### Decisions
- Dashboards keep the draft-and-check path for now: six charts would spend six generations.
- The tool returns no ledger ids (the sqlgen `Schema` carries none); Superset maps names to ids from
  its Ossie catalog.

---

## Knowledge Captured
- `requests` decodes a JSON body without a charset as Latin-1: EKOS's `—` came back as `â€”`. Set
  `response.encoding = "utf-8"` before reading `.text`.
- The current EKOS refuses the local `idemsales` project's `ekos.toml` (`profile = "p1"` and
  `dictionary` on a `kind = "postgres"` database are Oracle-only), and its ledger has 0 objects — the
  Azure demo's "EKOS with compiled iDempiere knowledge" is not there yet.
- `ekos_sql_check` did not flag `store st ON st.s_store_sk = s.ss_customer_sk` (a join off the foreign
  keys) on the TPC-DS fixture; the fixture has no FK from `store_sales` to `store`, so L002 had no path
  to compare with. Worth a look.
- The fork's CI mounts the tests at `/t` with `--noconftest`: test helpers must be imported relatively
  (`from .fake_ekos import …`), not as `tests.unit_tests…`.
- The PostToolUse hook asks for the Sonar routine after any `git push`; it applies to EKOS main only.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0250-mcp-sql-generate.md` | new RFC, accepted and implemented |
| `ekos/crates/compiler-core/src/config.rs` | `SqlGenerateConfig` |
| `ekos/crates/cli/src/commands/sql_check.rs` | `agent_generate`, `Usage`, JSON result |
| `ekos/crates/cli/src/commands/mcp.rs` | tool definition, gating, dispatch, scope decision, tests |
| `web/api/app/*.py`, `web/api/tests/test_mcp_gateway.py` | metered tools in the gateway, settings, table, tests |
| `docs/deployment.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | docs |

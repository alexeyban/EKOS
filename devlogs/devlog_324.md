# Devlog 324 — Putting ingested Apache Ossie models to work (RFC 0246)

**Date:** 2026-11-07
**PRs:** — (worked on main; commit 60145c7 + follow-ups)
**Branch:** main

---

## Summary

The user asked how an imported Ossie model can help EKOS: text-to-SQL, pipeline generation,
others. A survey of the code found that only the export read the ingested objects back. The user
chose all four uses, and all four shipped on one shared resolver.

| Use | Result |
|---|---|
| Text-to-SQL | 27/36 against 24/36 on a local model (q03 and q09 0/3 → 3/3 with multi-table metrics; one regression) |
| dbt semantic layer | passes `dbt parse` with MetricFlow validation |
| Code meanings and question resolution | source `ossie`; business words resolve to their table, with the reason |
| EKOS Migrate | catches a one-cent change on one row through declared metrics, on real PostgreSQL and ClickHouse |

---

## RFC 0246 — Ossie knowledge consumers

### What was built
| Component | Where |
|---|---|
| One resolver: metrics rewritten over tables (sqlparser AST walk), joins as column pairs, keys, synonyms, time axes; `skipped` with reasons | `crates/ossie/src/knowledge.rs` |
| Text-to-SQL: `Schema::with_ossie`, `declared_joins` + `all_joins()`, `metric` tool via `protocol_with`, metrics in `describe` and across tables in `render`, prompt instruction, L002 stated keys, **L012** | `crates/sqlgen/src/{ossie,schema,lint,agent}.rs`, `cli/src/commands/sql_check.rs` |
| `ekos export dbt-semantic [--with-time-spine]` | `crates/dbt-gen/src/semantic.rs`, `cli/src/commands/export.rs` |
| Ossie as a meaning source (`ossie_meanings`, `ossie_gloss`); commit step drops non-current Ossie objects | `semantic/src/business_semantics.rs`, `cli/src/commands/semantics.rs` |
| Synonym and metric-name resolution with `ResolvedEntity.via` (reported by `ekos_retrieve`) | `runtime/src/retrieval.rs::synonym_entities`, `cli/src/commands/mcp.rs` |
| `migrate assess`: Ossie keys → `DocCheck` source `ossie`; `migrate validate`: tier **V2m** | `cli/src/commands/migrate.rs`, `migrate-validate/src/metrics.rs` |
| A/B harness: deterministic TPC-DS SQLite, 12 questions, local Ollama | `evals/text2sql/ossie-tpcds/` |

### Measured
**Text-to-SQL.** `ekos sql generate` on `qwen2.5-coder:3b` (local, GTX 1650), 12 questions × 3
runs, with and without the official TPC-DS Ossie model in the sources:

| Question needs | without | with |
|---|---|---|
| q03 customer lifetime value (a 2-table metric) | 0/3 | **3/3** |
| q09 store productivity (a 2-table metric) | 0/3 | **3/3** |
| q10 synonyms "products", "POS data" | 3/3 | 0/3 (the agent dropped a join) |
| q06, q08 | 0/3 | 0/3 |
| the other 7 | 3/3 | 3/3 |
| **total** | **24/36** | **27/36** |

- **Determinism.** The local model answers the same way on every run of a question.
- **Change since the first single run** (6/12 → 7/12). That run gained q02/q05/q12, where the
  model's `ss_ext_sales_price` definition replaced the guessed `ss_sales_price`; in the later run
  the baseline happened to get those right. It also exposed q03/q09: their metrics span two
  tables, so `describe` never showed them. `render` now lists those once, under
  "metrics across tables", and both flipped to 3/3.
- **The q10 regression.** It is a small-model slip, not a misleading metric: the agent used
  `date_dim` columns without joining the table.
- **What this shows and what it doesn't.** A defined metric reaches the model and is used. It is
  not a benchmark claim: 12 questions on one 3B model.

**dbt.** `dbt parse` (dbt 1.12.6, dbt-metricflow) accepts the exported layer, including
MetricFlow's semantic-manifest validation. Opt-in test:
`EKOS_DBT_VALIDATE=1 cargo test -p ekos --test ossie_e2e`.

**V2m, live.** On the RFC 0156 fixture, with PostgreSQL and ClickHouse in Docker:
- a clean copy agrees on all 4 metrics;
- `amount + 0.01` on one row is caught as `metric:revenue`;
- `COUNT(DISTINCT tag_a)` stays green;
- V1 is silent.

### Decisions
- **One resolver.** Every consumer reads `knowledge::resolve`, so name resolution (the spec's
  field → expression rule) exists once.
- **The `metric` tool appears only when metrics exist** (`protocol_with`). The published BIRD
  cells' prompts stay byte-identical.
- **Declared joins are kept beside `foreign_keys`, not merged into it.** `ForeignKey` is built
  literally in many places, and the output marks a declared join as such.
- **MetricFlow measures need a time dimension on their own model.** TPC-DS `store_sales` has none,
  so its metrics are skipped with that reason rather than given an invented time axis.
- **A time spine model is written only with `--with-time-spine`.** A project already on the
  semantic layer has one, and a second would clash.
- **V2m runs only single-table metrics, with portable functions** (SUM/COUNT/AVG/MIN/MAX/NULLIF/
  COALESCE/ABS). Anything else is `not_run`. Integers are compared exactly, other numbers within a
  relative 1e-9.

---

## Knowledge Captured

- **MetricFlow requires two things the dbt-semantic-interfaces docs do not stress.** dbt parse
  found both, which no unit test would have:
  - a project with metrics needs a time spine model (`time_spine:` YAML, dbt ≥ 1.9);
  - a semantic model with dimensions needs a primary entity, or `primary_entity:` when there is
    no one-column key (TPC-DS `store_sales` has a composite one).
- **A search hit's `kind` is not filled by every backend.** The SQLite `Ledger`'s lexical hits had
  none, so filtering on `h.kind` silently dropped every Ossie object. Load the object and check
  its kind.
- **The search index matches all of a query's words.** Searching a whole question returned
  nothing. Search each keyword, then check multi-word synonyms against the question as whole
  words.
- **A multi-table metric belongs to no table**, so a per-table description never shows it. The
  A/B found that, not a test.
- **SonarCloud treats the eval scripts' `random.Random` and their argv-driven subprocess and
  output paths as security issues** (gate ERROR, security rating 3). The fix:
  - route draws through one marked helper, keeping the call sequence and so the exact same data;
  - drop the `argv` entry point of `make_db`;
  - check the binary path;
  - put `NOSONAR` with the reason on the two operator-chosen lines.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/ossie/src/knowledge.rs` | new: the resolver |
| `ekos/crates/sqlgen/src/{ossie.rs,schema.rs,lint.rs,agent.rs,lib.rs}` | Ossie in the schema, metric tool, L002/L012 |
| `ekos/crates/dbt-gen/src/semantic.rs` | new: MetricFlow renderer |
| `ekos/crates/semantic/src/business_semantics.rs` | Ossie meaning source |
| `ekos/crates/runtime/src/retrieval.rs` | synonym resolution, `via` |
| `ekos/crates/migrate-validate/src/{metrics.rs,tiers.rs}`, `tests/live_tiers.rs` | V2m |
| `ekos/crates/cli/src/commands/{export,sql_check,semantics,migrate,mcp,catalogue}.rs`, `app.rs` | wiring, `export dbt-semantic` |
| `ekos/crates/cli/tests/ossie_e2e.rs` | dbt-semantic + opt-in `dbt parse` |
| `ekos/crates/recovery/src/ossie_analyzer.rs` | resolver tests on the official TPC-DS model |
| `ekos/crates/evals/src/bench/context.rs` | struct literals |
| `evals/text2sql/ossie-tpcds/` | A/B harness |
| `ekos/docs/rfcs/0246-ossie-knowledge-consumers.md`, `CLAUDE.md`, `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | docs |

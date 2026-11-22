# Devlog 351 — Talend jobs to dbt, RFC 0268 phase 1

**Date:** 2026-11-22
**PRs:** none (direct to `main`); lakehouse devlog_27 (`95f458b`)
**Branch:** `main`

---

## Summary

RFC 0268 is written and its first phase is shipped in ekos-lakehouse. A Talend project in a workspace's sources
now becomes a dbt project for the lakehouse: `ekos lakehouse talend translate <project>`. Each translated job has:
- silver models (the chain from each source);
- gold models (one per output, materialized by its data action under a named rule);
- `ref()`s between jobs through the tables they share;
- tests on keys;
- a reviewer's report with questions.

Four gates decide whether a model may be enabled. On the five public fixtures:
- no model is disabled by a gate;
- `dbt parse` accepts the generated projects;
- many models stay disabled, each with its reason, mostly the patterns phase 2 is for.

---

## RFC 0268 phase 1 (EKOS docs; lakehouse `95f458b`)

### Problem / motivation

RFC 0267 gives each Talend job as relations. A Talend job is imperative, though: data actions, state kept in its
targets, statements run from triggers. A dbt project is queries and materializations, and the mapping between the
two has to be explicit and checked.

### What was built

| Part | What |
|---|---|
| RFC 0268 | Layout, idioms by data action, gates, files, command; phase 2 (snapshots, two-writer merges, rejects, console tab, `ekos_talend_explain`) and phase 3 (differential run, LLM fill-in) |
| `etl-dbt` (lakehouse, new) | Table resolution, PostgreSQL → Trino, and the model checks, moved from `pentaho-dbt` behind the golden gate |
| `talend-dbt` (lakehouse, new) | The translation, its gates and the report |
| `lakehouse talend scan` / `translate` | Redacted read of the project, the deployed plan, `EXPLAIN` on Trino, and a proposal branch |
| Capabilities page | "Talend projects become silver and gold dbt models" |

### Result (synthetic plan)

| Fixture | Silver enabled | Gold enabled | Questions |
|---|---|---|---|
| B | 9 / 30 | 1 / 43 | 74 |
| C | 34 / 39 | 8 / 22 | 38 |
| D | 9 / 18 | 0 / 5 | 14 |
| E | 12 / 116 | 5 / 68 | 186 |

What disables models:
- two writers of one table (B 32, E 24);
- a model reading what it writes, as a `ref()` cycle (16 each);
- readers of those;
- in E, Oracle and Snowflake sources outside the lakehouse, 20 ELT deletes and file outputs.

The Pentaho golden manifests are identical after the move.

### Decisions

- **Shared pieces extracted, the layout written for Talend.** Talend's dbt shape differs from Pentaho's (one
  table, several writers), so `translate_one` was not generalized. What both need moved to `etl-dbt`.
- **The second writer of a table is disabled with a question,** rather than guessed into one load. Phase 2
  recognises insert-new plus update-known as one merge.
- **The differential run waits for a sandbox host.** It executes a project's built third-party jobs, and the plan
  says never on a developer machine. Until then the report says **not verified by execution**.

---

## Knowledge Captured

- **The gates found three defects before any test did:**
  - a lowering bug (`$path` named but not carried), found by the anti-invention gate;
  - an enabled model reading a disabled one, found by `dbt parse`;
  - Talend text containing `{{…}}` (folded run-time values), rendered by dbt's Jinja even in comments, also
    found by `dbt parse`.

  Run `dbt parse` on generated output: any adapter works with a placeholder profile, because parse never connects.
- **An extension subcommand's positional must not be named `project`.** EKOS reads it as the global home-project
  argument.
- **A Talend job that left-joins the table it loads is a cycle of `ref()`s in dbt.** Detect it and say so; never
  emit it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0268-talend-to-dbt.md` | new; design and phase 1 result |
| `docs/generated/ekos-self-documentation.html` | Talend → dbt capability |
| `TODO.md`, `CLAUDE.md` | RFC 0268 phase 1 ticked, phases 2–3 listed; highest RFC, devlog pointer |
| ekos-lakehouse `95f458b` | `etl-dbt`, `talend-dbt`, `talend_cmd`, `talend-rel` connections and `$path`; lakehouse devlog_27 |

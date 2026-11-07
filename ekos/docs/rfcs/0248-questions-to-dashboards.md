# RFC 0248 — Questions to dashboards: triage, term resolution and gold changes

**Status:** Proposed
**Date:** 2026-11-07
**Depends on:** RFC 0247 (annotations), RFC 0215 (requirements), RFC 0228 (marts), RFC 0213 (sign-off), RFC 0242/0243
(text-to-SQL checks), RFC 0187 (chat provider)
**Plan:** `ekos-demo-public/docs/PLAN.md`, "Step 8 in the console"
**Repository:** the engine is in `ekos-lakehouse` (`lakehouse ask`); the console page and annotations are core.

---

## 1. Motivation

Step 4 turns a written requirement into marts. A person who only wants an answer should not need to write one:
they ask a question and get a dashboard, with the SQL, the chain down to the source column and the evidence. The
model must not write SQL (RFC 0242's agent showed that SQL written by a model is the main source of wrong columns).

## 2. Design

### 2.1 The model produces terms, EKOS produces SQL

The model's only output is a structured request: measures, dimensions, filters, period (JSON, schema-checked, no
SQL). EKOS resolves **every** term against the approved source model and the gold marts, applying annotations
(RFC 0247) and recording which rule applied, then **compiles the SQL deterministically** from the resolved terms.
A term that does not resolve becomes a question to the user, never a guess. The anti-invention check of step 4
(`migrate-logic`'s atom check) runs on the compiled SQL: an atom absent from the resolved terms rejects it.

### 2.2 Triage

Every request is classified at once; the first four are decided by resolution, not by a model:

| Status | Rule |
|---|---|
| `possible_now` | every term resolves in existing gold marts |
| `possible_with_addition` | resolves only with a new column or mart; no existing mart changes |
| `possible_with_change` | needs a change to an existing mart (filter, join, grain, removal) |
| `impossible` | a term has no resolution in gold, silver or the source |
| `unsafe` | RFC 0249 |

Terms missing in gold are looked up in silver along **declared keys** only.

### 2.3 Gold changes

- **Additive** (new column or mart, grain of existing marts unchanged): applied automatically if all hard gates pass.
- **Modifying**: the proposal carries the mart diff, the terms, the joins and the impact on other users' dashboards
  and annotations (`ekos_impact`).
  - **Hard gates** (deterministic, not overridable): every term resolved; 0 invented atoms; dbt tests pass;
    reconcile with silver; query size and time limits.
  - **AI review** is itself a query to EKOS: the screen shows the EKOS query, EKOS's raw answer with evidence, then
    the model's plain-language explanation and verdict. The model receives the structured diff and the gate
    results, user text in a separate data block. **It may reject what passed the gates and can never approve what
    failed them**, and adds no facts of its own.
  - **Confirmation**: impact, the one-modifying-change-per-day rule, consent text. "Continue? Yes / No". Yes
    records consent (time, text version) and uses the daily limit; No uses nothing; an AI rejection happens before
    the screen and uses nothing.
  - **Build** through a queue (one at a time) → reconcile → publish. Every change is revertible (git revert +
    rebuild; Iceberg snapshots). A concurrent change to the same mart is recomputed from the new digest.

### 2.4 Publishing and reconcile

A dashboard is published to Superset (REST + a YAML bundle committed to the shared repository). After publishing,
each chart's values are checked against the same totals computed from silver; a mismatch blocks publication.

### 2.5 Console page (step 8)

Request feed (everyone's requests are visible), review screen (SQL; the chain dashboard → chart → mart → silver →
bronze → Oracle table and column, with evidence; the term → resolution table with the rule that applied; a Trino
preview), and corrections: fix a term, add a comment, add an instruction or synonym (RFC 0247).

## 3. Data

Ledger: annotations, step 3 decisions, mart change history with digests. App database (PostgreSQL): users, limits,
consents, AI review verdicts, violations, bans, the request feed.

## 4. Measures (acceptance)

Share of the demo question set (`ekos-demo-public/demo/questions.yaml`) answered without a question back; after one
clarification; invented atoms = 0 (hard gate); every chart reconciles with silver; at least one case where an
instruction from the ledger changes a later answer by itself.

## 5. Tests

Triage table over the 18-question set (each status at least once); a planted term that cannot resolve returns a
question; an invented atom is rejected; a modifying change that fails a gate cannot be approved by the review; the
daily limit is spent by Yes only; a reversed-input test for resolution; a mart change is reverted by `git revert` and
rebuild.

## 6. Open questions

1. Is a resolution in silver allowed without a declared key when exactly one join exists?
2. Superset row-level security versus a Trino-side view per dashboard for "gold + silver only".

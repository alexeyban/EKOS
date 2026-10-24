# Devlog 294 — iDempiere Part I exit eval (RFC 0222)

**Date:** 2026-10-25
**PRs:** none (direct to `main`: d8ac5d0 dataset + first baseline, 839cce4 attributions, this commit results)
**Branch:** `main`

---

## Summary

Part I of the Oracle/iDempiere plan ends with a measured answer to "how well does the ledger answer an iDempiere
team's questions". The 25-question dataset `idempiere-25` (`evals/idempiere/`) was graded on two projects with the
same code + docs sources, one with the live Oracle database and one without, both evidence-only and answered by
Claude Haiku 5.5 through OpenCode Zen. Evidence reached the model for 9 of 18 answerable questions in both; answers
passed 14/25 (with the database) and 15/25 (without), with no hallucination and every adversarial question
declined. The database adds nothing measurable yet: the one question only it answers fails because retrieval never
reaches the live profile. Every miss is attributed, and the follow-ups are listed in RFC 0222 §4.4.

---

## RFC 0222 — Part I exit eval

### Problem / motivation
Part I built the Java, Oracle DDL/PL-SQL, schema-history, live-catalog, write-behaviour, ticket and docs analyzers.
Nothing said how well the result answers real questions, or what the database adds over the files.

### What was built

| Component | What it is |
|---|---|
| `evals/idempiere/{code,schema,history,docs,behaviour,adversarial}.yaml` + `manifest.yaml` | 25 scenarios (6/4/3/3/3/6), facts checked in `release-13`, the docs repo or the testbed; `any_of` alternates |
| RFC 0222 §4 | both projects × both gradings, per-scenario attribution, follow-ups |

### Measured

| | `idemdocs` (with DB) | `idemnodb` (no DB) |
|---|---|---|
| evidence shown (of 18) | 9 | 9 |
| passed (of 25) | 14 | 15 |
| correctness / groundedness / completeness | 48.1 / 84.0 / 45.6 % | 53.7 / 88.0 / 50.9 % |
| hallucination | 0 % | 0 % |
| answered but uncited | 2 | 3 |
| eval spend | $0.023 | $0.022 |

`idemnodb` pipeline with Zen SQL naming: build 6 s, recover 3,601 s (21,302 passes, 267 model calls, $2.30),
resolve 17 s, compile 29 s, commit 1,767 s.

### Implementation details worth remembering
- The two projects differ by three scenarios, and each difference is a citation the model left out (idc-005,
  ids-003, idd-001), not a fact the database supplied. One-scenario swings between projects are noise at n = 25.
- The nine retrieval misses are identical in both projects. Five of them are docs sections (2,520 in the project)
  outranking the Java class that implements the answer.

### Decisions
- **Cloud model, not local, for the answered grading.** Planned with Ollama `llama3`; the 15 GB machine could not
  hold llama3, the pipeline and the user's Trino together (two runs reaped). The maintainer asked for the cloud
  model; each eval run was capped at `budget-usd = 0.5`, the pipeline at 8.
- **No `[llm]` block during a pipeline unless meant.** A project's `[llm]` makes `recover` describe every SQL file
  with the model. The no-database project carried `ollama/llama3` from the planned eval and spent 9 h 46 min on 225
  calls before it was stopped.

---

## Knowledge Captured

- **`[llm]` in a project's `ekos.toml` turns on per-file SQL naming at `recover`.** Configure a model only for an
  eval and it still runs during the next pipeline: thousands of calls locally (≈3 min each for llama3 on CPU, prompts
  truncated at 8,192 tokens, many unparseable), or real spend on a cloud provider. Large migration files average
  ~75k input tokens per call with Haiku.
- **A killed command writes no `llm` line.** The run log's model-call count is written when the command ends; a run
  stopped mid-way shows none, while `llm-usage.jsonl` already records every call. Read the usage file for a run that
  did not finish (not a bug, mistaken for one here).
- **`pkill -f <script>` from a tool shell can kill that shell** when the command line names the script; kill by PID.
- Haiku 5.5 on Zen: about 2 US cents for a 25-question eval (~6,100 tokens a question, P95 7.8 s).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0222-idempiere-part-one-eval.md` | design note on the cloud grading; full §4 results, attribution, follow-ups |
| `TODO.md` | Part I exit ticked; RFC 0222 follow-ups |
| `README.md`, `docs/generated/ekos-self-documentation.html` | iDempiere eval section |
| `CLAUDE.md` | latest devlog 294 |

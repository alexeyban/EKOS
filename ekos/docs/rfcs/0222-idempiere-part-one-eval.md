# RFC 0222 — iDempiere Part I Exit Eval

**Status:** Accepted
**Date:** 2026-10-24
**Depends on:** RFC 0138/0139/0177 (eval harness, `--evidence-only`), RFC 0209–0212, 0219, 0221
**Plan:** `oracle-idempiere-plan.md` v3, Part I exit — "25 questions with expected citations through
`ekos eval`, uncited answers counted against groundedness", without the database first, then with it.

---

## 1. Motivation

Part I built the analyzers; nothing yet says how well the ledger answers the questions an iDempiere
team would ask. A fixed question set, graded the same way on every run, turns "it seems to work" into
numbers that a later change can move — and shows what the database adds over files alone.

## 2. Design

**Dataset.** `evals/idempiere/` (its own directory, so the EKOS self-eval in `evals/datasets/` is
untouched), dataset `idempiere-25`, six files:

| File | Category | Questions |
|---|---|---|
| `code.yaml` | application code (Java) | 6 |
| `schema.yaml` | migrations, DDL, PL/SQL | 4 |
| `history.yaml` | git history, tickets | 3 |
| `docs.yaml` | the documentation repository | 3 |
| `behaviour.yaml` | write behaviour; one question only the live database answers | 3 |
| `adversarial.yaml` | no grounded answer; the correct answer declines | 6 |

Every expected fact was checked in the iDempiere `release-13` source, the docs repository or the live
testbed before it was written down; a question whose premise is false (`T_ReportStatement.Account_ID`
was never added by a migration, RFC 0219 §6) is an adversarial one. Facts are never tuned to what a run
retrieves.

**Two gradings.**

1. `ekos eval run --evidence-only`: no model call; whether each scenario's expected facts are in the
   evidence a model would be shown (`facts_shown`).
2. `ekos eval run` answered by a model, graded by the harness's evaluators — an uncited answer scores 0 on
   groundedness, a confident answer to an adversarial question counts as a hallucination. Planned with local
   Ollama `llama3`; run with Claude Haiku 5.5 through OpenCode Zen (`[llm] provider = "zen"`, `budget-usd = 0.5`
   per run) at the maintainer's request, after the local model could not run beside the pipeline in 15 GB.

**Two projects**, identical sources (`idempiere` code + `docs`, full git history, `[tickets]`), one with
`[[databases]]` (Oracle XE testbed, P2 profile, dictionary) and one without.

## 3. Exit

Both projects measured both ways, results recorded here; each miss attributed (not shown in evidence /
shown but not answered / not in the ledger). Raising the scores is follow-up work with its own RFCs.

## 4. Results

Run 2026-10-24/25, release build, scratch `EKOS_HOME`, dataset `idempiere-25`, ruler as shipped.

| Project | Sources | Pipeline model calls |
|---|---|---|
| `idemdocs` | `idempiere` (code, git) + `docs` + Oracle XE (P2 profile, dictionary) | 0 |
| `idemnodb` | the same two sources, no `[[databases]]` | 267 (`sql-naming`, Haiku 5.5 via Zen, $2.30; 9 failed) |

`idemnodb`'s pipeline: build 6 s, recover 3,601 s, resolve 17 s, compile 29 s, commit 1,767 s; 21,302 passes,
409,177 objects / 635,714 relationships loaded at resolve. An earlier attempt with local `llama3` made 225 SQL
naming calls in 9 h 46 min (≈3 min each, prompts truncated at the 8,192-token window, many replies unparseable) and
was stopped.

### 4.1 Evidence only

| | `idemdocs` | `idemnodb` |
|---|---|---|
| expected facts shown (of 18 answerable) | 9 | 9 |
| time | 38 s | 27 s |

The same nine scenarios miss in both projects:

| Scenario | Cause |
|---|---|
| idc-002 (completing an order) | `completeIt` exists in dozens of document classes; docs pages outrank `MOrder` |
| idc-003 (who sets GrandTotal) | `StandardTaxProvider` not retrieved; a docs page on the tax lookup interface and `C_Tax_Trl` lead the evidence |
| idc-004 (DocStatus `CO`) | `doc/DocStatus.txt` retrieved, but its excerpt ends before the `CO Completed` line |
| idc-005 (C_BPartner model class) | `MBPartner` not retrieved; docs pages on event handlers and model factories lead the evidence |
| idc-006 (change log) | `PO.java` not retrieved; docs pages (auto-save of tab changes) lead the evidence |
| ids-001 (migration for a column) | the `C_Country` model's column map fills the evidence; the change set is not shown |
| idb-001, idb-002 (deletes) | `WriteBehaviour` objects are not reached by search for "deleted" |
| idb-003 (live row count) | the live table's profile is not surfaced for "how many rows" — in `idemnodb` there is none to surface |

In five of the nine, documentation sections take the places the code would have had: with 2,520 docs sections in
the project, prose that mentions a term outranks the class that implements it.

### 4.2 Answered (Claude Haiku 5.5 via Zen)

| | `idemdocs` | `idemnodb` |
|---|---|---|
| passed | **14 / 25** | **15 / 25** |
| answer correctness | 48.1% | 53.7% |
| groundedness | 84.0% | 88.0% |
| completeness | 45.6% | 50.9% |
| hallucination rate | 0.0% | 0.0% |
| answered but uncited | 2 | 3 |
| refusals | 14 | 12 |
| attribution: retrieval / generation | 9 / 1 | 9 / 1 |
| model spend | $0.023 | $0.022 |
| time | 170 s | 149 s |

All six adversarial questions were declined in both projects (no hallucination). The answered scenarios that differ
between the projects differ by citation, not by the database:

| Scenario | `idemdocs` | `idemnodb` |
|---|---|---|
| idc-005 | uncited, fail | names `X_C_BPartner` (extends `PO`) from the naming-convention docs page, cited; half the facts — pass |
| ids-003 | correct but uncited, fail | the same answer, cited — pass |
| idd-001 | pass | correct but uncited, fail |

idh-001 (who committed IDEMPIERE-6349) is the one generation miss in both: the commit was shown, the answer said the
author was not.

### 4.3 What the database adds — nothing measurable yet

Both projects score within one scenario of each other, and the swing comes from citations. idb-003, the one question
only the live database answers, fails in `idemdocs` too: the live profile holds the row count but retrieval never
reaches it. So the live catalog, profiles and dictionary (RFC 0211/0212) are in the ledger but not yet in answers.
`idemnodb`'s 267 cloud-named SQL files changed no score either.

### 4.4 Follow-up (own RFCs)

1. **Code over prose in ranking** — when a question names a table or a class, the `JavaType`/`ModelTable` implementing
   it should outrank docs sections that only mention it (idc-002/003/005/006).
2. **Excerpt windows** — a matched line beyond the excerpt end (idc-004).
3. **Write behaviour and live facts in retrieval** — "deleted", "change log", "how many rows" should reach
   `WriteBehaviour` and live-profile objects (idb-001..003).
4. **Change sets for a column** — a question naming `table.column` should reach the `SchemaChange` that added it
   (ids-001).
5. **Always cite** — four correct answers lost groundedness for a missing citation; the ask prompt's citation rule
   needs a check or a retry.

Exit (§3) met: both projects measured both ways, each miss attributed.

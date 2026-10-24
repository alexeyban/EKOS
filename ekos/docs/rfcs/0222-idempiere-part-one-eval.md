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

**Two gradings, no metered model.**

1. `ekos eval run --evidence-only`: no model call; whether each scenario's expected facts are in the
   evidence a model would be shown (`facts_shown`).
2. `ekos eval run` with local Ollama (`[llm] provider = "ollama"`, `model = "llama3"`): answers graded by
   the harness's evaluators — an uncited answer scores 0 on groundedness, a confident answer to an
   adversarial question counts as a hallucination.

**Two projects**, identical sources (`idempiere` code + `docs`, full git history, `[tickets]`), one with
`[[databases]]` (Oracle XE testbed, P2 profile, dictionary) and one without.

## 3. Exit

Both projects measured both ways, results recorded here; each miss attributed (not shown in evidence /
shown but not answered / not in the ledger). Raising the scores is follow-up work with its own RFCs.

## 4. Results (partial)

**With the database, evidence only** (project `idemdocs`: code + docs + Oracle XE, 2026-10-24, 38 s, no model
call): expected facts shown for **9 of 18** answerable scenarios; the 6 adversarial ones are not graded without
answers. Misses, by cause:

| Scenario | Cause |
|---|---|
| idc-002 (completing an order) | `completeIt` exists in dozens of document classes; docs pages outrank `MOrder` |
| idc-003 (who sets GrandTotal) | `StandardTaxProvider.calculateInvoiceTaxTotal` not retrieved; setter calls are not recorded as writes |
| idc-004 (DocStatus `CO`) | `doc/DocStatus.txt` retrieved, but its excerpt ends before the `CO Completed` line |
| idc-005 (C_BPartner model class) | the `ModelTable` column list fills the evidence; the class objects are not shown |
| idc-006 (change log) | `PO.java` not retrieved for "change log" |
| ids-001 (migration for a column) | the `C_Country` model's column map fills the evidence; the change set is not shown |
| idb-001, idb-002 (deletes) | `WriteBehaviour` objects are not reached by search for "deleted" |
| idb-003 (live row count) | the live table's profile is not surfaced for "how many rows" |

**Not yet run:** the project without the database (its pipeline was stopped twice by the machine running out of
memory) and both Ollama-answered gradings.

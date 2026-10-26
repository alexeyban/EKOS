# Exit — the plan's release gates on iDempiere (plan v3, Part VI.4)

**Date:** 2026-10-26 · **Project:** `idemdocs` (iDempiere `release-13` code + docs + Oracle XE 21c, testbed home) ·
**EKOS** `54dd67f` · **ekos-lakehouse** `c49c29f` · **Model calls for this report:** 0

Every gate of the plan, with what measures it and where the evidence is. Two measurements were taken for this report
(model calls per step, row values in the ledger); the rest cite the run that produced them.

## BLOCKERS

1. **Code ↔ database conflicts unexplained at sign-off: 195, gate 0.** Step 3's `conflicts` subject cannot be
   signed while they are open, and with `identity` (1 candidate) it keeps steps 4–7 behind the gate. Each needs a
   person: a column the model declares that the database lacks, a mandatory column the database allows null, … The
   review shows them; deciding them is not the software's.
2. **Planted deletes not all exercised: cascade, no key, permissions loss.** Single, delete-then-reinsert, mass
   (guard tripped) and purge-with-retain ran live (RFC 0225). A cascade delete, a table without a key and a
   permissions loss tripping the guard are classified and designed but were not planted on the testbed.

## The gates

| Gate | Bar | Measured | Status | Evidence |
|---|---|---|---|---|
| Model calls in steps 1, 2, 3, 5, 6 | 0, printed by every run | step 1: 24 runs, 0 · step 2: 1 run, 0 · step 5: 6 runs, 0 · steps 3 and 6: no run log (those runs predate RFC 0218) | **met where printed; 3 and 6 not printed** | `model_calls.py` below |
| Part I eval, without and with the database | groundedness measured and published | 14–15 of 25 answered, no hallucination; evidence shows 9 of 18 expected facts in both projects | met | RFC 0222, `evals/idempiere/` |
| Java / SQL call sites / PL/SQL coverage | reported by count and fidelity | 763 model tables, 16,660 columns; 9 of 62,251 migration statements rejected; 61 of 61 routines | met | RFCs 0209, 0210; `docs/operator-workflow.md` step 1 |
| Census reconciled against `ALL_OBJECTS` | exact | tables 903 = 903, views 160 = 160, routines 62 = 62, triggers 0 = 0; 10,613 objects, 0 mismatches | met | the demo book, step 1 (2026-10-26 run log) |
| Code ↔ database conflicts unexplained at sign-off | 0 | 195 | **BLOCKER** | RFC 0213, `part2-review/` |
| Sign-off invalidated by a changed fact | demonstrated | a column added in Oracle turned `pii` stale, naming it; `scope` stayed valid | met | RFC 0213 §6 |
| Steps 1–3 completed without a terminal | yes, recorded by the UI tour | step 1 and the sign-offs ran in the browser (`part2-sources/`, `part2-review/`); step 3 is not complete (blocker 1) | partial | the demo book, steps 1–3 |
| Planted deletes handled as designed | all; mass-delete guard trips | single, re-insert, mass (guard tripped, nothing applied), purge `retain`: as designed; cascade, no key, permissions loss: not planted | **BLOCKER** (partial) | RFC 0225 §5 |
| Source ↔ bronze unexplained divergences | 0 | 12 tables loaded and reconciled live, green; `C_ORDER`, `C_PAYMENT` from the generated SQL green; 165-table subset through dbt: 3,664 of 3,667 tests pass, 3 data warnings explained | met for what ran; the full 849-table load was not run (maintainer's decision) | RFCs 0216, 0227 |
| Gold anti-invention | 100% of terms resolved or asked | 4 marts: every term resolved, none invented; the model's draft needed 9 answers from the operator, none from the model | met | RFCs 0215, 0228 |
| Row values in the ledger | 0 | 228 text values sampled from 36 business columns: **0** in a profile or any database object; 190 matches are bounds of `NUMBER` columns (kept by design) | met for text; numbers and dates keep min/max by design | below |
| Pagila / LedgerSMB gates after the seam refactor | unchanged | Pagila byte-identical (84 files) and 152 tests green; LedgerSMB's database is gone from the sandbox | partial | RFC 0216 §5 |

## Model calls per step

```bash
python3 evals/oracle-idempiere/exit/model_calls.py <state>/runs <home>/control/runs <console db> idemdocs
```

| Step | Runs with a run log | Model calls |
|---|---|---|
| 1 | 24 | 0 |
| 2 | 1 | 0 |
| 3 | 0 | — |
| 4 | 6 | 0 |
| 5 | 6 | 0 |
| 6 | 0 | — |
| 7 | 1 | 0 |

Step 4 may call a model (the plan's one LLM step); its draft by local `llama3` ran before run logs covered it. Steps
3 and 6 will print their count with the next sign-off and the next load: every command that changes something writes
a run log since RFC 0218.

## Row values in the ledger

```bash
cargo run -p ekos --release --example value_scan -- --home <home> --project idemdocs --values <file> --all
```

The values: distinct text values (≥ 5 characters, up to 200 per column) read through Trino's read-only Oracle catalog
from 36 columns of `C_BPARTNER`, `AD_USER`, `C_LOCATION`, `M_PRODUCT`, `C_ORDER`, `C_INVOICE`, `C_PAYMENT`,
`C_BANKACCOUNT`, `C_BP_BANKACCOUNT`, `M_INOUT`, `C_PROJECT`, `R_REQUEST` — names, e-mails, phones, addresses,
document numbers, account numbers. They are not kept in this repository.

The scan: every current object (162,851) and relationship (282,146), serialised. 77 of the 228 values occur
somewhere; 235 of the hits are objects read from source files — the documentation and code mention the demo data
(`GardenWorld`, `Joe Block`, document numbers in tests and migration scripts) — and are those files' own text. In
objects read from the database (`origin: live`), 195 hits:

| Where | Hits | What it is |
|---|---|---|
| `columns[n].profile.min/max` of a `NUMBER` column | 190 | a numeric bound equal to a sampled document number (`200000`, `10000000`, …) |
| table names, code citations, view SQL, comments | 47 occurrences in these records | metadata containing a sampled word (`WebService`, `Standard`, `Vendor`, `System`) |
| check constraints | 13 | DDL text (`IsLandscape in ('Y','N')`) |
| PL/SQL conditions | 4 | routine source |
| an object id, a timestamp | 3 | digits that happen to match |
| a profile bound of a text column | **0** | |

So no row's text reached the ledger. Numeric and date columns keep their minimum and maximum (RFC 0211's profile
`p2`): those are values from rows, kept on purpose, never shown in the estate documentation. If the gate is read as
"no value at all", the profile must drop to `p1`.

Only current versions were read; earlier versions in the append-only ledger are outside this scan.

## Not in the gates, for the record

- RFC 0217 (CDC) is not built: the plan builds it only when an operator asks.
- Step 2 was published afterwards, at the maintainer's request, to the demo repository's wiki (RFC 0230).
- Generated Dagster schedules start switched on; on a test machine that launched the full run once (cancelled before
  any step, RFC 0228).

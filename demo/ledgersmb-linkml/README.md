# Demo: a real LinkML schema for LedgerSMB

This demo takes [LedgerSMB](https://github.com/ledgersmb/LedgerSMB), an open-source accounting system with 20 years of history in PostgreSQL, PL/pgSQL and Perl, and recovers a [LinkML](https://linkml.io) schema of its business semantics from the source alone (RFC 0170). Along the way you can see what EKOS finds, review it, export it, and check the result with LinkML's own tools.

```bash
demo/ledgersmb-linkml/run.sh                                   # clones LedgerSMB from GitHub
LEDGERSMB_SRC=~/src/LedgerSMB demo/ledgersmb-linkml/run.sh     # or from a local clone
SKIP_REVIEW=1 demo/ledgersmb-linkml/run.sh                     # hypotheses only, no review
```

**Requirements:** `cargo`, `git` and `python3` with `venv`. The script installs `linkml==1.11.1` into a local venv.

**No LLM is called.** `[llm]` points at an environment variable that does not exist, and `llm-definitions` stays off.

**Run time:** about 10 minutes. Most of that is `ekos commit` writing roughly 11k objects, 8k relationships and 11k evidence records to a fresh ledger. The release build adds time on the first run.

Everything the script writes goes to `work/`, which is git-ignored. `output/` holds a copy of one run's schema and transcript, so you can read the result without running anything.

## What happens

| Step | Command | What you see |
|---|---|---|
| 1 | `git checkout 544bcd947` | LedgerSMB master as of 2026-09-13, pinned so the numbers below reproduce |
| 2 | `ekos build → recover → resolve → compile → commit` | Synthesis runs at the end of `commit`, because `[semantics] enabled = true` |
| 3 | `ekos semantics list / show / gaps` | Everything is a **hypothesis**, each with `path:line` evidence |
| 4 | `ekos semantics confirm / edit / reject --as demo-reviewer` | A human review, played by the script |
| 5 | `ekos export linkml` → edit YAML → `ekos import linkml` | The same review done inside the LinkML file |
| 6 | `ekos export linkml`, then `linkml-lint`, `gen-json-schema`, `gen-pydantic`, `linkml-validate` | A schema that LinkML's own tools accept and generate from |

## What EKOS recovers (step 2–3)

```
Business semantics: 47 concept(s), 222 coded value(s) in 61 column(s) (179 explained), 60 constraint(s),
15 gap(s), 0 conflict(s), 128 rationale link(s), 17 mapping suggestion(s) — all hypotheses
(458 of 649 predicate sites resolved)
```

Each item shows where it came from. For example, the concept behind `entity_class = 2`:

```
EntityCreditAccountCustomer — BusinessConcept
  definition: "entity_credit_account.entity_class IN (2)"
  evidence:
    sql/modules/Company.sql:1297  entity_credit_account.entity_class IN (2) (eca__get_pricematrix_by_pricegroup, Where)
    sql/modules/Company.sql:1310  entity_credit_account.entity_class IN (2) (eca__get_pricematrix, Where)
    sql/modules/Company.sql:1397  entity_credit_account.entity_class IN (2) (eca__save_pricematrix#8, Where)
  links:
    ExplainedBy → 439aaf1160 More customer/vendor pricematrix work, … (RationaleLink)
    Describes → entity_credit_account (Table)
```

Code meanings carry their sources too. For example, `entity_class = 2 → Customer` is backed by both the `entity_class` seed rows and Perl's `use constant EC_CUSTOMER => 2`. Codes that EKOS cannot explain become gaps. For example: *What does `account_link.description = 'AP'` mean? It is used in logic, but no comment, lookup row or CASE label explains it.*

## Review (steps 4–5)

Only a human can confirm an item. No MCP tool can do it, and a source-scan test enforces that. In this demo **the script plays the reviewer** (`--as demo-reviewer`). Each decision cites the LedgerSMB text it rests on:

| Decision | Grounds in LedgerSMB |
|---|---|
| `account.category` A/L/Q/I/E → Asset / Liability / Equity / Income / Expense (labels capitalized), plus its `CHECK` | `COMMENT ON COLUMN account.category IS 'A=asset,L=liability,Q=Equity,I=Income,E=expense'` |
| `entity_credit_account.entity_class` 1/2/3 → Vendor / Customer / Employee | `entity_class` seed rows; `LedgerSMB::Magic` constants (`EC_CUSTOMER => 2`) |
| `oe.oe_class_id` 1–4 → Sales Order / Purchase Order / Quotation / RFQ | `oe_class` seed rows; `COMMENT ON TABLE oe` |
| `EntityCreditAccountCustomer` → **CustomerAccount**, `…Vendor` → **VendorAccount** | the seed rows above |
| `PartsAssembly` → **Assembly**, `PartsWithInventoryAccnoId` → **InventoryPart**, `PartsNotObsolete` → **ActivePart** | `COMMENT ON TABLE parts` ("If assembly is true, then an assembly …") |
| `OeNotClosed` → **OpenOrder**, `TransactionsApproved` → **ApprovedTransaction** | `oe.closed`; `approved` filters in 28 report queries |
| **reject** `AccountLinkDescriptionSummaryT` | `'t'` is PostgreSQL's text form of `true`, so this is the same filter as `AccountLinkDescriptionSummary` |
| `YearendNotReversed` → **ActiveYearEnd**, done by editing the exported YAML | step 5: `ekos import linkml --dry-run`, then apply |

The final list has 21 confirmed items: 8 concepts, 12 coded values and 1 constraint. The other 300-odd items stay hypotheses. **The demo's reviewer is not an accounting expert.** These confirmations are believable readings of LedgerSMB's own comments, not an expert's verdict.

## The schema (step 6)

`ekos export linkml` defaults to `--status confirmed`, so no hypothesis leaves EKOS looking like a fact. The confirmed export has 14 classes and 3 enums in about 730 lines. `--status all` writes every item with its status, about 5,900 lines.

```yaml
  CustomerAccount:
    is_a: EntityCreditAccount
    description: A credit account held by a customer (entity_class 2).
    annotations:
      ekos_id: 074fe04f-ed9b-548d-9201-6bc644be2ea6
      ekos_status: confirmed
      ekos_definition: entity_credit_account.entity_class IN (2)
      ekos_evidence: sql/modules/Company.sql:1297; sql/modules/Company.sql:1310; sql/modules/Company.sql:1397
      ekos_rationale: commit 1b5baf71b0 "Groundwork for fix on pricematrix issues" (2011-11-22); …
enums:
  AccountCategory:
    permissible_values:
      A:
        description: Asset
        annotations:
          ekos_status: confirmed
          ekos_reviewed_by: demo-reviewer
          ekos_source: case_label, column_comment
          ekos_evidence: sql/Pg-database.sql:72; sql/modules/FinStatements.sql:690; …
```

Table classes such as `Account` come from the DDL and are marked `ekos_status: observed`. Their `NOT NULL` columns become `required`. Because the `account.category` `CHECK` was confirmed, `category`'s range becomes the `AccountCategory` enum, and LinkML's own validator then enforces it:

```
== 6a. linkml-lint
   errors: 0, warnings: 83 (recommended 71, standard_naming 12)
== 6b. gen-json-schema, gen-pydantic
   ledgersmb.schema.json (50432 bytes)
   ledgersmb_models.py (1082 lines)
== 6c. linkml-validate: records against the recovered schema
   account-ok.yaml (category A): valid
   account-bad.yaml (category X): INVALID
     [ERROR] 'X' is not one of ['A', 'E', 'I', 'L', 'Q'] in /category
```

The 83 lint warnings are style advice, not errors:
- 71 are slots without a description. The SQL has no comment for those columns, and EKOS does not invent one.
- 12 are permissible values named by their code (`A`, `1`). Those are the values the database actually stores.

## What this demo does not show

- **Quality numbers.** `ekos semantics eval` needs a gold set written by an accounting expert *before* seeing any output (`ekos semantics gold-template` produces the blank form). The starter set in `ekos/docs/rfcs/0170-ledgersmb-starter-gold.yaml` is neither expert-written nor blind.
- **Meaning without a literal trace.** Concepts like *overdue* or *unpaid* are defined by comparing columns or dates (`duedate < current_date`, `amount > paid`). EKOS records column-vs-literal predicates, so it does not recover these.
- **Schema history.** LedgerSMB creates `user_preference` twice: once in `sql/Pg-database.sql`, and again in `sql/changes/1.9/transpose_user_prefs.sql`, which replaces it. Both definitions get the same id, and the one read last wins. Since devlog_242 the read order is fixed (source path, then pass), so the `changes/1.9` definition wins every time. Before that fix, the winner varied between runs and so did the counts (46 or 47 concepts). EKOS still does not apply `sql/changes/` as migrations on top of the base schema (see TODO).

## Files

| File | |
|---|---|
| `run.sh` | the whole demo |
| `output/ledgersmb.linkml.yaml` | the confirmed schema from one run |
| `output/transcript.txt` | that run's console output |
| `work/` (git-ignored) | the LedgerSMB clone and EKOS workspace, the LinkML venv, every generated file |

After a run, the workspace stays live:

```bash
cd demo/ledgersmb-linkml/work/LedgerSMB
ekos semantics gaps                        # what is still unknown
ekos semantics list --status hypothesis    # what is left to review
ekos mcp serve --workspace .               # agents: ekos_semantics_lookup / ekos_semantics_gaps
```

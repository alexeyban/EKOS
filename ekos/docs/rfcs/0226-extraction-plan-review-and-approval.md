# RFC 0226 — EKOS Lakehouse: the extraction plan as a reviewed decision (step 5)

**Status:** Accepted
**Date:** 2026-10-26
**Plan:** Oracle/iDempiere plan v3, Part II.5 (step 5's UI)
**Repos:** ekos-lakehouse (`bronze`, `silver`, `lakehouse-ext`); EKOS (`extension.rs` actions with inputs, the
console's extension page, one registry row)
**Depends on:** RFC 0213 (review subjects, sign-off), RFC 0198/0207 (extension pages), RFC 0225 (the plan from write
behaviour)

## 1. Problem

`lakehouse plan` proposes how each table lands, and `lakehouse generate` writes whatever a fresh plan says. Nobody
agrees to it, and nothing step 3 decided reaches it: a table the operator put **out of scope** is still planned, a
**PII** column the operator confirmed is not masked unless someone also writes it into `ekos.toml`, and a
**write-behaviour override** ("this table is deleted by a nightly job the code does not show") does not change the
delete plan. Step 5 of the workflow asks for a page where the operator sees the plan with its evidence, changes it per
table, and approves it — and for the approved plan to be the one that is built.

## 2. Design

### 2.1 Step 3 in (per subject, only when signed off)

The plan reads the review (EKOS `review_state`, cached) and uses a subject's decisions only while that subject's
sign-off is **valid**; an unsigned or invalidated subject is reported and ignored, never half-applied.

| Subject | Effect on the plan |
|---|---|
| `scope` | a table whose decision (or, undecided, proposal) is `out` is not planned; it is listed as `excluded`, with the reason "out of scope (step 3)" |
| `pii` | `confirm` with mask `hash` → silver HMAC, `redact` → NULL, `none` → no mask; `reject` → none. An `ekos.toml` `pii` entry for the same column wins and is reported |
| `write-behaviour` | an `override` replaces the overridden fields (`update_class`, `delete_classes`) of that table's facts before the mode and delete plan are chosen; the evidence says so |

Without a ledger or a review, the plan is what it was (the Pagila lock is byte-identical).

### 2.2 The proposal

`lakehouse plan` keeps printing the plan and now also saves it to `<state>/lakehouse/extraction/proposed.json`: the
plan, the masks it would apply, the excluded tables, the step 3 subjects used and their sign-off digests, and when.
It needs the source (class *external*); everything below reads only that file and the state directory.

### 2.3 Changes per table

`lakehouse extraction change --table T <change> [--reason R] [--by P]` (class *review*: a person, never a token or an
agent), one of:

| Change | Allowed when | Effect |
|---|---|---|
| `--mode full` | always | full load; no delete plan |
| `--mode incremental` | the table has a primary key and the application watermark column, orderable | incremental on it, with the RFC 0225 delete plan; **needs `--reason`** when the facts kept it full (some code skips `UPDATED`) — the evidence then says the operator accepted that risk |
| `--deletes run` / `--deletes audit` | the table has a delete plan | cadence |
| `--purge retain` / `--purge propagate` | the delete plan's kind is `purge` | policy |
| `--undo` | a change exists | removes it |

Changes are kept in `<state>/lakehouse/extraction/changes.json` (table, change, reason, who, when) and applied to
the proposal on every read, so a re-plan keeps them; a change that no longer fits the new proposal is shown as
`stale` and not applied.

### 2.4 Approval

`lakehouse extraction approve [--note N] [--by P]` (class *review*): refuses without a proposal or with a stale
change. It writes `<state>/lakehouse/extraction/approved.json` (the final plan and masks) and a ledger fact
`LakehouseApproval` — who, when, the **digest** of the approved plan, counts by mode and cadence, the changes with
their reasons, the step 3 digests used. No key or value is in it.

The digest covers what a build depends on: per table its names, key, mode, delete plan, column mappings and renames;
the refused and excluded tables; the masks; the plan settings. Evidence text and notes are excluded (they carry row
estimates, which move between runs).

### 2.5 The approved plan is the one built

With `require-plan-approval = true` (new setting, default off), `lakehouse generate` (and `sync`) re-plan from the
source as today, apply the changes, and compare the digest with the approval's: equal → write, as before, the lock
then being exactly what was approved; different → refuse, naming the tables whose part of the digest changed ("the
source or step 3 changed since the plan was approved: plan and approve again"). `--from-lock` is unchanged.
Without the setting, `generate` applies the changes and step 3's decisions but needs no approval.

### 2.6 `lakehouse extraction --json [--table T]` and the console page

A read command (class *read*) over the state files: the summary (approval: by, at, digest, still matching the
proposal or not; counts; changes pending since the approval; step 3 subjects used or ignored and why), one row per
table (mode, watermark, delete cadence and kind, purge policy, contradiction with the code, columns not exact, masked
columns, the change), the selected table's evidence, notes and columns, the refused and the excluded tables.

The console page `extraction` (RFC 0207 page) shows those panels, a table selection, and the actions: **Plan**,
the changes for the selected table, **Approve plan** (with a confirmation sentence).

A reason is free text, which a page action could not take. EKOS gains `Action::inputs`: `(argument, label)` pairs the
person types before the run, sent as that value argument through the same catalogue path (≤512 characters, no NUL,
checked again by `build_argv`). The page checker accepts an argument as given when it is bound, an input, or the
console-set reviewer `by` of a review command.

### 2.7 Workflow step 5

Done when an approval exists whose digest matches the current proposal; partial when a proposal exists without one
(or with changes since); todo otherwise. It replaces "tables synced".

### 2.8 Not in this RFC

Step 6 (silver from step 3's labels, retained purges, the SCD2 close, a Dagster run), a found delete as a
`ConflictingEvidence` review item, masks other than the two silver has.

## 3. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | step 3 in; proposal saved | iDempiere: excluded = scope `out`, masks = confirmed PII, overrides change the delete plan; Pagila lock identical |
| 2 | `extraction change`/`approve`/`--json`, digest, gate in `generate`, step 5 | unit tests: each change's rule, stale change, digest stability over row estimates, gate refuses a changed table |
| 3 | `Action::inputs` in EKOS; the console page | the page passes the checker; in a browser: a change with a reason, approve, step bar shows step 5 done |

## 4. Tests

Pure: step 3 application per subject (signed / unsigned / invalidated), change rules, staleness, digest over row
estimate changes (stable) and over a mode change (moves). CLI: plan → change → approve → generate under the gate;
a re-plan after a source change refuses generate. EKOS: the checker with inputs and `by`; `build_argv` with an input;
the console test for an action with inputs.

## 5. Results

iDempiere testbed (`idemdocs`, 903 tables; step 3: scope, PII and write behaviour signed off, conflicts and identity
not), 2026-10-26.

**Phase 1 — step 3 in.** `lakehouse plan` (release, 20 s with the review cached): 849 tables planned, 1 refused, 53 out
of scope — 903. The 53 are exactly the scope subject's `out` set (51 proposed `out` by class: `T_*` transient, `I_*`
import, `*LOG` tables; 2 decided `out`), counted independently from the review cache. One mask, `AD_ORGINFO.EMAIL` →
HMAC, the one PII candidate confirmed (with mask `hash`); the 195 undecided candidates are not masked. Against RFC
0225's plan (739 incremental, 162 full of 901), the 53 came out of incremental (24), full (28) and refused
(`I_CONVERSION_RATE`): 715 incremental (665 deletes every run, 50 audited), 134 full. No write-behaviour override was
decided on the testbed; overrides are covered by the unit tests. Pagila (no review): lock and SQL byte-identical;
only Dagster's definitions differ, as after RFC 0225.

**Phase 2 — changes, approval, the gate** (CLI, `require-plan-approval = true`):

| Step | Result |
|---|---|
| `change --table AD_MESSAGE --full` | recorded |
| `change --table A_ASSET --incremental` without `--reason` (facts: `raw_sql_skips_updated`) | refused: say why |
| the same with a reason | recorded; the proposal's reasons kept as "the proposal said: …" |
| `change` on `T_SPOOL` (out of scope) / `--keep-purged` on a table not only purged / `--full` on a full table | refused, each with its reason |
| `generate` before any approval | refused |
| `approve` | ledger fact, digest `768f6a97…`; step 5 detail "plan approved by …" |
| `generate` | 5,553 files; the 53 out-of-scope tables' files removed; the lock holds both changes |
| undo the `AD_MESSAGE` change → `generate` | refused: "changed: ADEMPIERE.AD_MESSAGE" |
| redo it → `generate` | builds (the digest is the approved one again) |
| a column added to `AD_MESSAGE` in Oracle → `generate`; dropped → `generate` | refused naming `AD_MESSAGE`; builds |

**Phase 3 — the console.** The Extraction page passes EKOS's checker; in a browser (`static:write` session, Playwright):
the approved plan with the step 3 subjects and their sign-offs; a table click shows its evidence and columns; "Load in
full" on `C_BPARTNER` with a typed reason runs as the signed-in person (the change records `static:write`, whatever the
form said); the plan shows "changed since the approval" and the reason; "Approve plan" waits for the acknowledgement,
then records the approval and the plan is approved again — 11 of 11 checks. The bar shows step 5's detail but the
state `blocked`: steps 4–7 stay blocked while any step 3 subject is unsigned (conflicts, identity), as RFC 0213 says.

**Not done:** step 6 (silver from step 3's labels, retained purges flagged, the SCD2 close, a Dagster run); a found
delete as a review conflict; a change on many tables at once; the page lists all 849 tables in one table (no filter).

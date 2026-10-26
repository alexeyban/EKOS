# RFC 0215 — EKOS Lakehouse: requirements to mart metadata (step 4)

**Status:** Accepted
**Date:** 2026-10-26
**Plan:** Oracle/iDempiere plan v3, Part II.4 (step 4)
**Repos:** ekos-lakehouse (`gold`, `lakehouse-ext`); EKOS (`Task::Requirements`, docs)
**Depends on:** RFC 0195 (gold marts from descriptions), RFC 0197 (LLM task routing), RFC 0213 (sign-off),
RFC 0226 (step 5 approval), RFC 0227 (silver from the source model)

## 1. Problem

RFC 0195 turns a mart description written as a YAML header (grain, dimensions, measures, filters) into a tested dbt
model, and asks back for every term it cannot resolve. Step 4 of the operator workflow asks for more:

- an operator may describe a mart in **plain language** ("monthly sales by product category and region for
  completed orders") rather than in the header format;
- the description is reviewed and **approved as a spec** before anything is built, and the spec says which silver
  models the mart needs;
- it is done from the console.

On iDempiere the resolver also meets things Pagila and LedgerSMB never showed it: Oracle names are upper case while
people write `c_order.grandtotal`, and every table points at `AD_Client` and `AD_Org`, so the key graph is dense.

The LLM appears here and nowhere else in the workflow (plan v3): it drafts the header from text. It never decides what
a term means: everything it drafts is resolved by the same deterministic code as a hand-written header, and what does
not resolve is a question for the operator, not for the model.

## 2. Design

### 2.1 A requirement, from text or from a header

`lakehouse requirements draft --mart <name> (--text <request> | --file <description.md>)`:

- `--file`: a description in RFC 0195's format, used as written.
- `--text`: the configured model for the new task `requirements` (RFC 0197 routing; local Ollama by default, a
  metered route only with `--allow-metered`, as for `pentaho translate`) drafts the header. It sees the request and a
  **catalogue excerpt**: the silver tables the request's words point at (§2.3), with their columns, types and
  comments. Its answer must parse as a header (`gold::parse`); an unparseable answer goes back to it with the parse
  error, up to three attempts. The request becomes the description's prose; the model, its answer and the attempts
  are kept with the draft.

The draft is saved in the state directory (`<state>/lakehouse/requirements/<mart>.md` with `<mart>.json` beside it),
never in the pipeline repository: a draft is not a decision.

### 2.2 Resolution and questions

Every draft is resolved against the deployed plan (the lock: step 3's scope and the operator's step 5 changes already
applied) by RFC 0195's resolver, with two changes:

- **Names are matched without regard to case when the match is unique.** `c_order.grandtotal` resolves to
  `C_ORDER.GRANDTOTAL`. Two candidates that differ only in case are a question.
- **Join paths are searched to a depth of four.** Paths longer than four hops would mean something no request said,
  and the search over all simple paths in a dense graph does not finish otherwise. The search walks only tables that
  can still reach the target in the hops left.
- **`via` chooses a path.** More than one path to a table is still a question (RFC 0195), and RFC 0195 offered no way
  to answer it but to give up the attribute (name the key column itself). On iDempiere it is the common case: an order
  reaches its business partner as customer, bill-to, drop-ship and pay-from. A term may end in
  `via table.column[, table.column]`: the foreign keys its tables' join paths must pass through
  (`customer: c_bpartner.name via c_order.c_bpartner_id`). A `via` column that is not a single-column foreign key is
  a question. The question names, per path, its last key the other paths do not share.
- **One table, one path.** Two terms that would join one table along two paths (customer and bill-to partner) are a
  question: a mart reads each table once, under its own name.

A table the step 3 review put out of scope is not in the lock. A term that names one is a question that says so.

The model is never asked to answer a question. The operator answers by changing the term:
`lakehouse requirements edit --mart <name> --term <name> --value <definition>` replaces one dimension or measure, or
adds one when it does not exist (`--section dimensions|measures`, default from the existing term). `--filter <text>`
adds a filter, and `--drop-filter <text>` removes one. Each edit resolves the draft again.

### 2.3 The catalogue excerpt

Pure and deterministic. The request is split into words. Each silver table is scored by the words found in its name
(split on `_`, 3 points), its comment (2) and its columns' names (1). Plural endings are ignored, and words of fewer
than three letters and a short stop list do not count. A table the three best reference by a key gets one point more.
The best eight are kept. Each lists its columns with their silver type (flags are booleans, RFC 0227) and code lists
of up to twenty values, and its keys to the other tables shown. The excerpt is the only schema the model sees.

### 2.4 Approval

`lakehouse requirements approve --mart <name> [--note]` (class *review*):

- refuses a draft with any question;
- writes the description to the pipeline repository as `marts/<mart>.md` and commits it (RFC 0195 drafts gold only
  from a committed description);
- records a `LakehouseApproval` fact with the id `mart:<mart>`: who, when, the note, the description's SHA-256 and the
  silver models the mart needs.

An approval is valid while the committed description still has that digest and every term still resolves against
the lock. `lakehouse requirements show` reports `approved`, `stale` (the description or the plan moved), `questions`
or `resolved` (ready to approve) per mart.

With `require-plan-approval = true` (RFC 0226), `lakehouse gold draft` builds only a description whose approval is
valid.

### 2.5 What the marts need

Each mart's needed silver models are the tables its compiled SQL reads (the base and every joined table).
`requirements show` lists them per mart and in total. That is the information step 5 needs to narrow the extraction.
Narrowing is left to the operator through RFC 0226's changes; this RFC does not take tables out of the plan by itself.

### 2.6 The console and the workflow bar

The Lakehouse extension gets a **Requirements** page:

- the marts with their state;
- for the selected mart, the description, the term table (term → what it resolved to → how), the open questions with
  their candidates, and the silver models it needs;
- actions: **Draft from text** (inputs: mart name, request), **Change a term** (input: term, definition),
  **Add a filter**, and **Approve** (confirmation).

Step 4 on the workflow bar is computed from this state: *todo* with no requirement, *partial* while one has a
question
or is not approved, *done* when every requirement is approved and valid. Steps 4–7 keep RFC 0213's sign-off gate:
`requirements draft`, `edit` and `approve` refuse without a valid step 3 sign-off when `require-signoff` is set.

## 3. Not in this RFC

Building and reconciling the marts (step 7, plan II.7: its own RFC), narrowing extraction automatically, intake from
Confluence or tickets, multi-mart requests in one text, and business names from the application model's dictionary
(`AD_Element` print names): the excerpt reads table and column comments only.

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | resolver changes, excerpt, `requirements draft --file`, `edit`, `show`, `approve`, gate in `gold draft` | unit tests; a hand-written iDempiere spec over `C_ORDER` resolves, is approved, and `gold draft` builds it on silver |
| 2 | `--text` with `Task::Requirements` | a plain request drafted by local `llama3`: the draft and its questions recorded as they came out, answered by edits, approved |
| 3 | console page, step 4 on the bar | the page in a browser: draft, change, approve |

## 5. Results (2026-10-26, iDempiere testbed)

**Phase 1.** A hand-written requirement over `C_ORDER` (completed sales orders per month and customer, with
`customer: c_bpartner.name via c_order.c_bpartner_id`) resolved at once: 6 terms, 0 questions, reading `C_ORDER`
and `C_BPARTNER`. Before approval, `gold draft` refused it; after `requirements approve` committed it, `gold draft`
proposed the mart (16 atoms checked, nothing invented, planned by Trino) and its SQL returned rows on silver. The
silver flag from RFC 0227 is a boolean comparison (`"ISSOTRX" = true`).

**Phase 2.** The plan's own example, "monthly sales by product category and region for completed orders, last two
years", drafted by local `llama3`:

| Run | What happened |
|---|---|
| first | the excerpt missed `C_REGION` (tables matching "product category" twice crowded it out); the model copied the format's `TABLE.` placeholder, wrote SQL (`DATE_SUB`), and its first answer had no `---` lines |
| after fixes | one attempt, `C_REGION` in the excerpt; 8 terms, 3 questions: a column `total` that does not exist, `isprocessed` (none; `PROCESSED` offered), a date relative to today |

The draft also contained two mistakes no check can see: the region dimension was named `store` (taken from the
prompt's example), and a filter `issotrx is false` (purchase orders, not sales). Both resolve, so they are shown for
the operator, which is why approval is a person's.

Answered by nine edits, none by the model. One was choosing the measure on order lines (`sum of
c_orderline.linenetamt`). Then the joins asked three questions in turn: `C_ORDER` from the line (3 paths), `C_REGION`
(36 paths) and `M_PRODUCT_CATEGORY` (19 paths). Each was answered with `via` (two keys for region and category). The
requirement then resolved over seven silver tables, was approved, and `gold draft` proposed it (30 atoms checked,
nothing invented). On iDempiere's demo data it returns two rows.

**Phase 3.** The Requirements page in a browser, 14/14 then 10/10 on reruns: states, terms with their paths, a change
that makes an approved requirement "changed" and its undo that makes it "approved" again, a draft from text (local
`llama3`), its three questions answered on the page and approved there, step 4's detail "3 of 3 requirement(s)
approved". The bar shows step 4 as blocked on this testbed, because step 3 is only partly signed off.

Found and fixed while building it:

- **Candidates for a missing column listed all 92 of them.** They are now the names like it (`total` →
  `GRANDTOTAL`); none when nothing is close.
- **A model lists terms without names** (`- count of c_order`). From a model's answer, each gets a name made from its
  definition; parse errors now name the section and give an example.
- **`= true`** reads as `is true`: silver flags are booleans.
- **`requirements edit` is class *review*.** As *write*, the console did not set the signed-in person, so edits were
  logged under the console's `$USER`. A change is a person's decision, as in step 5.
- `--drop-term` and the page's **Remove a term**: a model's misnamed dimension cannot otherwise be replaced.

Not done: relative dates ("last two years") cannot be stated in the header, so they come back as a question for the
operator to drop or replace. A draft from text records the console's `$USER`, not the signed-in person, because
`draft` is class *external*.

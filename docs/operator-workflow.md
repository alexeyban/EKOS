# The operator workflow: from a legacy database to approved marts

This guide describes the seven-step workflow that takes an application's code, documentation and database through
EKOS's analysis into a lakehouse (bronze, silver and gold), as built so far. It was built and measured on
[iDempiere](https://github.com/idempiere/idempiere) running on Oracle, and every number below comes from that
testbed. Steps 1–3 belong to EKOS core; steps 4–7 to the [EKOS Lakehouse](https://github.com/alexeyban/ekos-lakehouse)
extension, whose binary (`ekos-lakehouse`) is EKOS plus the `lakehouse` commands.

Deploying the console, tenants, roles and credentials is covered by [deployment.md](deployment.md). The design
decisions are in the RFCs named in each section (`ekos/docs/rfcs/`); what happened while building each one is in
`devlogs/devlog_286.md`–`devlog_304.md`.

---

## The seven steps at a glance

| Step | The operator… | What runs | Model | State |
|---|---|---|---|---|
| 1 Analyse sources | adds code, docs and databases, clicks **Init & compile** | connectors and static analyzers compile everything into the ledger | none | built |
| 2 Documentation | generates the estate's documentation | `ekos docs generate --layout estate` | none | generator built; publishing not done |
| 3 Review and sign-off | settles scope, PII, conflicts, identity, write behaviour and glossary, then signs each subject off | decisions and sign-offs become ledger facts bound to a digest | none | built |
| 4 Requirements | describes the marts wanted, in plain words or as metadata | a model drafts the metadata; EKOS resolves every term | **yes, only here** | built |
| 5 Extraction and landing | reviews how each table lands in bronze, changes it, approves the plan | the planner, from write behaviour and step 3 | none | built |
| 6 Bronze and silver | generates the pipeline and runs it | SQL, dbt-trino silver, Dagster, reconciliation | none | built; run on a 165-table subset |
| 7 Marts | proposes, merges, builds and reconciles each mart | dbt gold from approved requirements, Dagster, Trino | none¹ | built |

¹ RFC 0195's gold drafter can ask a local model for a measure written as prose; such a draft is checked by the
anti-invention test and the engine before it is used.

The **step bar** at the top of every project page in the console (and `ekos workflow status`) shows each step as
*todo*, *partial*, *done* or *blocked*. Steps 4–7 are *blocked* until every step-3 subject has a valid sign-off. They
still show their own progress after the reason, for example "needs a valid sign-off of every review subject (step 3); 3
of 3 requirement(s) approved". Agents read the same state through the MCP tool `ekos_signoff_status`. No MCP tool can
make a decision, sign off or approve: those are human-only, and a test enforces it.

Steps 1–3 make no model call at all: every pipeline command ends its run log with `llm count 0` (RFC 0220).

---

## Before you start

You need:

- the application's source tree (a git checkout), and its documentation if you have it;
- a **read-only** database user. For Oracle, that is `CREATE SESSION`, `SELECT` on the application schema and
  `SELECT_CATALOG_ROLE`. EKOS reads `DBA_*` views inside `SET TRANSACTION READ ONLY` and reports a user that could
  write;
- Oracle Instant Client on `LD_LIBRARY_PATH` (and `libaio.so.1`) wherever the binary reads Oracle, with a build that
  has the `oracle` feature;
- for steps 5–7, the lakehouse stack: Trino with a catalog over the source database and an Iceberg catalog
  (Polaris), object storage, and Dagster. `ekos-lakehouse lakehouse install` sets up a sandbox stack.

The testbed used throughout this guide is `docker-compose.oracle.yml` with `deploy/fixtures/idempiere-oracle.sh`
(Oracle XE 21c, the iDempiere 12 seed, the read-only user `ekos_ro`).

### A project's configuration

A project lives in the EKOS home (`ekos project add`, RFC 0178); its `ekos.toml` names the inputs. This is the
iDempiere testbed's, cut to what this guide uses:

```toml
[workspace]
root = "/path/to/idempiere"

[recover.sql]
default-dialect = "oracle"
[[recover.sql.dialect-rules]]          # each engine's copy of a script is that engine's SQL
path-glob = "**/oracle/**"
dialect = "oracle"
[[recover.sql.dialect-rules]]
path-glob = "**/postgresql/**"
dialect = "postgres"

[tickets]
keys = ["IDEMPIERE"]                   # exact tracker keys that join commits, migrations, code and docs

[[sources]]
name = "idempiere"
kind = "git"
path = "/path/to/idempiere"

[[sources]]
name = "docs"
kind = "git"
path = "/path/to/idempiere-docs"

[[databases]]
name = "idempiere-db"
kind = "oracle"
connect = "localhost:51521/XEPDB1"
user = "ekos_ro"
password-env = "EKOS_ORACLE_PASSWORD"  # the variable's NAME; never the password
owners = ["ADEMPIERE"]
profile = "p2"                         # p0 statistics, p1 exact counts, p2 one aggregate scan per table
dictionary = "idempiere"               # also read AD_Table's flags (who may delete what)

[docs-estate]
licences = { "idempiere-db" = "GPL-2.0" }

[extensions.lakehouse]
source-engine = "oracle"
source-database = "idempiere-db"       # reuses the [[databases]] entry
source-catalog = "idempiere"           # the Trino catalog over Oracle
namespace-prefix = "bronze_idem_"
application-watermark = "UPDATED"      # step 5: incremental where the code keeps it
trino-url = "http://127.0.0.1:58080"
dagster-url = "http://127.0.0.1:53100"
require-signoff = true                 # steps 4–7 refuse to write without step 3 signed off
require-plan-approval = true           # generate builds only the approved plan; gold only approved requirements

[llm.tasks.requirements]               # step 4's drafting model; local, so never metered
provider = "ollama"
model = "llama3"
context-window = 8192
```

`ekos init --detect` writes most of the top half from what it finds in the tree, including one dialect rule per
engine-named directory.

---

## Step 1 — Analyse sources

**Console:** the **Sources** tab adds directories and databases, keeps a database password in the server's keychain
(never in `ekos.toml`, never available to an API token), tests the connection and shows the database's census next to
what was compiled. **Init & compile** runs the pipeline. **CLI:**

```bash
ekos database add idempiere-db --connect localhost:51521/XEPDB1 --user ekos_ro \
    --password-env EKOS_ORACLE_PASSWORD --owner ADEMPIERE --profile p2 --dictionary idempiere
ekos database test idempiere-db      # read-only connection, writes refused, census vs ledger
ekos build && ekos recover && ekos resolve && ekos compile && ekos commit
```

What the analysis recovers, all deterministically:

| From | Into the ledger | RFC |
|---|---|---|
| Java source | types and methods; the application model (`ModelTable`) from iDempiere's generated `I_`/`X_` classes: columns, business names, list values, mandatory columns, code-declared foreign keys; SQL built in method bodies, parsed and linked to tables | 0209 |
| Oracle SQL and PL/SQL | tables, views, routines and triggers; migration scripts as dated change sets with classified changes, linked to their tables | 0210 |
| the live database | the catalog as `Table`/`View`/`Procedure` objects with `origin: live`, reconciled against `*_OBJECTS`; profiles that never read a value out (counts, null counts, number and date bounds, text lengths) | 0211 |
| code vs database | conflicts where they disagree (RFC 0172), each with an explanation when the tree has one (a pending migration, a translation table) | 0211 |
| code, dictionary and keys | one `WriteBehaviour` per table: whether updates always move `Updated`, and every way rows disappear (code, users, cascades, purges), each cited | 0212 |
| git history | one `Ticket` per tracker id, linked to commits, files, change sets, code and doc sections | 0219 |
| documentation | Markdown front matter as properties; sections linked to the tickets they name | 0221 |

Every command that changes something writes a JSON-lines **run log** (`<state>/runs/`, or the console's run when
started there), one line per step with its counts, ending with the number of model calls: the pipeline commands
since RFC 0220, every command classed write, review, admin or external since RFC 0218 — a sign-off, an extraction
change, a mart build. The console shows it as a steps table on the run's page.

**On iDempiere** (`release-13`, 4,542 Java files, the Oracle seed): 763 `ModelTable`s with 16,660 columns and 2,882
code-declared keys; migration scripts with 9 of 62,251 statements rejected; 61 of 61 routines recovered; 2,154 change
sets with 26,060 classified changes; census = compiled for tables, views, routines and triggers; 903 tables profiled
in 6 s; 444 code ↔ database conflicts, 239 explained by the tree itself; 144 tables updated without moving `Updated`
and 754 deletable by users; 4,597 tickets linked to 3,074 change sets; 1,743 doc sections linked to tickets. A
25-question eval (`evals/idempiere/`, RFC 0222) answered 14–15 of 25 with no hallucination.

---

## Step 2 — Documentation

```bash
ekos docs generate --layout estate --output wiki/
```

writes the database estate as wiki pages (1,060 for iDempiere, in 26 s), in the page format the lakehouse-docs
publishers already read (GitHub wiki, Confluence, Azure DevOps). Two gates are built into the renderer: no data value
ever reaches a page (a column profile gives counts only), and text is used only from sources whose licence is known
(`[docs-estate] licences`). A re-run keeps manual sections and deletes only pages it generated (RFC 0214).

Each run records where it wrote and how many pages in `<state>/docs/estate.json`; the bar shows step 2 *partial*
with that line ("1060 page(s) generated … into …") until the pages are published. From the console, run it on the
**Commands** page (`docs generate`, layout `estate`, an output folder inside the project).

**Not done:** publishing from the console. A dry run against this repository's GitHub wiki produced 1,061 files with
no broken links; the real publish waits for the maintainer.

---

## Step 3 — Review and sign-off

The source model is reviewed as six **subjects**: *scope* (which tables the lakehouse takes), *PII*, *conflicts*,
*identity* (candidate matches), *write behaviour* and *glossary*. Each item shows the facts behind it. A person
decides items, then **signs the subject off on the digest of what was shown**. A sign-off's validity is recomputed on
every read, never stored: when a fact under it changes (a column added in the database, say), that subject's
sign-off becomes stale and names what moved, while the others stay valid (RFC 0213).

**Console:** the **Review** tab lists each subject's items, records decisions for the selected ones, and signs a
subject off after its name is typed. **CLI:**

```bash
ekos review list                                  # subjects with their counts and sign-off states
ekos review show scope                            # items, facts, decisions, the digest to sign
ekos review scope AD_ERROR AD_PINSTANCE_LOG --out --reason "runtime logs, not business data"
ekos review pii AD_USER.EMAIL --confirm --mask hash --reason "personal email"
ekos review write-behaviour C_ORDER --set update_class=raw_sql_sets_updated --reason "verified in code review"
ekos review identity "C_ORDER = ORDERS" --confirm      # by id or `A = B` label
ekos signoff show scope && ekos signoff approve scope --digest <digest> --note "reviewed with the app team"
ekos workflow status                              # the step bar
```

The decisions flow on: out-of-scope tables are never planned (step 5), confirmed PII columns are masked in silver
(`hash` → HMAC, `redact` → NULL), and write-behaviour overrides change a table's load mode and delete plan. Only
subjects whose sign-off is **valid** are applied.

**On iDempiere:** the review computes in 1.8 s (cached). Identity candidates must agree word for word, which took the
identity review from 25 candidates to the 1 genuine one (RFC 0224). A full sign-off still needs a person to settle 195
conflicts; on the testbed, 4 of 6 subjects are signed, which is why steps 4–7 show as blocked there.

---

## Step 4 — Requirements

An operator describes each mart wanted. The model drafts its metadata, EKOS decides what every term means, and the
operator approves the result (RFC 0215). This is the only step that uses a model to understand anything, and it
never decides: whatever it drafts is resolved by deterministic code, and what does not resolve is a question for the
operator, never sent back to the model.

**Console:** the **Requirements** page. **Draft from text** takes a mart name and a request in plain words. The page
lists each requirement with its state (*questions*, *resolved*, *approved*, *changed* since its approval, or *stale*).
For the selected one it shows the description, each term with what it resolved to and how, the open questions with
their candidates, and the silver tables it reads. **Change a term**, **Add a dimension**, **Add a measure**, **Remove
a term**, **Add a filter** and **Remove a filter** answer questions; **Approve** asks for a confirmation. **CLI:**

```bash
ekos-lakehouse lakehouse requirements draft --mart category_region_sales \
    --text "monthly sales by product category and region for completed orders"
ekos-lakehouse lakehouse requirements draft --mart sales_by_month --file sales.md   # metadata written by hand
ekos-lakehouse lakehouse requirements show --mart category_region_sales
ekos-lakehouse lakehouse requirements edit --mart category_region_sales \
    --term region --value "c_region.name via c_orderline.c_bpartner_location_id, c_location.c_region_id"
ekos-lakehouse lakehouse requirements edit --mart category_region_sales --filter "c_order.issotrx is true"
ekos-lakehouse lakehouse requirements approve --mart category_region_sales --note "agreed with finance"
```

### The metadata

A requirement is RFC 0195's description: a YAML header, then the request as prose.

```yaml
---
mart: sales_by_month
grain: [month, customer]
dimensions:
  month: month of c_order.dateordered
  customer: c_bpartner.name via c_order.c_bpartner_id
measures:
  total: sum of c_order.grandtotal
  orders: count of c_order
filters:
  - c_order.issotrx is true
  - c_order.docstatus = 'CO'
---

Completed sales orders per month and customer.
```

| Section | Forms |
|---|---|
| dimension | `table.column`; `day\|week\|month\|quarter\|year of table.column` |
| measure | `sum\|count\|average\|min\|max of table.column`; `count distinct table.column`; `count of table` |
| filter | `table.column is true\|false\|null\|not null`; `table.column = \| <> \| < \| <= \| > \| >= <number or 'code'>` (`= true` reads as `is true`) |
| any term | `… via table.column[, table.column]`: the foreign keys its join path must pass through |

Anything else is prose: RFC 0195's drafter may write it as SQL at build time, under the anti-invention check, if it
names columns of the tables the mart reads; otherwise it is a question.

### How a term is resolved

- Names match a silver table and column of the **deployed plan** (step 3's scope and step 5's changes applied),
  regardless of case when that is unique. A table out of scope does not exist here.
- Tables are joined from the measures' table along declared single-column foreign keys, child to parent only, so a
  join never repeats rows. A path is at most **four keys** long. When there are several paths to a table, that is a
  question listing, per path, the key that distinguishes it; answer it with `via`. One table is joined along one path
  only, so two terms that need two paths to one table are a question.
- A missing column is asked with the columns named like it (`total` → `GRANDTOTAL`).
- The draft from text sees only an excerpt of the plan: the eight tables the request's words point at (each word gets
  its best-named table first, then by score), with their columns, silver types (flags are booleans), code lists and
  keys. A model answer that does not parse is retried up to three times with the reason; terms listed without names
  are named after their definitions.

### Approval and what it unlocks

`approve` refuses a requirement with any question. It commits the description to the pipeline repository as
`marts/<mart>.md`, records a `LakehouseApproval` fact (`mart:<name>`: who, when, the note, the description's digest,
the silver tables it reads), and keeps the approval in the state directory. A later edit makes the requirement
*changed*; undoing the edit makes it *approved* again. With `require-plan-approval`, `lakehouse gold draft` builds only
a description whose approval is valid. The silver tables every requirement reads are listed on the page, which is the
information step 5 needs to narrow the extraction. The narrowing itself stays the operator's change.

The `requirements` task is routed by `[llm.tasks.requirements]` (RFC 0197). A metered route is refused unless
`--allow-metered` is given, and in the console unless the tenant allows metered calls.

**On iDempiere:**
- **Hand-written requirement:** resolved at once, was approved, and gold was proposed from it (16 atoms checked,
  nothing invented).
- **The plan's example request,** drafted by local `llama3`: three questions in the draft. One column did not exist,
  another (`isprocessed`) had a near match offered, and one date was relative to today.
- **Mistakes only a person could see:** the region dimension was called `store`, and a filter selected purchase
  orders instead of sales. Both resolve, so no check can catch them.
- **Answered in nine edits,** three of them `via` choices among 3, 36 and 19 key paths. The requirement was then
  approved, and its gold mart proposed with 30 atoms checked and nothing invented.
- **Browser:** 14/14.

---

## Step 5 — Extraction and landing

The planner proposes, per table, how it lands in bronze. EKOS's facts are the evidence, step 3's decisions are
applied, and the operator approves (RFC 0225, 0226).

- **Load mode.** With `application-watermark = "UPDATED"`, a table whose code always moves `Updated` (RFC 0212's
  update class `po_only` or `raw_sql_sets_updated`) loads incrementally on it; one with a raw `UPDATE` that leaves it
  alone (`raw_sql_skips_updated`) loads in full. Tables smaller
  than `incremental-min-rows` stay full.
- **Deletes without a marker.** Each incremental table gets a delete plan from its delete classes: checked every run,
  or audited every `delete-audit-days` where the code never hard-deletes. Deletes are found by a **bucketed key
  diff**: 256 key buckets compared on the source and on bronze, keys read only in buckets that differ. A
  **mass-delete guard** applies nothing when more than `mass-delete-percent` (5 %) or `mass-delete-rows` (10,000) keys
  vanish at once; `lakehouse deletes --accept` applies it when a person knows why. Purged rows can be kept
  (`--keep-purged`) and are flagged in silver.
- **Types.** Each column's Oracle → Iceberg mapping with its lossiness; a column that cannot be landed exactly is
  refused, and the table with it.
- **Masks** from step 3's PII decisions; **scope** from step 3.

**Console:** the **Extraction** page: the plan's summary, the step 3 decisions used, every table with its mode,
deletes and masks; a table's evidence and columns; changes with a reason (**Load in full**, **Load incrementally**,
**Look for deletes every run**, **Audit deletes**, **Keep purged rows**, **Remove purged rows**, **Undo changes**);
**Approve plan**. **CLI:**

```bash
ekos-lakehouse lakehouse plan                     # reads the source, saves the proposal
ekos-lakehouse lakehouse extraction show --table ADEMPIERE.C_BPARTNER
ekos-lakehouse lakehouse extraction change --table C_BPARTNER --full --reason "order import is being reworked"
ekos-lakehouse lakehouse extraction approve --note "first load"
```

The approval is a ledger fact with the plan's digest. The digest leaves the evidence text out, so changing row
estimates don't make an approval outdated. With `require-plan-approval`, `generate` builds only the approved plan;
when the source, step 3 or a change has moved since, it refuses and names the tables. `sync` (source schema changes,
RFC 0194) is not gated by the approval.

**On iDempiere:** 849 tables planned, 1 refused (a column that cannot land exactly), 53 out of scope,
1 column masked; 715 incremental (665 with deletes every run, 50 audited), 134 full. Live: a single delete, a delete
and re-insert, a 148-key mass delete stopped by the guard and applied once accepted, and a retained purge, each
reconciled green.

---

## Step 6 — Bronze and silver

```bash
ekos-lakehouse lakehouse generate                 # the pipeline repository: bronze SQL, dbt silver, Dagster
ekos-lakehouse lakehouse run --in-process --wait  # launch the Dagster job and follow it (the console's Run the pipeline)
ekos-lakehouse lakehouse reconcile --table ADEMPIERE.C_ORDER
ekos-lakehouse lakehouse status
```

**Bronze** is written through Trino into Iceberg. Every load is **reconciled** with the source at V1–V3 (counts,
column aggregates, bucketed row digests) with a planted control. Oracle cannot build a row string past 4,000 bytes,
so its rows are hashed over column digests (RFC 0216).

**Silver** reads what the source and EKOS know about each column (RFC 0227):

| The column is… | Evidence | Silver |
|---|---|---|
| a `Y`/`N` flag | a check constraint `c IN ('Y','N')`, or the application model's boolean accessor on a one-character column | `boolean`; a flag only the code declares is also tested on bronze, so no other value becomes NULL unseen |
| a list of codes | a check constraint | `accepted_values`, fails the build |
| a list of codes | the application model | `accepted_values`, **warns**: the dictionary can add values |
| a logical foreign key | the application model, with no declared key | `relationships`, **warns** |

Rows the source purges but the plan keeps carry `_ekos_purged_at_source`; a `history` snapshot closes a row deleted at
the source (`hard_deletes: invalidate`), at the time the delete was seen. `flags-as-text = true` keeps flags as text.

What the generator does for you:

- **A reload when a load expression changes.** An incremental load never rewrites an unchanged row, so `generate`
  requests a reload for every incremental table whose column expression changed since the deployed lock.
- **Oracle `CHAR(n)` padding.** It is kept (Trino's cast drops trailing spaces).
- **Safe names.** Oracle names with `$`, such as `CREATE$JAVA$LOB$TABLE`, get safe names in Iceberg, Dagster and dbt.
- **The lock.** `lakehouse.lock.json` is always rewritten with the SQL beside it.

**On iDempiere:**
- **The full 849-table run was dropped:** it needs more memory than the testbed machine has.
- **The subset instead:** 165 tables, closed over their declared and code-declared references, ran in one Dagster
  process. Every bronze table reconciled with Oracle, and 165 silver models were built.
- **Tests:** 3,664 of 3,667 passed, with 3 warnings that are real data outside the application's lists.
- **What the facts amount to:** of 2,376 code-declared references, 2,367 already have a declared key. All 1,776
  check lists are `Y`/`N` flags.

---

## Step 7 — Marts

Each approved requirement becomes a gold table: proposed as a dbt model, merged by a person, built through Dagster,
and reconciled with silver (RFC 0195, 0228).

**Console:** the **Marts** page lists every mart with its state. The selected mart shows the approved requirement
beside the generated SQL, each term as `exact` (compiled from a pattern) or `drafted` (written by a model; read it),
the tests, each measure's mart and silver totals, and the lineage graph. Actions: **Propose**, **Merge** (a
confirmation), **Build** and **Reconcile**. **CLI:**

```bash
ekos-lakehouse lakehouse marts show --mart category_region_sales
ekos-lakehouse lakehouse marts propose --mart category_region_sales     # a branch ekos/gold/<mart>/<id>
ekos-lakehouse lakehouse marts merge --mart category_region_sales --note "agreed with finance"
ekos-lakehouse lakehouse marts build --mart category_region_sales --wait
ekos-lakehouse lakehouse marts reconcile --mart category_region_sales
```

| State | Meaning |
|---|---|
| `requirement` | the requirement is not approved, or no longer valid |
| `to propose` | approved; no proposal carries it |
| `proposed` | a branch carries the approved description |
| `merged` | the model is on the pipeline's branch; not built since |
| `failed` | the latest build failed (its message names the failed tests) |
| `built` | the latest build passed; not reconciled since |
| `reconciled` / `diverged` | every measure's totals equal, or one differs |

- **Propose** runs the gold drafter on the committed requirement. It checks the whole statement against the
  requirement's own atoms (anti-invention), plans it on Trino with `EXPLAIN`, adds a grain-uniqueness test and one
  reconciliation test per additive measure, and opens a branch with `proposals/gold-<mart>.md`.
- **Merge** is the approval of the generated model. It refuses a branch whose description is not the approved one,
  and leaves nothing half-merged. It records a `LakehouseApproval` fact (`gold:<mart>`).
- **Build** launches the Dagster job with only the mart's asset. When Dagster loaded its code before the merge, Build
  reloads the code location first. From the console it returns once the run is launched; the page shows `built` when
  the run ends.
- **Reconcile** reads, through the read-only Trino reader, each additive measure's total in the mart and the same
  aggregate over silver under the mart's joins and filters, and records both (`mart_reconcile` metrics).

The SQL is a template: a term marked `drafted` needs a person to read it, and a generated mart may need tuning.

**On iDempiere:** four marts went from approved to reconciled, the fourth entirely on the page; every total was
equal (for example 379.90 = 379.90 and 3 = 3 orders for `sales_by_month`). Each build of one mart took about 50 s.
A browser run passed 8/8.

> Starting Dagster for a project turns on its generated schedules, which are on by default: if the hourly tick has
> passed, the whole job starts at once. Decide whether the schedules should run before starting Dagster on a test
> machine.

---

## The demo book

`tools/demo-book` in ekos-lakehouse (RFC 0218) walks the console through the seven steps and writes a book: per step,
the screen, what the operator clicks, the run log behind it, what happens behind it, what they get and the caveats.

```bash
python -m demo_book tour --console URL --workspace ID --out capture/ [--act test-db --act docs --act edit-undo --act reconcile]
python -m demo_book render --capture capture/ --out book/ --pdf book/demo-book.pdf
```

The tour reads; each `--act` runs something that leaves the project as it was (a connection test, the estate docs, a
change and its undo in steps 4 and 5, one mart's reconciliation), never an approval or a build. Screenshots, logs and
numbers are regenerated on every render; the narration lives in manual sections a render keeps; what the capture
lacks is said on the page and fails the render unless `--allow-missing`. The iDempiere book is in the demo
repository, [`book/`](https://github.com/alexeyban/ekos-lakehouse-oracle/tree/main/book).

---

## What is guaranteed

- **Read-only on the source.** Database sessions are read-only transactions with a user EKOS reports if it can
  write; the source tree is never written (a project's state lives in the EKOS home).
- **No secret or data value stored.** Passwords stay in environment variables or the keychain, and only their names
  are in config. Profiles keep counts and bounds of numbers and dates, never text values. Run logs and estate pages
  carry no value. The redaction baseline (RFC 0043) runs at every raw-content entry point.
- **Human-only decisions.** Review decisions, sign-offs, step-5 changes and approvals, and requirement changes and
  approvals, and merging a mart are commands of class *review*. The console runs them as the signed-in person, refuses them to API tokens,
  and no MCP tool reaches them.
- **No metered model call without consent.** Steps 1–3 make none; step 4 refuses a metered route without
  `--allow-metered` (and the tenant's permission in the console).
- **Everything decided is a ledger fact** with its evidence and digest, so a later change says what it invalidated.

---

## Where things are

| What | Where |
|---|---|
| a project's config and state | `<EKOS_HOME>/data/<tenant>/<project>/{ekos.toml,state/}` |
| run logs | `<state>/runs/<stamp>-<command path with dashes>.jsonl` (`…-lakehouse-marts-build.jsonl`) |
| the latest estate documentation | `<state>/docs/estate.json` (where, pages, when) |
| review cache | `<state>/review/state.json` |
| step 5 proposal, changes, approval | `<state>/lakehouse/extraction/{proposed,changes,approved}.json` |
| step 4 drafts and approvals | `<state>/lakehouse/requirements/<mart>.{md,json,approved.json}` |
| step 7 latest build and reconciliation | `<state>/lakehouse/marts/<mart>.{run,reconcile}.json` |
| the pipeline repository | `<project>/pipeline/` (`pipeline-dir`): `lakehouse.lock.json`, `bronze/`, `dbt/`, `dagster/`, `marts/` |
| reload requests | `<pipeline>/.lakehouse/reload/<schema.table>` |
| proposals | branches `ekos/gold/…` and `ekos/change/…` in the pipeline repository (`lakehouse proposals`) |

### Settings of `[extensions.lakehouse]` used by these steps

| Setting | Default | Meaning |
|---|---|---|
| `source-engine` | `postgres` | `oracle` reads the `[[databases]]` entry named by `source-database` |
| `source-catalog`, `target-catalog` | — , `iceberg` | Trino catalogs read from and written to |
| `namespace-prefix` | `bronze_` | bronze schema prefix |
| `application-watermark` | none | the column the application keeps (`UPDATED`) |
| `incremental-min-rows` | 0 | smaller tables stay full |
| `delete-audit-days` | 7 | between audits of tables the code never hard-deletes |
| `mass-delete-percent`, `mass-delete-rows` | 5, 10,000 | the mass-delete guard |
| `flags-as-text` | false | keep `Y`/`N` flags as text in silver |
| `require-signoff` | false | steps 4–7 refuse to write without step 3 signed off |
| `require-plan-approval` | false | `generate` builds only the approved plan; `gold draft` only approved requirements |
| `dagster-url`, `trino-url` | — | the services `run`, `plan` and `reconcile` use |
| `pipeline-dir` | `pipeline` | the pipeline repository, relative to the project |

### RFCs

| Part | RFCs |
|---|---|
| Step 1 | 0209 Java and the application model · 0210 Oracle SQL, PL/SQL, schema history · 0211 live Oracle and conflicts · 0212 write behaviour · 0219 tickets · 0220 run logs · 0221 docs · 0222 eval · 0223 Sources tab · 0224 identity |
| Step 2 | 0214 estate documentation |
| Step 3 | 0213 review, sign-off, the step bar |
| Step 4 | 0215 requirements · 0197 model routing · 0195 gold |
| Step 5 | 0225 extraction plan and deletes · 0226 the Extraction page |
| Step 6 | 0216 Oracle source · 0227 silver from the source model · 0192–0194, 0196 bronze, silver, schema changes, monitoring |
| Step 7 | 0195 gold · 0228 marts: propose, merge, build, reconcile |
| All | 0218 run logs for every command that changes something; the generated demo book |

---

## Not done

- Step 2's publish to a wiki (waits for the maintainer).
- A complete step-3 sign-off on iDempiere (195 conflicts for a person to settle).
- A relative date in a requirement ("last two years"), several marts from one request, and attributing a draft from
  text to the signed-in person.
- In step 7: Build following the run on the page, and marts not described by a requirement.
- Optional CDC with Debezium (RFC 0217).
- Run logs for steps 3 and 6 on iDempiere: the sign-offs and the subset load ran before RFC 0218, so the demo book
  quotes none for them.

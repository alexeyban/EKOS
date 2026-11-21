# RFC 0265 — Talend documentation and the migration assessment

**Status:** Accepted (2026-11-21)
**Umbrella:** RFC 0262 (Slice 2). **Builds on:** RFC 0263 (model), RFC 0264 (ledger, lineage), RFC 0214 (estate docs).

## Problem

Slice 1 put every Talend job, component, link and column lineage in the ledger. A person who has to move off
Talend still has two questions the ledger does not answer as written:

1. What do these jobs do, page by page, the way a wiki would say it?
2. How big is the move, and what is wrong or odd in the jobs before anything is translated?

The plan (`talend-plugin-plan.md` Parts III and IV) answers both deterministically, from ledger facts, with no
model call.

## Design

### Where the work happens

The analyzer pass already holds each project, expanded, with its lineage. The assessment is a pure function there
(`ekos_talend::assess`). Its results are written to the ledger:
- one keyed `TalendFinding` object per finding;
- per-job and per-project summaries as properties of `TalendJob` and `TalendProject`.

Readers only read the ledger:
- `ekos talend assess` renders the report;
- `ekos docs generate --layout estate` writes a `talend/` section.

Neither reads a source file, so neither is a raw-content entry point. Both are deterministic.

### Findings (`TalendFinding`)

A finding has these fields:
- `rule`: one of the rule names below;
- `class`: `probable_defect`, `copy_paste`, `environment` or `dead_item`;
- `subject`: a job, or `job.component`;
- `message`: the finding in words;
- `evidence`: `path:line` of every component or item it rests on;
- `status: hypothesis`, because a person confirms it.

The finding's key is project + rule + subject, never a line, so a finding keeps its id when unrelated lines move.
It is linked to its job by `About`. A finding is never a defect until a person says so, and the report says that
in words.

| Rule | Class | What it checks |
|---|---|---|
| `measure_ignores_named_column` | probable_defect | A measure (the formula is one aggregate: `count`, `sum`, `avg`, `min`, `max`) whose name contains, word for word, the name of a multi-word column the job reads (`NB_QTE_VENTE` ⊃ `QTE_VENTE`), while the formula never reads that column. |
| `leap_year_by_four` | probable_defect | An expression on a year that tests divisibility by 4 (`/4 == floor(/4)`, `% 4 == 0`) without `100` and `400`. |
| `repeated_message` | copy_paste | The same `tDie` / `tWarn` message literal in several jobs, naming one job (its name or its subject — a word of its name, five letters at least) while some job carrying it is not that job and does not call it. A message shared without that is ordinary logging. |
| `foreign_lookup_name` | copy_paste | A row-link label equal to another job's subject, when this job reads no table or file containing it. |
| `absolute_path` | environment | A file or directory parameter that folds to an absolute path (`C:/…`, `/…`, `\\server`), and it says when the path is under a person's own folders (`Users`, `home`, `Downloads`, `Desktop`, `Documents`). |
| `uncalled_job` | dead_item | A latest job version that no job calls and that calls no job. |
| `unused_item` | dead_item | A context, connection or custom routine that nothing uses. A job's implicit context load uses its repository connection, and a routine is used when it is called as `Routine.m(` or `routines.Routine.m(`. Repository schemas are not covered: no relationship records their use yet. |

**Clone clusters** are not findings. Two jobs are clones when their component graphs are equal after
normalisation: component types in order, row links and triggers as edges between those types, and every name,
table and column removed. The clusters are a project property (`clone_clusters`: the jobs, and the shared shape
as text).

### Per-job summary (`TalendJob.assessment`)

- `components` by IR mapping: mapped or not, with the reason when not.
- `expressions` by kind: `column`, `arithmetic`, `routine`, `java_method`, `ternary`, `runtime`, `literal`.
- `state`: row-level writes (`UPDATE`, `INSERT_OR_UPDATE`, `DELETE`), write-back to a table the job also reads,
  file moves and deletes, sequences (`Numeric.sequence`), and commits.
- `runtime`: context variables overridable at run time, `globalMap` keys read, clock functions
  (`TalendDate.getCurrentDate`), and `tFileList` iteration.
- `idioms`: idioms recognised by structure only, each with the evidence it rests on:
  - `scd2_by_lookup`: a lookup on the target plus outputs that insert, update and insert again;
  - `reject_to_table`;
  - `aggregate_then_insert`.

  Slice 3 turns idioms into dbt constructs.

The plan's "translatability by fidelity class" needs Slice 3's classifier, which does not exist yet. Until it
does, mapping to the IR stands in for it, and the report names it as a stand-in.

### Per-project summary (`TalendProject.assessment`)

- `clone_clusters`;
- `migration_order`: the jobs sorted by the table graph (a job after every job writing a table it reads), clone
  clusters kept together, and cycles reported rather than broken;
- `target_sketch`: counts of the dbt objects the translation would produce (a model per target table, a snapshot
  per `scd2_by_lookup`, a seed per file input that a job reads only once, a macro per clone cluster), labelled as
  a sketch;
- counts by class.

No effort estimate in days: EKOS has no measured basis for one.

### `ekos talend assess`

```
ekos talend assess [--project <name>] [--json] [--out <file>]
```

It reads the ledger (read-only) and prints Markdown, or the same content as JSON. Its catalogue class is `read`.
It makes no model call.

### Documentation (`--layout estate`)

When the ledger holds current `TalendProject` objects, the estate pages gain `talend/`:

| Page | Content |
|---|---|
| `talend/index.md` | the projects, with product version, job and component counts, and engines |
| `talend/<project>/index.md` | overview by folder, the orchestration tree from root jobs (`Calls`, triggers with conditions), jobs nothing calls, coverage (data components mapped, lineages with a gap) and a link to the assessment |
| `talend/<project>/jobs/<job>.md` | the author's description, tables and files read and written (with `DATA_ACTION`), contexts used and whether each value can change at run time, every tMap and ELT map (inputs, join keys, outputs with filters, a column table "output ← expression"), column lineage per target column (formula, origins, gaps), and the job's findings |
| `talend/<project>/contexts.md` | variables by context with their defaults and the jobs that use them. A password variable is listed with no value, as stored |
| `talend/<project>/assessment.md` | what `ekos talend assess` prints |

RFC 0214's licence rule applies. Expressions, SQL, messages and descriptions are source text, shown only when
every source of the object has a known licence; otherwise the text is dropped and counted on the estate home.
Names, counts and structure are always shown.

A workspace without a Talend project gets no `talend/` pages. The estate home's count reads "Talend jobs 0",
which is fixture A's result.

## Exit

- **Fixture B:** the assessment lists every §1.2 finding of the plan, and the six ODS jobs fall in one clone
  cluster. The five findings are:
  - `NB_QTE_VENTE` counts rows;
  - the leap-year test checks divisibility by 4 only;
  - the repeated *Categorie* message;
  - `sousCategorie` lookup labels outside their job;
  - `jGenerateCalendar` is uncalled and reads from a user's Downloads folder.
- **Fixture A:** no `talend/` pages, and "Talend jobs 0".
- **Fixtures C, D, E:** the assessment runs without a diagnostic, and every finding cites a file and line.
- **No false findings:** across all five fixtures, no finding a reviewer marks false. The plan sets the threshold
  after the first run; this RFC records the measured count beside it.
- **0 model calls**, printed.

## Result (2026-11-21, devlog_347)

| Fixture | Findings | Notes |
|---|---|---|
| B | 22 (19 + 3 in the zip's project) | Every §1.2 finding. The six ODS jobs form one clone cluster, the two dimension jobs another |
| C | 81 | absolute paths under a person's Desktop, uncalled demo jobs, unused contexts, connections and routines |
| D | 41 | the same kinds |
| E | 12 | including a real copy: `main_extract_STRATIO`'s error messages name the SAP job's children |
| A | 0 | no `talend/` pages; "Talend jobs 0" |

**False findings.** The first run had 7 that review marked false:
- a longer column name taken for a measure (`SALES_ORDER_TYPE_ID` ⊃ `ORDER_TYPE_ID`);
- the connection implicit context load uses (`PARAMS_LOG`), which the ledger did not record;
- a routine called as `routines.HashUtil.…`, which the ledger did not record;
- four log lines shared by sibling jobs.

The rules above are the corrected ones. Two of the causes were gaps in the ledger, not in the rules: the missing
`UsesConnection` (job → connection, discriminator `implicit_context`) and `UsesRoutine` relationships are now
written. After the corrections, every finding was checked by hand against the items; 0 are false. The threshold
the plan leaves open is therefore set at **0 false findings on the five fixtures**, held by
`crates/recovery/tests/talend_assess.rs`.

## Not in scope

- Publishing to Confluence, Azure DevOps or GitHub wikis. The lakehouse-docs publishers read the estate page
  format unchanged.
- Diagrams beyond Mermaid, and the Studio `.screenshot` attachment.
- The console page.

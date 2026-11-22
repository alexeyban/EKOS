# RFC 0268 — Talend jobs to dbt: layout, idioms, gates, differential run, console

**Status:** Accepted (2026-11-22)
**Umbrella:** RFC 0262 (Slice 3; plan Parts V.4–V.7 and VI). **Builds on:** RFC 0266 (shared IR), RFC 0267
(lowering), RFC 0204 (Pentaho → dbt layout), RFC 0190 (the lakehouse plan).
**Code:** `ekos-lakehouse` — new crates `etl-dbt` (moved from `pentaho-dbt`) and `talend-dbt`; `talend-rel` and
`lakehouse-ext` changed. EKOS: the console tab and `ekos_talend_explain` (phase 2).

## Problem

RFC 0267 turns each Talend job into the shared relational IR. A Talend job is imperative, though: it reads source
tables, writes target tables with a data action, and keeps state in those targets between runs. A dbt project is
queries plus materializations. The step between the two is this RFC's:
- which nodes become which models;
- how a table one job writes becomes a `ref()` in the job that reads it;
- what each data action becomes in dbt;
- what has no dbt form (trigger-run statements, run-time context);
- the gates that keep a wrong model from being shipped as an enabled one.

## Design

### Shared pieces move to `etl-dbt` (RFC 0266's method)

Three pieces of `pentaho-dbt` are not Kettle's:
- `catalog` (`Resolver`, `Resolution`, `TableKey`): a table resolves to a silver model of the lakehouse plan, a gold
  model the package writes, or unknown with the reason;
- `pgsql::to_trino`: PostgreSQL source SQL rewritten for Trino, with each table name resolved;
- `parses_as_trino` and `slug`.

They move to a new crate, `ekos-lakehouse-etl-dbt`, and `pentaho-dbt` re-exports them (`pub use`), so no caller
changes. The move is checked as RFC 0266's was: the golden manifests (bench-gold, Pentaho's samples,
AdventureWorks) must be byte-identical after it.

### Connections on table references

`talend-rel` now fills `TableRef.connection` from the component's database connection:
- `db_type`: the engine, upper-cased (`POSTGRESQL`);
- `database`: the folded `DBNAME`;
- `schema`: the folded `SCHEMA_DB`.

Each is read from the component, or from the `tDBConnection` its `CONNECTION` names. The resolver can then tell a
read of the lakehouse source from a read of another database, as it does for Kettle.

### Layout (`talend-dbt`)

Per project, the jobs whose latest version has data components. Model names are
`talend__<project>__<job>__<component>`. Layers, as RFC 0204 draws them:
- **Silver:** the row-local chain from each source (`Project`, `Filter`, `Sort`, a `Distinct` without a count) is
  one model, materialized `table` in schema `silver_talend`. Its source is resolved against the plan:
  - PostgreSQL SQL on the lakehouse source is rewritten by `to_trino`;
  - an ELT input reads its table through the resolver;
  - anything else is a gap with the reason, and a question asking which lakehouse table holds the data.
- **Gold:** one model per sink: the nodes upstream of the sink, stopping at silver models.
- **Across jobs:** a table one job writes is a `ref()` to that gold model in every job that reads it. The written
  tables are resolved to a fixpoint, as `pentaho-dbt` does.

A model with a gap in its nodes is written **disabled**: `config(enabled=false)`, the reasons as comments, and the
SQL commented out.

### Idioms, phase 1: what a sink's data action becomes

Every rule is named in the model's report entry.

| Talend sink | dbt | Fidelity ceiling | Rule |
|---|---|---|---|
| `INSERT`, the table cleared first (`TABLE_ACTION` clear / truncate / drop-create) | `table` | from the nodes | `reload` |
| `INSERT`, no clearing | `incremental`, `append` | `rewritten` + question: are the input rows only new ones? | `append` |
| `INSERT_OR_UPDATE` / `UPDATE_OR_INSERT` / `UPSERT` / `MERGE` with key columns | `incremental`, `merge`, `unique_key` = the keys | `rewritten` | `merge` |
| `UPDATE` with key columns | `incremental`, `merge`, `unique_key`; in an incremental run, only rows whose key `{{ this }}` holds | `rewritten` + question | `update` |
| a data action without key columns (other than `INSERT`) | disabled | gap | `no-key` |
| `DELETE` | disabled; a question | gap | `delete` |
| a file output | disabled: a file is not a dbt model; listed | — | `file` |

**Statements** (`Op::Statement`, RFC 0267) get no model. Each is listed in the report with its verb, its table and
its SQL. A statement that `UPDATE`s a table no job reads as data is reported as **run bookkeeping**, with a question:
dbt records its own run times, so is anything else reading this table?

**Watermarks:** a source reading a context variable that a `tJavaRow` sets from a row (`ContextFromData`). Its
model is written with the variable's default value and the RFC 0267 note. A question names the variable, the
query that sets it and the predicate that reads it, since dbt's `is_incremental()` over the model's own maximum is
the usual replacement. Phase 1 does not rewrite the predicate.

### Files

| Path | What |
|---|---|
| `dbt/models/silver/talend/<project>/<model>.sql` | silver models |
| `dbt/models/gold/talend/<project>/<model>.sql` | gold models |
| `dbt/models/gold/talend/<project>/schema.yml` | `unique` and `not_null` on each gold model's key columns |
| `talend/<project>/translation.json` | the report: per component (original configuration, SQL, fidelity, rule, notes), per model, questions, statements, watermarks |
| `proposals/talend-<project>.md` | the report for a reviewer |

Every SQL file starts with a header naming the generator, and must not be edited.

### Gates (before a model is written enabled)

1. **Trino parse.** The model's SQL, its `ref()`s and `source()`s replaced, parses as Trino. A model that fails is
   disabled with the parser's message.
2. **Anti-invention.** Every column name in the model's SQL is either a column of a node in the model or of a
   table it reads, or a name the translator made (a CTE, `__g<n>`, `__seq_<s>`). A model that fails is disabled,
   naming the column.
3. **No raw Java.** No model contains `context.`, `globalMap`, `input_row.`, `row<N>.` or `Var.` outside a comment.
4. **`EXPLAIN` on Trino (opt-in):** the extension's command runs it when `trino-url` is set. A model Trino refuses
   is disabled with Trino's message.

### Command

`ekos lakehouse talend translate <project> [--explain]`:
- **Projects:** the Talend projects under the workspace's sources, found by `talend.project` and read through
  RFC 0043 redaction, as the Pentaho command reads packages.
- **Plan:** the deployed lakehouse plan (`lakehouse generate` first).
- **Output:** the files, written into the pipeline repository.

The command's catalogue class is `external`, as `lakehouse pentaho translate`'s is: it plans on Trino and branches the repository. It makes no model call. `lakehouse talend scan` lists the projects (`read`).

## Phases

1. **This delivery:** the `etl-dbt` move, connections on table references, the layout, the phase 1 idioms, files,
   gates 1–3, and the command with gate 4.
2. **Pattern idioms and the console:**
   - two sinks on one table, inserting new keys and updating known ones, become one `merge` model;
   - lookup on current rows plus deactivate and insert-version becomes a dbt **snapshot**;
   - rejects become a `rej_` model;
   - a `tRowGenerator` date spine plus a holiday file becomes a spine model and a seed;
   - `--faithful`;
   - the console's Talend tab (RFC 0198/0207 page seam): projects, a job's graph, fidelity per component,
     original next to SQL, questions, and the `translate` action;
   - `ekos_talend_explain` (EKOS MCP, read-only, scoped).
3. **Verification by execution:**
   - **The differential run** (plan V.6) executes the project's built jobs, which are third-party code. Per the
     plan, that happens only in a sandbox (no network, a throwaway database), never on a developer machine, so it
     waits for a sandbox host. Until then the result says **not verified by execution**.
   - **LLM fill-in for gaps** (plan V.7), with `--allow-metered`, through RFC 0205's loop.

## Phase 2 design: pattern idioms (2026-11-22)

Measured first on the fixtures (lakehouse `talend-dbt` diagnostic over B and E). The disabled models of phase 1 come
from five shapes. Each is recognised by structure, names its rule, and falls back to phase 1 when any condition
fails. A fallback names the condition that failed. A recognised pattern is never `exact`.

| Shape (where) | Recognised when | dbt result | Rule |
|---|---|---|---|
| **SCD2 by hand** (B: ODS and DIM jobs) | one job; a tMap looks up the target's own rows (a `Source` on the table); three sinks on that table: an `INSERT` of the rows the lookup did not match, an `UPDATE` keyed on the lookup key or on a key read from the lookup that sets a flag column to a literal, and an `INSERT` of matched rows that pass a comparison of main and lookup columns; both inserts write the same columns and set the flag to a different literal than the update | a **snapshot** (`strategy='check'`, `unique_key` = the lookup key, `check_cols` = the main-side columns of the comparison) over the first insert's projection with the lookup removed, plus a gold model over it that gives the table's own columns back (`flag` from `dbt_valid_to IS NULL`, the insert's clock column from `dbt_valid_from`, the update's clock column from `dbt_valid_to`) | `snapshot` |
| **Delete then insert on a key** (E: 17 ELT jobs) | one job; a `DELETE` of `B.k` for the rows where `A LEFT JOIN B` matched, `B` a `Source` on the table; an `INSERT` over the same join that reads only `A` and writes `k` | an incremental model, `merge` on `k`, over the insert's projection with the join removed | `keyed-replace` |
| **One rejects table, many writers** (B: `ODS_REJET`, 18 writers in 6 jobs) | ≥ 2 writers, all `INSERT_OR_UPDATE` (or all `INSERT`) with the same keys and columns; none reads the table; every writer's flow carries a reject branch | one `rej_` model: the writers' rows, `UNION ALL`, incremental `merge` on the shared keys (append for inserts) | `rejects` |
| **Write-back flag** (B: `ODS_VENTE.BL_LIGNE_IN_DWH`) | an `UPDATE` of a table the same job reads as a source, every non-key column a literal | no model; a `write-back` question: the consumer's incremental predicate replaces the flag, unless something else reads it. The sink is not counted as a writer of the table. | `write-back` |
| **Lookup on the target** (B: `jOdsVente`; E: nine upserts) | the only writer of a table reads it through a `Source` upstream; the rule is incremental; the source reads only columns the model writes | the source reads `{{ this }}` in an incremental run, and an empty relation of the model's typed columns on the first build (every key is new) | the sink's rule, noted `self-read` |

Conditions and questions:
- **Snapshot divergences are stated, not hidden.**
  - An update keyed on the natural key changes every version under that key. In fixture B it also writes NULL into
    the attribute columns, because field options are off. The snapshot keeps the history, and a `snapshot`
    question names the overwritten columns.
  - A key the update matches on that no insert writes (a `SERIAL`) becomes `dbt_scd_id`. The values change, so a
    `surrogate-key` question is raised.
- **Not recognised:** jOdsClient's third output *inserts* a row that is NULL except for the flag, instead of
  updating. It stays phase 1, with a question that names the output.
- **Keyed replace** holds when every key in the table is the insert's expression of its row (E: `md5` of the natural
  key). That is a stated assumption, raised as a `keyed-replace` question.
- **Rejects:** a merge refuses two rows with one key in a run. Talend's row-by-row upsert keeps the last one. The
  model's notes say so.
- **`--faithful <job>`** (repeatable, or `all`) turns recognition off for those jobs: phase 1, one model per output.

Two lowering changes in `talend-rel` (RFC 0267) are needed by fixture B's calendar:
- **A row count a `tJava` sets.** `tRowGenerator`'s `NB_ROWS` can name a context variable that a `tJava` assigns.
  Phase 1 read its default (0) as an exact count, which is wrong. The count is now:
  - `TalendDate.diffDate(context.E, context.S, "dd") + 1`, computed from the two defaults and `rewritten` with a
    note;
  - any other assignment is a gap naming the `tJava`.
- **What follows the JVM's locale** stays a gap unless the operator states it with `--locale <tag>`, from a closed
  list (`fr_FR`, `de_DE`, `en_GB`, `en_US`, …, `talend-rel`'s `Locale::KNOWN`). Two things depend on it:
  - `getPartOfDate("WEEK_OF_YEAR")`. Under a Monday-first, four-day locale (every listed country but the United
    States) Java's week is the ISO week, so the part lowers to the ISO week, `rewritten`. Under `en_US` it stays
    a gap.
  - `formatDate("MMMM")` and `formatDate("EEEE")`, the month's and the day's name: a `CASE` over the month or the
    weekday, with CLDR's names (as Java 9 and later print them), `rewritten`.

The calendar's holiday file is a `talend_files` source with its `file` question. The translation is pure and never
reads the file, so it cannot write a seed.

## Exit (phase 1)

- **Golden gate:** the Pentaho golden manifests are byte-identical after the `etl-dbt` move.
- **Fixture B:** with a plan built in the test from the tables its jobs read (synthetic, and marked so), every gold
  model without a gap in its nodes is enabled, and every enabled model passes gates 1–3.
- **Coverage on B–E:** models enabled out of written, by layer and rule, and questions by kind, reported in the
  devlog.
- **Fixture E:** its 42 statements are listed, and the 12 watermark sources each carry their question.
- **Pure:** a test checks that `talend-dbt` reads no file.

## Exit (phase 2 idioms)

- **Fixture B:**
  - its seven SCD2 shapes (five ODS jobs, two DIM jobs) are recognised as snapshots, or refused by name where the
    job differs from an SCD2;
  - `ODS_REJET` is one `rej_` model;
  - the write-back update has no model;
  - the calendar's count is computed, and with `--locale fr_FR` its model is enabled.
- **Fixture E:** each delete-then-insert job whose delete and insert read one join is `keyed-replace`; the others
  are refused by name.
- **Each rule** has a synthetic test, and a refusal test for each condition that matters (the NULL-insert "close"
  of jOdsClient).
- **Gates:** no model is disabled by a gate, and `dbt parse` accepts B's project with its snapshots.
- **`--faithful`** gives phase 1's output for the named jobs.

## Result (phase 2 idioms, 2026-11-22, devlog_352; lakehouse devlog_28)

The plan is synthetic, as in phase 1, and the fixtures run with `--locale fr_FR`. Models enabled out of written:

| Fixture | Phase 1 | Phase 2 | Snapshots |
|---|---|---|---|
| B | 10 / 73 | 18 / 44 | 2 / 5 |
| C | 42 / 61 | 42 / 61 | — |
| D | 9 / 23 | 9 / 23 | — |
| E | 17 / 184 | 25 / 164 | — |

What the rules found:
- **Fixture B:**
  - **Snapshots:** five of the seven SCD2 shapes are snapshots: `ODS_TYPE_CLIENT`, `ODS_CATEGORIE`,
    `ODS_PRODUIT`, `DIM_CLIENT` and `DIM_PRODUIT`.
  - **Two refused, both by a difference in the job itself:**
    - jOdsClient's third output *inserts* a row that is NULL except `BL_LIGNE_ACTIVE`, where an SCD2 updates;
    - jOdsSousCategorie inserts a new version of every matched row, changed or not, while it closes only the
      changed ones, so an unchanged key gains a second current row on each run.
  - **Questions raised on the snapshots:**
    - `snapshot`: the closing updates of the ODS jobs match on the business key and write NULL into the
      attribute columns of every version;
    - `surrogate-key`: the DIM jobs' `SERIAL` keys become `dbt_scd_id`.
  - **Rejects:** `ODS_REJET` is one `rej_` model over 18 writers in 6 jobs.
  - **Write-back:** `ODS_VENTE.BL_LIGNE_IN_DWH` has no model.
  - **Calendar:** enabled. Its row count is computed, 2,192 days from the two context defaults.
- **What stays disabled in B is downstream of the two refused jobs.** That covers the client and product
  dimensions' inputs, `ODS_VENTE`, `FAIT_VENTE` and the rejects model, which reads `ODS_VENTE`. A reviewer's
  answer to the two questions decides them. `--faithful` gives phase 1's shape for any job.
- **Fixture E:** 10 jobs are keyed replaces. Three are enabled; the rest read Snowflake or Oracle tables outside
  the lakehouse. Ten are refused by name: nine join the target in the middle of a chain of joins, and one (`DIM_SKU`) does not delete the rows its join matched.
- **Checks:**
  - no model on B–E is disabled by a gate, the new scope gate included;
  - `dbt parse` (dbt 1.11, by hand) accepts B's project: 16 models, 2 snapshots, 2 tests, nothing left to stub;
  - the Pentaho golden manifests are identical on the three corpora.

**Amendments from the run.** Five were defects that the gates could not see; the last four are additions:
- **The file a row came from was dropped by the first tMap.** A later tMap read `$path` from an input that did not
  have it. Each tMap output now carries `$path` when its main input does.
- **A new gate, scope:** every column a projection, filter, lookup or join reads is a column of its own input. The
  anti-invention gate checks names against the whole job, which is how the `$path` defect passed it.
- **`schema.table.column` references** were left as they were by the PostgreSQL → Trino rewrite (`etl-dbt`), while
  the table became an aliased `ref()`. Trino would refuse them, and only `EXPLAIN` would have said so. They are now
  `table.column`.
- **A component on an existing connection keeps `""` in its own `SCHEMA_DB` and `DBNAME`.** The lowering took the
  empty value instead of the connection's. Fixture B's calendar wrote a table with no schema, which the fact job's
  read did not match.
- **A date context's default is a timestamp.** It was a text literal, which date arithmetic refuses in Trino.
- **Lowered: `getLastDayOfMonth`, `getFirstDayOfMonth` and `compareDate`.** `compareDate` prints both dates with a
  numeric pattern and compares them as text, as Talend does.
- **`--locale` replaces the ISO-weeks flag of the design.** It settles both the week rule and the month and day
  names.
- **A self-read is a lookup on the target only.** A job whose main flow reads its own table re-reads every row it
  wrote; it stays a `self-read` question. A self-read source is inlined into every model that reads it: as
  `{{ this }}` in the model that writes the table, as its `ref()` in the others.
- **The translation runs on a 64 MiB stack in the command.** Fixture B's long concatenations nest deeply in the SQL
  parser, and an async worker's 2 MiB is not enough.

## Result (phase 1, 2026-11-22, devlog_351; lakehouse `95f458b`, devlog_27)

The plan is synthetic, as the exit says. Models enabled out of models written:

| Fixture (project) | Silver | Gold | Questions |
|---|---|---|---|
| B (`ALIMENTATION_DATA_WAREHOUSE`) | 9 / 30 | 1 / 43 | 74 |
| C (three projects) | 34 / 39 | 8 / 22 | 38 |
| D (three projects) | 9 / 18 | 0 / 5 | 14 |
| E (`POC_DATACLOUD`) | 12 / 116 | 5 / 68 | 186 |

The exit holds:
- **Gates:** no model on B–E is disabled by a gate.
- **Golden:** the Pentaho golden manifests are identical after the `etl-dbt` move (5, 286 and 48 lines).
- **Fixture E:** its 42 statements are listed (42 bookkeeping questions), and its 12 watermark sources carry 13
  questions: one source reads two variables.
- **`dbt parse`** (dbt 1.11, run by hand, the lakehouse's silver models stubbed) accepts the generated projects of
  B and E.

Most disabled models come from three causes, which are phase 2's patterns:
- **two writers of one table** (B 32, E 24 models): insert-new plus update-known, or slowly changing dimensions;
- **a model that reads what it writes** (16 in each of B and E), found as a cycle of `ref()`s;
- **the disabling of every model that reads a disabled one.**

The rest are sources outside the lakehouse (E's Oracle and Snowflake tables), deletes and file outputs.

**Amendments from the run:**
- **Files are dbt sources.** A file input reads `{{ source('talend_files', '<file>') }}`, listed in `_sources.yml`
  with a question: where is it landed? A file a `tFileList` names carries the landed table's `$path` column. That
  was a lowering bug the anti-invention gate found: `talend-rel` named `$path` without carrying it.
- **A model that reads what it writes**, through any cycle of `ref()`s, is disabled with a `self-read` question.
  dbt cannot build it.
- **An enabled model that reads a disabled one is disabled**, repeated until none is left. `dbt parse` refused
  the first output for this.
- **Text from the project is made inert for Jinja** (`{{` → `{ {`) in YAML descriptions and in the commented SQL
  of a disabled model. dbt renders Jinja inside comments, so a folded file name holding
  `{{(String)globalMap.get(…)}}` broke the parse, and a commented `ref()` would still make a dependency.
- **A table reference with no database** matches the one table the project writes under the same schema and name:
  an ELT input names its table, not its database.
- **The column names the shared renderer makes itself** (`ekos_match`, `ekos_rn`) are known to the anti-invention
  gate.

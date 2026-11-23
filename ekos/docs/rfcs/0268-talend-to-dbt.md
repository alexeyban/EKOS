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

## Phase 2 design: the console tab and `ekos_talend_explain` (2026-11-22)

The two halves of a Talend job's story live in different places:
- **What the job is** lives in the EKOS ledger, written by the analyzer (RFC 0262–0265): its components with their
  configuration, links, context, lineage and findings.
- **What it became** lives in the lakehouse translation: models, rules, fidelity, SQL and patterns.

They are read from where they live:
- **`ekos_talend_explain`** (EKOS MCP, read-only, in `SCOPED_TOOLS` and the console's `mcp_policy`). It takes a
  `job` (its latest version), and optionally a `project` and one `component`. It returns:
  - what the job reads and writes;
  - its components, found through the job's `Contains` edges (with one component, its whole configuration, also
    as text);
  - its row and trigger links;
  - the context variables it uses, each with its default and whether a run can replace it. A secret's value is
    never stored, and the answer says so.
  - its column lineage and its findings.

  It reads only through the call's store, which a read scope filters. Objects the latest compile no longer holds
  are left out (`talend::current_only`). `ekos talend explain --job J | --component J:C [--talend-project P]
  [--json]` gives the same answer, with catalogue class `read`.
- **`lakehouse talend report`** (class `read`): `translate` saves its report, each model's SQL and the proposal in
  `<state>/lakehouse/talend/<pkg>.json`, so the console shows a translation before anyone merges it. The report
  adds a data-flow graph: one node per component, one edge from each input.
  - `--talend-project` selects one project.
  - `--component JOB:COMPONENT` selects one component, with its models' SQL.

  The project option is not `--project`, which is EKOS's global home-project argument.
- **The Talend tab** (RFC 0198/0207 page seam). Parameters: a project from the projects panel, a component from
  the components panel. Panels:
  - projects;
  - the translation's summary;
  - the data flow;
  - models, by rule and fidelity, with why disabled;
  - patterns recognised and refused;
  - components;
  - the component's original configuration (`talend explain`) and its translation (notes, models' SQL);
  - questions;
  - the proposal.

  Actions: scan, and translate.

**Result.**
- **`ekos_talend_explain` on fixture B through `ekos mcp serve`** (no model configured, a scratch workspace):
  - `jOdsTypeClient` gives 24 components, 21 links and 9 context variables, each replaceable at run time, since
    implicit context load is on;
  - one component (`tDBOutput_1`) gives its 43 parameters and its schemas, also as text.
- **Tests:**
  - a ledger test covers components, links, context (a secret without its value), lineage and refusals;
  - the MCP tool list and scope tests pass, and so does the console policy's equality test;
  - the lakehouse's report test covers the saved translation by project and component.

## Phase 3 design: the sandbox host (2026-11-22)

The differential run executes a project's built jobs, which are third-party code, so only in a sandbox. The host is a
libvirt/KVM virtual machine (Windows 11, `win11`) on the operator's machine, driven only from outside, with no GUI
automation:
- **From the host, with `virsh`:**
  - the VM's NIC is on an isolated libvirt network, `ekos-sandbox`, with no forwarding and no route out, so it
    reaches only the host's bridge;
  - a snapshot, `ekos-sandbox-clean`, is taken with the VM shut down. Every run starts from it and reverts to it
    after, so whatever a job did is undone.
- **Inside, over SSH:** a dedicated key, as the account the operator set up.
  - The tools are unpacked under `C:\ekos-sandbox`, from zip files the host downloaded and checked: a Java 17
    runtime (Talend 8.0.1's jobs are Java 8 bytecode) and PostgreSQL's Windows binaries. There are no installers
    and no services.
  - A Windows Firewall rule blocks outbound traffic as a second wall.
  - Each run gets a throwaway PostgreSQL cluster on `127.0.0.1` (`initdb`, then `pg_ctl` while the run lasts),
    loaded from the project's DDL scripts.
- **Copied in for a run:** the job's jar, its libraries and routines, and the input files. A job runs as
  `java -cp … <package>.<job> --context_param name=value…`, with the context pointed at the sandbox: host
  `127.0.0.1`, the throwaway database, the input directory.
- **Copied out:** the written tables, as CSV through `psql \copy`, and the job's log. Nothing else leaves the VM.

**Set up (2026-11-22).** The host side has these parts:
- **Network:** `ekos-sandbox` (192.168.150.0/24) has no forwarding, and the VM gets 192.168.150.10 by its MAC.
- **Snapshot:** `ekos-sandbox-clean` was taken shut off.
- **Tools:** Temurin JRE 17.0.21.1 (its Adoptium checksum verified) and PostgreSQL 16.16's EDB binaries. EDB
  publishes no checksum, so the host recorded its own and the VM compared against it.

Checked from inside:
- the VM has no default gateway;
- the internet, the host's ports (a published container port included) and the old NAT are unreachable;
- loopback PostgreSQL works.

What setting it up taught:
- **EDB's Windows binaries need the Visual C++ runtime** (`0xC0000135` without it). Its three DLLs are copied from
  the JRE's `bin` beside `postgres.exe`, an app-local deployment, instead of running an installer.
- **Never pipe a server's output over SSH.** `pg_ctl start | Out-Null` hung the session: the server inherits the
  pipe. The cluster script (`pg.ps1`) starts it with output redirected to files and waits with `pg_isready`.
- **Windows put the new network in the Public profile, where its OpenSSH rule did not apply.** The NIC went back
  to the NAT network for one minute, live, to widen the rule to every profile. Outbound traffic was already blocked
  then.
- **Windows ignores libvirt's ACPI shutdown** on this guest; `shutdown /s` over SSH works.

### The runner (the Talend side of the differential run)

`scripts/talend-sandbox/run.sh <project-dir> <out-dir>` in ekos-lakehouse. The sandbox is named by `EKOS_SANDBOX_SSH`,
`EKOS_SANDBOX_DOMAIN` and `EKOS_SANDBOX_SNAPSHOT`. A run goes through these steps:
1. **Revert** the VM to the clean snapshot, start it, and wait for SSH.
2. **Send one archive.** It holds each job's built jar (`poms/jobs/**/target/*.jar`), the project's libraries
   (`temp/lib`, routines, beans), the input files, the project's SQL scripts and the VM-side script.
3. **Run in the VM** (`vm-run.ps1`):
   - a throwaway cluster on `127.0.0.1:5432`, the port the jobs' own context names;
   - the database the context names, then the project's create and insert scripts, in their order;
   - the input files where the jobs look for them;
   - each job in the orchestration jobs' order, `java -cp <job jar>;lib\* <class>`, with output to files,
     exit code and time recorded. After each job, every table of the project's schemas is dumped as CSV.
4. **Bring back** the dumps, the logs and a `manifest.json`. Revert the snapshot again.

The project's own files say where a job looks. **Every value the runner sets that the project did not is recorded
in the manifest as a sandbox setting**: the run day, the context rows changed, the paths a job hard-codes. Fixture B
shows all three:
- **Input directory:** `projectFolder` plus the run day's date, `TalendDate.getDate("YYYYMMDD")`.
- **Context rows:** its context table names a database `Vente_DB` while the jobs' defaults say `vente_db`; the
  runner keeps the defaults.
- **Hard-coded path:** the calendar reads `C:/Users/pc/Downloads/joursFeries.xlsx`.

A job that fails is reported with its log and exit code, and the next jobs still run. Its tables are **not verified
by execution**, with the reason.

### The comparison (the dbt side)

`lakehouse talend verify <project> --run <dir> --trino-url <url>` (class `external`). It reads the runner's output
directory and the project, and translates the project as `translate` does, with the same settings and an empty
plan: a model reading a lakehouse table is not verified, by name. Then:
1. **On a throwaway Trino, in its `memory` catalog**, under schemas prefixed with this verification's id, which
   are dropped afterwards. Trino's in-memory tables are lost on restart, and nothing is written elsewhere.
2. **Land each `talend_files` source** from the input files the run used, as the job's file component reads them:
   - its separator, header rows, encoding and empty-row setting, with no quoting when its CSV option is off;
   - its schema's types, an empty value being NULL for a non-text column;
   - `$path` as the job saw it, the VM's path.

   A listed input is named by its `tFileList` mask, so each mask lands the files it matches.
3. **Execute every enabled model's first build in `ref()` order:** incremental models in full and, for a snapshot,
   its first version. One Talend run on empty tables is the first build of each model, and that is what is
   compared. A second run, whose incremental behaviour is not compared yet, is stated as such.
4. **Compare each gold model that writes a table with the table the jobs left after the last job.** Both sides are
   multisets of rows, over the columns both have, compared as normalised text: numbers canonical, timestamps without
   trailing zero fractions, booleans `true`/`false`. These columns are left out, each named with why:
   - columns set from the clock (`Now`/`Today` in the IR; a snapshot's `dbt_valid_*`);
   - database-assigned keys (a snapshot's `surrogate`), and every column of another model with the same name, since
     both hold values of that key.

   Per model the result is `equal`, `differs` (counts each side, and up to five rows only on one side), or
   `not verified` (why).
5. **Comparer control:** the comparison is repeated on the first `equal` model with one of its dbt rows removed,
   and it must report `differs`, or the result is not trusted.

The report goes to `<state>/lakehouse/talend/<pkg>.verification.json`, and the summary to the console.

### A second run: the incremental behaviour

A model's first build is compared above; a Talend job's state-keeping (merges, appends, closing versions) shows only
on a later run. The plan can name a second run:
- **`runs[1].edits`:** changes to the run-1 input files, made on the host: `replace` a text, `append` a line. The
  fixture's files are never committed changed.
- **The runner:** after run 1, it replaces the input files with run 2's and runs the jobs again, on the tables run 1
  left. Dumps are kept per run (`r1_…`, `r2_…`).
- **`verify`:** it re-lands the run-2 files and builds each model as dbt's second run would:
  - a table is rebuilt;
  - an append inserts its incremental rows;
  - a merge (and an update, a keyed replace, a `rej_` model) replaces the rows whose key its incremental rows hold;
  - a model's `{{ this }}` is its run-1 table;
  - a snapshot (`check`) closes the current version of each key whose compared columns changed and inserts the new
    versions and new keys.

  It then compares with the tables after run 2. Trino's `memory` catalog has no `MERGE`, so each step builds a new
  table, `<alias>__r2`.

A difference on run 2 that the translation already reports (the snapshot keeps history Talend overwrites) is
expected. The report names it, and the comparison does not hide it.

### Result: the first differential run (2026-11-22, fixture B)

**The Talend side.** Fixture B's ten jobs all ran to their end in the sandbox, in their orchestration order with the
calendar before the DWH. Rows written:

| Table | Rows |
|---|---|
| `DIM_CALENDRIER` | 2,192 |
| `FAIT_VENTE` | 500 |
| `DIM_CLIENT` | 17,415 |
| `DIM_PRODUIT` | 3,788 |
| `ODS_REJET` | 4 |

The calendar's 2,192 rows are the count the translator computes from the context defaults.

**The comparison** (`--locale fr_FR`, the sandbox JVM run as `fr_FR`):

| Table | Rows | Result |
|---|---|---|
| `DIM_CALENDRIER` | 2,192 | equal on all 17 columns: week numbers, month and day names, holidays, last day of month, leap years |
| `ODS_CATEGORIE` (a snapshot) | 3 | equal, `DT_INSERTION` left out as the run's time |
| `ODS_TYPE_CLIENT` (a snapshot) | 3 | equal, `DT_INSERTION` left out as the run's time |

- **Built:** all 18 enabled models built on Trino.
- **Not verified:** the 12 other outputs. Their models are disabled, downstream of the two ODS jobs refused for
  defects in them; each is reported with its reason.
- **Control:** the comparer found its planted difference.

**Amendments from the run:**
- **Six different files had one source name.** A file input that reads a `tFileList`'s current file was named after
  the path expression, the same text in every job. It is now named by that list's masks (`ICOM_*_TYPE_CLIENT.csv`),
  so each file lands apart. This was a translation defect.
- **A file's name was wrong on Windows.** `CURRENT_FILE` was `regexp_replace($path, '^.*/', '')`, but a job run on
  Windows stores `C:\…\file.csv`. It now cuts at either separator, as Java's `File.getName()` does. This too was a
  translation defect.
- **The JVM's locale is a sandbox setting** (`jvm_locale`). The VM's own Java ran `en_US` and printed `January`,
  where the translation, told `fr_FR`, printed `Janvier`. Both sides run one stated locale.
- **The calendar was compiled but not packaged** by the project's build. The runner zips its `target/classes`;
  nothing is compiled. Its Excel libraries, named by its pom and missing from `temp/lib`, come from the Talend
  Studio 8.0.1 the project was built with (`EKOS_TALEND_STUDIO`). Both are recorded in the run's `origins.json`.
- **What the landing had to learn, all from the files:**
  - `CUSTOMER.csv` ends its lines with a bare CR, and Talend's reader takes CR, LF and CRLF;
  - a delimited date column is read with its schema's pattern (`dd-MM-yyyy`);
  - a workbook's dates are Excel day numbers.
- **A `date` and a midnight `timestamp` normalise to one value.** PostgreSQL's column is a date, and the model's a
  timestamp.
- **The runner's own defects, found on its first run:**
  - PowerShell's `Start-Process` joins arguments without quoting them, so SQL goes through files;
  - a process object loses its exit code unless its handle is read first. A job that does not run
in the sandbox stays **not verified by execution**, with the reason.

### Result: the second run, the remaining items (2026-11-22)

**The second run** (fixture B; run-2 edits change one category and one client type and add one of each):
- **`DIM_CALENDRIER`:** equal again, 2,192 rows.
- **`ODS_CATEGORIE` and `ODS_TYPE_CLIENT` differ, and the jobs are at fault.** Each job's closing `UPDATE` writes
  NULL into columns its own DDL declares `NOT NULL`. PostgreSQL refuses it, the batch rolls back with the new
  version's insert, and the job still exits 0 with the error in its log. The original jobs therefore cannot load
  any change to a category or a client type.
- **The snapshot behaves as the job evidently meant:** it closes the old version and adds the new version and the
  new key.
- **The verification reports each difference with the job's error lines** (`job_errors`, from the run's manifest).
  This confirms what phase 2's `snapshot` question only suspected.

**The remaining items:**
- **Console:** `lakehouse talend report` attaches the latest verification, and the Talend tab shows it under
  "Verified by execution".
- **Fixture E's chained keyed replace:**
  - **The rule:** the left join on the target may sit anywhere in a chain. The insert's path from it is rebuilt on
    the staged side when no node on it reads a target column (a projection that only passes one on is dropped).
  - **The delete:** it may have no filter, since an unmatched row's key is NULL and deletes nothing.
  - **Result:** E has 19 keyed replaces (10 before), 27 / 146 models enabled (25 / 164 before), no gate disables.
    `FACT_MOV_COST` stays refused: it deletes a key it computes.
- **Fixtures C, D and E in the sandbox: not verified by execution (decided 2026-11-23).** The reasons:
  - **No built jobs:** none of the three ships one (no jars, compiled classes or generated Java).
  - **No headless build:** Talend Open Studio 8.0.1 has none. Its command-line builder
    (`org.talend.commandline`) ships only with the commercial Studio, and the Studio window is not automated.
  - **E needs Snowflake:** every data job of E reads or writes Snowflake (ELT pushed down into it, or its outputs),
    and some read Oracle. An offline sandbox can run none of them, built or not.
  - **C and D write files:** about 20 of their jobs touch no external system, but they write files. A file output
    has no dbt model (rule `file`), so the comparison has no counterpart to compare with.

  Comparing a job's output file with the SQL of its chain would be new work, and is not planned.
- **LLM fill-in, RFC 0205 for Talend:**
  - **Moved code:** `DraftRequest`, the answer parser and the checks moved from `pentaho-dbt` to `etl-dbt`
    (re-exported; golden identical).
  - **`talend-rel`:** it offers a gap whose one input is fully known (a code component's code, or a tMap output's
    expressions), and applies a checked draft as `llm-drafted`.
  - **`lakehouse talend translate`:** it drafts by the configured `translate` route (Ollama needs no flag;
    anything else `--allow-metered`), keeps the drafts in `<pkg>.drafts.json`, and `verify` uses them.
  - **Measured locally, never metered:**
    - `qwen2.5-coder:3b`: 0 of 7 gaps passed the checks; it wrote Java names into SQL.
    - `glm-4.7-flash`: 2 of fixture C's 5 passed. It refused, correctly, a `tJavaFlex` that counts across rows.
  - **Nothing unchecked was accepted.**

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

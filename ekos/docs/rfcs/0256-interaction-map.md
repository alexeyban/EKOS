# RFC 0256 — The interaction map: who reads and writes each table, declared and observed

**Status:** Accepted (2026-11-11, on the maintainer's instruction to start work)
**Date:** 2026-11-11
**Depends on:** RFC 0075 (`ReadsFrom` / `WritesTo`), RFC 0163 (statement footprints), RFC 0212 (write behaviour),
RFC 0157 (usage evidence, redaction at the live entry point), RFC 0083 (system decomposition), RFC 0214 (estate
docs), RFC 0247 (annotations), RFC 0254 (open questions, source passport), RFC 0226 (extraction plan review),
RFC 0198 (console extension pages), RFC 0255 (`usage_stats` capability)
**Repos:** EKOS (`pg-live`, `semantic`, `docs-gen`, `cli`); ekos-lakehouse (`lakehouse-ext`, the plan warning only)

---

## 1. Motivation

An analyst preparing a source asks three questions about every table before trusting it:

1. **Who owns it** — which system or process creates and changes its rows?
2. **Who else touches it** — which consumers break if it changes?
3. **Is that the whole story** — does anything write to it that nobody has told me about?

EKOS answers the first two per object. `ReadsFrom` and `WritesTo` edges come from SQL, routines, Kettle steps and
Java (RFC 0075, 0163, 0212), and RFC 0214's table page lists a table's readers and writers. Two things are missing.

- **No system-level view.** The edges join one statement to one table. Nothing rolls them up into "system A writes
  these 40 tables, system B reads 12 of them", which is the picture an analyst draws by hand in the first week.
- **Nothing observed.** Every edge is *declared*: it exists because source code says so. RFC 0157 reads
  `pg_stat_user_tables` and `pg_stat_statements`, but uses them for design hints (`ORDER BY`, write rates). It does
  not ask *who* ran the statements. So the third question has no answer: a nightly script on someone's server, a
  BI tool writing back, a DBA's manual fix — none of them is in any repository, and all of them write rows the
  lakehouse will copy.

The third question matters beyond documentation. RFC 0212 proves a table's update class from code ("every `UPDATE`
sets `Updated`"). A writer outside the code is outside the proof.

## 2. Goals and non-goals

**Goals**

1. **Actors**: a small, closed set of things that read and write, each traceable to evidence.
2. **Observed access**: per database role, table and operation, from the engine's statement statistics, as dated
   observations.
3. **The matrix**: actor × table × operation, with declared and observed side by side.
4. **Findings**, derived deterministically: an unexplained writer, a table with several writing systems, a
   declared write never observed.
5. Findings feed what exists: open questions (RFC 0254), the passport, the extraction plan review (RFC 0226).

**Non-goals**

- Network or API-level interaction (who calls which service). This RFC is about data stores.
- Query auditing. No statement text, no per-statement history and no per-user activity log is kept; only
  aggregates per role, table and operation.
- Deciding ownership. EKOS reports who writes; a person says who owns (an annotation, RFC 0247).
- Mapping a role to a system automatically. A match is a hypothesis for review, as cross-system identity is.
- Timing of writes (when a table is complete for the day). Statement statistics do not carry it; a later RFC may
  take it from scheduler metadata.

## 3. Design

### 3.1 Actors

An actor is one of:

| Kind | Source | Already in the ledger |
|---|---|---|
| `component` | application code, grouped as RFC 0083 groups it | yes |
| `routine` | a procedure, function or trigger in the database | yes |
| `etl_job` | a Kettle job or transformation | yes |
| `db_role` | a database role seen running statements | **new** |

The first three are views over existing objects; no new kind. A `db_role` is a new keyed `Custom("DbRole")`
(database name + role name; registered, `structurally_keyed: true`).

### 3.2 Observed access

Read by the live connector, in the session RFC 0157 defines, when `pg_stat_statements` is installed:

1. For each row: the role (`userid` joined to `pg_roles`), the normalized statement text, `calls` and `rows`.
2. The text passes through redaction (RFC 0157's rule for this view) and then through the existing statement
   footprint (RFC 0163): the tables read and written, and the operation per table.
3. Aggregated **in process** to (role, table, operation) → calls, rows. The statement text is then dropped.

The result is one `Custom("ObservedAccess")` fact per (database, role, table, operation):

| Property | Meaning |
|---|---|
| `calls`, `rows` | totals over the window |
| `window_start` | the view's last reset (`pg_stat_statements_info.stats_reset`) where the server reports it, else unknown |
| `window_end` | when it was read |
| `truncated` | the view evicts entries beyond `pg_stat_statements.max`; when the server reports evictions (`dealloc`), the fact says the observation is incomplete |
| `unparsed_calls` | calls whose statement the footprint could not attribute to a table |

Rules:

- **An observation is dated and partial, and is labelled so everywhere.** "Not observed" over a three-day window
  is not "never happens".
- EKOS's own sessions are excluded by role and by statement shape (catalog and profile queries).
- **Role names.** A role may be a person's account. `[interaction] roles = "names" | "hashed" | "off"` (default
  `names`); with `hashed`, roles are salted hashes (the project salt of RFC 0157) and a reviewer can still tell
  two roles apart. Roles a redaction rule matches are always hashed.
- When the extension is absent, or the engine's `usage_stats` capability is absent (RFC 0255), nothing is
  recorded and every surface says "this source provides no usage statistics" — not an empty list.
- `pg_stat_user_tables` gives per-table insert/update/delete counts with no role. It is the cross-check: a table
  whose counters show writes while no statement in the window explains them is itself a finding (§3.4).

Other engines provide observed access through their own live connector when they declare `usage_stats`. Oracle is
not part of phase 1.

### 3.3 Linking a role to an actor

A role is linked to a `component` or `etl_job` only by evidence, and only as a hypothesis:

- the role is the user of a connection the code or job declares (a JDBC URL, a Kettle connection, a
  `[[databases]]` entry);
- the set of tables the role touches is contained in the set one component touches, and in no other's.

Each link is a review item (the RFC 0029 rule: recorded as unconfirmed, never merged automatically). A person
confirms, rejects, or names the actor in an annotation ("`etl_nightly` is the finance team's export script").
A confirmed link does not carry forward when the role's observed tables change beyond what the actor declares.

### 3.4 Findings

Derived at `ekos commit` by a pure function over declared edges, observed access and confirmed links. Each is a
keyed fact with its evidence, current while its inputs are.

| Finding | Rule | Why it matters |
|---|---|---|
| `unexplained_writer` | a role writes a table, and no actor linked to that role has a declared write to it | a writer outside the code: RFC 0212's update class for the table does not cover it |
| `unattributed_writes` | `pg_stat_user_tables` shows writes that no observed statement explains | same, weaker: the window or the view is incomplete |
| `multiple_writing_systems` | two or more unlinked actors write the same table | ownership is unclear; the analyst must ask |
| `declared_not_observed` | a declared write with zero observed calls, the window longer than `[interaction] quiet_after` (default 30 days) and not `truncated` | a dead path candidate; low confidence by construction |
| `read_only_consumers` | actors that only read a table (not a problem: the list of who breaks) | impact of a change |

Each of the first three produces an RFC 0254 open question (`who_is_this_writer`, `who_owns_this_table`), routed
and answered as any other.

### 3.5 Surfaces

| Surface | What |
|---|---|
| `ekos interaction matrix [--table] [--actor] [--json]` (class *read*) | the matrix: rows are tables, columns actors, cells the operations, each marked declared, observed or both |
| `ekos interaction findings [--json]` (class *read*) | the findings with evidence |
| console | an **Interaction** page: the matrix as a table (RFC 0198 `table` view) and the findings; a system-level graph on the existing graph view, actors and tables as nodes |
| table page and passport (RFC 0214, 0254) | "Writers and readers" gains the observed column, the window and the open findings |
| MCP `ekos_interaction` | read-only, scoped (RFC 0191) |

### 3.6 The lakehouse

`lakehouse plan` reads the findings. A table with a current `unexplained_writer` or `unattributed_writes` carries
a warning on the extraction plan review (RFC 0226), naming the role and the window: the watermark was proven from
code, and something outside the code writes here.

**EKOS advises; a person decides** (owner's decision, 2026-11-11). The plan's load mode is never changed by a
finding. The warning carries advice: the mode EKOS would recommend for this table (for example full refresh when
the table is under `full-refresh-max-rows`, otherwise a wider reconciliation window), the reason, and what it
would cost. The mode changes only by an explicit instruction from the operator on that table, recorded in the
plan review with who, when and the finding it answers. Keeping the planned mode is also a recorded decision, so
the warning does not return on every plan; it returns when the finding's evidence changes (another role, another
table).

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | Declared matrix from existing edges; `ekos interaction matrix`; table page and passport | on iDempiere: the matrix lists, per table, the Java, PL/SQL and trigger writers already on its table page, with equal counts |
| 2 | `DbRole`, `ObservedAccess` for PostgreSQL; role settings; the labels for window and truncation | a fixture database with a known workload from three roles yields the expected facts; no statement text in the ledger or a log |
| 3 | Role–actor link hypotheses and review; findings; questions | a planted writer (a role updating a table no code touches) is reported as `unexplained_writer` and raises a question; a confirmed link removes it |
| 4 | Console page, MCP tool, extraction plan warning with advice and the explicit decision | a browser run on the testbed; the planted writer's table carries the warning and the advice in the plan review; the mode changes only after the operator's instruction, and a "keep" decision silences the warning until the evidence changes |

## 5. Tests

- A statement the footprint cannot parse adds to `unparsed_calls` and to no table.
- EKOS's own sessions never appear.
- `roles = "hashed"`: no role name in any fact, page or log; two roles stay distinct.
- A window shorter than `quiet_after`, or a truncated view, never yields `declared_not_observed`.
- No `pg_stat_statements`: every surface states it, and no finding is derived from absence.
- A restricted reader (RFC 0181) sees only cells for tables in scope.

## 6. Open questions

1. *Decided (owner, 2026-11-11):* an `unexplained_writer` never changes a table's load mode by itself. EKOS
   advises; the operator gives an explicit instruction (§3.6). *Decided (2026-11-12):* the instruction is an
   RFC 0226 per-table change (`changes.json`, with who, when and why), which now records the evidence of the
   warnings it answers; keeping the mode is a new change of the same kind, `--keep`. No second store.
2. Statement statistics are cumulative. Reading them at every build gives a series of windows; should EKOS keep
   deltas between builds to answer "since when"? It is cheap, and moves toward monitoring, which RFC 0252 keeps
   out of profiling. *Decided (2026-11-12):* the cheap half only, no series. Each observed cell carries
   `first_observed` (the earliest observation that held it) and `calls_since_previous` (known only when the
   statistics were not reset in between). Nothing is kept per observation beyond the newest fact.
3. `pg_stat_statements` has no client application name. A sampled read of `pg_stat_activity` would add it and is
   a point-in-time sample of sessions, including client addresses. Worth it only if roles prove too coarse.
   *Decided (2026-11-12): not now.*
   - On every fixture so far (LedgerSMB, the phase 2 fixture, iDempiere on Oracle, ClickHouse) roles were enough.
   - A connection's declared user (§3.3) separated an ETL job from the application without it.
   - A session sample would bring client addresses into the ledger's inputs, which nothing here needs.
   - Revisit when a source shows one role shared by several systems.
4. iDempiere's application dictionary names scheduled processes. RFC 0212's opt-in dictionary reader reads only
   `AD_Table`; extending it to processes would give a fifth actor kind for dictionary-driven ERPs. *Done
   (2026-11-12)*, see §9.

## 9. Implementation log

### Phases 1–2 (2026-11-11)

**Built.**
- `ekos_kir::interaction` (pure): `Actor` (`component`, `routine`, `etl_job`, `db_role`), `Op`, `Cell`
  (declared operations, observed counts with their window, `via` citations), `Matrix`, `declared`.
  - Routines, views and triggers come from their footprints (`reads`/`writes`/`inserts`/`updates`/
    `deletes`; a trigger also writes its own table).
  - Components come from `JavaMethod.sql[]`, grouped by the leading directories of the method's file
    (`[interaction] component-depth`, default 1).
  - ETL jobs come from Kettle `TransformNode` `ReadsFrom`/`WritesTo` edges, grouped by `.ktr`/`.kjb`.
- `ekos interaction matrix [--table] [--actor] [--json]` (class *read*).
- `ekos interaction observe [--database] [--allow-writable-role] [--json]` (class *external*), PostgreSQL:
  - reads `pg_stat_statements` (+ `_info`, `pg_stat_user_tables`) in the read-only probe session;
  - redacts each statement and takes its footprint (`statement_footprint_in`, PostgreSQL dialect);
  - aggregates in process and drops the text;
  - writes keyed `ObservedAccess`, `DbRole` and `ObservationWindow` facts, listed in
    `<state>/interaction/current.json`. Readers count only the listed facts.
- `[interaction]`: `component-depth`, `roles = names | hashed | off`, `quiet-after-days`. A role a
  redaction rule matches is always hashed (salted with the project salt, `<state>/profile/salt`).
- The estate table page's "Read by" / "Written by" lists are replaced by **"Writers and readers"** from
  the same matrix (declared, observed, window), with a note when no usage statistics were read.

**Decisions.**
- Observed access is read by its own command, not at `ekos build`. This mirrors `ekos profile run`: a
  live read stays opt-in and outside the build fingerprint.
- A routine read from a live catalog is an actor only when no file defines one of the same name
  (RFC 0211's rule). On LedgerSMB this halved `acc_trans`'s actor list (103 → 55): every routine was
  there twice, once from `sql/` and once from the live catalog.
- Java test code (`java_po::is_test_path`, RFC 0212's rule) is no actor. iDempiere's
  `org.idempiere.test` showed up as a deleting writer of `c_order` before this rule.
- Footprint names of relations the ledger does not know (temporary tables, CTEs, record variables)
  are dropped.
- The window and the per-table counters (the cross-check of §3.4) are one `ObservationWindow` per
  database. `unparsed_calls` is per role on that fact, since an unparsed statement names no table.
  Calls whose text the session may not read (`<insufficient privilege>`, no `pg_read_all_stats`) are
  counted as `hidden_calls`.

**Measured.**
- LedgerSMB (sql/ + live PostgreSQL): 148 tables, 476 actors, 1,085 cells.
- iDempiere (home project `idempiere`): 621 tables, 2,180 cells (1,165 routine, 1,015 component),
  303 writing (251 component, 52 routine). Oracle and PostgreSQL scripts of the same view are two
  actors (`C_ORDER_HEADER_V`, `c_order_header_v`): two databases' code, as elsewhere.
- Phase 2 exit, on a throwaway PostgreSQL 16 with `pg_stat_statements` and a known workload from three
  roles. The facts equal the workload:
  - `app_writer` inserts 20 / updates 5 on `orders`;
  - `etl_nightly` updates 3 on `customers`;
  - `reporter` reads both 7 times;
  - the reading role's 38 calls are left out;
  - the superuser's 20 DDL calls are `unparsed_calls`.
- A scan of every file under `.ekos/` (zstd-decompressed) for text unique to the observed statements
  found none.

### Phase 3 (2026-11-11)

**Built.**
- `ekos_kir::interaction::{link_hypotheses, findings}`, pure:
  - a role is linked to the one component or ETL job whose tables contain every table the role
    touches (none or several: no hypothesis);
  - findings `unexplained_writer`, `unattributed_writes` (per operation, from the table counters),
    `multiple_writing_systems`, `declared_not_observed` (needs a known window longer than
    `quiet-after-days` and no truncation). With no observation window, nothing is derived.
- Review subject **`interaction`** (optional, like `relationships`; `ekos review interaction <role>
  --confirm|--reject [--reason] --as`, human-only through the review machinery). One item per role
  seen writing:
  - signed facts: database, role, the `table:operation`s it writes, the proposed system;
  - shown only: calls and the tables where its writes are unexplained (blocking while undecided).
  - Because the writes are signed, a confirmation goes `outdated` when the role starts writing
    elsewhere. This is the RFC's "does not carry forward" rule, with no extra code.
- `interaction::commit_step` at the end of `ekos commit` (before the questions) and of `ekos
  interaction observe`: `DbRole` items and `InteractionFinding` facts (appended only when changed),
  `<state>/interaction/findings.json`. `ekos interaction findings [--json]` (class *read*).
- Questions (RFC 0254): `unexplained_writer` and `unattributed_writes` → `who_is_this_writer` (the
  first blocking); `multiple_writing_systems` → `who_owns_this_table`. `declared_not_observed` asks
  nothing (low confidence by construction).

**Decisions.**
- *Explained*: a person confirmed the role; or the role is linked and its actor declares that write;
  or, with **no link**, some code actor declares that write on the table. Without the last rule,
  every role running known code (an application role calling the routines EKOS read) would be
  "unexplained" until someone confirms it. That is noise, and it hides the writer outside the code,
  which is the point.
- The role link is not a separate `SameAs`-style relationship. It is the review item's
  `proposed_actor`: a confirmation accounts for the role (as that system, or as the one the reviewer
  names in the reason); a rejection drops the proposal.
- `multiple_writing_systems` counts components and ETL jobs that declare a write, plus unexplained
  roles. Routines are not systems; they are called by them.

**Measured** (the phase 2 fixture):
- `etl_nightly`'s update of `customers` is an `unexplained_writer` and raises a blocking
  `who_is_this_writer`. `app_writer` is explained by `place_order`/`pay_order`.
- The 50 rows loaded into `customers` before the statistics reset are `unattributed_writes` (insert).
- `ekos review interaction etl_nightly --confirm` removes the finding and its question at the next
  commit.
- After `etl_nightly` then deleted a row of `orders`, the decision was `outdated` and both its
  writes were findings again.

### Phase 4, EKOS side (2026-11-11)

**Built.**
- MCP **`ekos_interaction`** (read-only, in `SCOPED_TOOLS` and the console's `mcp_policy`): the
  matrix (`table` / `actor` filters, at most 50 tables unfiltered) and the current findings for the
  tables shown. The analyst-loop tools now go through one `analyst_tool` dispatcher in `mcp.rs`, which
  keeps `call_tool` under the complexity limit.
- A restricted reader (RFC 0181) gets a row only for a relation its store returned.
  - Found while testing: observed facts carry no link to the table object, so with the table denied,
    its row and findings still came back through them. `retain_visible` now drops them; a unit test
    covers it.
  - Measured with `mcp serve --read-scope` denying both `customers` tables (file and live): the table
    and its two findings are gone, `orders` and its finding remain. A source-restricted reader sees no
    observed access at all (the facts carry no source).
- Console: an **Interaction** page (findings, then the matrix per table, with a table filter), routes
  `GET …/interaction` and `GET …/interaction/findings` (read), and both CLI reads on the
  `readproc` allowlist. `who_is_this_writer` / `who_owns_this_table` are in the Questions page's type
  filter. Accounting for a role stays on the Review page (`interaction` subject).
- The estate table page lists the table's open findings under "Writers and readers".

**Not yet.**
- The lakehouse extraction-plan warning with advice and the operator's explicit decision (§3.6). It
  lives in ekos-lakehouse, where RFC 0255 is changing the plan code now, so it is left for after that
  merges. Open question 1 (where the decision is stored) is still open.
- Oracle observed access (done 2026-11-12, see "Remaining tasks").
- The browser run of the exit (done 2026-11-12, see "Remaining tasks").

### Phase 4, lakehouse side (2026-11-12)

**Built** (ekos-lakehouse `ba1ebf6`, `crates/lakehouse-ext/src/writer_warnings.rs`, pure):
- `lakehouse plan` reads the current findings through `ekos::commands::interaction::current_findings` and keeps
  the `unexplained_writer` / `unattributed_writes` ones in the proposal, with the source's row estimates
  (`plan_cmd::build_plan_checked` now returns them).
- `extraction::finalize` derives the warnings against the proposal's modes. Only an **incremental** table
  warns: a full table reads every row on every run. A finding's table (EKOS's bare lower-case key) matches every
  planned table of that name.
- The advice, never applied:
  - full load when the table's row estimate is within `full-refresh-max-rows`, costed in rows;
  - otherwise, when the outside writer deletes and the table's deletes are audited, look for deletes every run;
  - otherwise keep the mode and reconcile after each run.
- `extraction show` (JSON and text) and the console's Extraction page show them: a summary count of open and
  answered warnings, a column on the tables list, a "Written outside the code" panel, and a section in the
  table's evidence.
- The decision: any `extraction change` to the table records `answers`, the evidence of the warnings on it at
  that moment. `--keep` (console action "Keep the planned mode") is a change of kind `mode` that alters
  nothing and is refused when there is nothing to answer.
- Evidence is a hash of finding, table, role and operations: never counts. More calls by the same role stay
  answered; another role or another operation is open again. `--undo` removes the answer with the change.

**Decisions.**
- Warnings are outside the plan and its digest. Keeping the mode leaves an approval valid; following the advice
  changes the plan as any change does and needs approving again.
- An open warning does not block approval. The RFC asks for a warning and a recorded decision, not a gate.

**Measured.** Unit tests only (`writer_warnings` 7, `extraction` 1):
- a warning appears on an incremental table and not on a full one;
- the plan is identical with and without findings;
- `--keep` leaves the digest unchanged;
- a new role reopens the warning.

**Not yet.**
- The phase 4 exit: a browser run on the testbed with the planted writer (done 2026-11-12, see "Remaining tasks").
- Oracle observed access (done 2026-11-12).

### Remaining tasks (2026-11-12)

**Built.**

- **Observed access on more engines** (§3.2).
  - **Every engine.** `interaction observe` reads each `[[databases]]` engine through the same read-only probe
    session, parses its statements in its own dialect, and drops them.
  - **ClickHouse:** `system.query_log`.
    - One sample statement per (user, normalized statement) of the finished queries on the entry's `owners`.
    - `normalizeQuery`'s `?..` lists do not parse, so the sample keeps its literals until it is dropped.
    - The window starts at the oldest entry the log keeps. There are no table counters, so no
      `unattributed_writes`.
    - An insert whose rows came as data (`INSERT INTO t VALUES`, `… FORMAT CSV`) gets a placeholder row before
      parsing, so its table is still found.
  - **Oracle:** `V$SQL` joined to `ALL_USERS` (the reader needs `SELECT_CATALOG_ROLE`).
    - Inserts, selects, updates, deletes and merges, by users Oracle does not maintain itself.
    - The window starts at the instance's start-up.
    - Cursors age out of the shared pool, so every observation is marked partial and never yields
      `declared_not_observed`.
    - `DBA_TAB_MODIFICATIONS` counts since the last statistics gathering, a different window, so it is not the
      cross-check.
  - **SQL Server and Informix** record no per-login statement statistics EKOS reads. Each is recorded as
    "provides no usage statistics", with the reason.
  - **Schema statements** (`CREATE`, `DROP`, `ALTER` other than a ClickHouse mutation, grants) are counted
    apart (`ddl_calls`) and attributed to no table. A `CREATE TABLE` used to read as a read of the new table.
  - **The reading user** is left out case-insensitively (Oracle upper-cases it).
- **A source without usage statistics is said so everywhere** (§3.2, §5).
  - `<state>/interaction/current.json` records the reason per database.
  - The matrix (`no_usage_statistics`, the observed note), `findings`, MCP `ekos_interaction` and the console
    page all state it.
  - An earlier observation of that database stops being current, and no finding is derived from the absence.
- **Since when** (open question 2): `first_observed` and `calls_since_previous` on every observed cell.
  `unexplained_writer` carries `first_observed`; the lakehouse warning shows "first seen <date>".
- **Link by connection** (§3.3).
  - The Kettle analyzer (pass `v3`) records `connection_user` on each step that uses a connection: the
    `<username>` of the transformation's `<connection>`, never the password. A user given as a variable is
    skipped.
  - `link_hypotheses_with` proposes a role that is the user of exactly one system's connections as that system
    before the table-containment rule.
  - The role item shows `link_basis` (`connection` | `tables`) as information, not as a signed fact.
  - Users are compared as roles are shown (hashed when roles are), and case-insensitively.
  - A `[[databases]]` entry's user is EKOS's own, already left out.
- **Kettle write steps.** `Update`, `InsertUpdate`, `Delete` and `SynchronizeAfterMerge` are sinks on their
  `<lookup>` table. Before this, a job that updates a table declared only a read of it.
- **`read_only_consumers`** (§3.4): per table, the actors that only read it, declared or observed. Pure
  (`interaction::read_only_consumers`), apart from the problems: it asks no question and no lakehouse warning
  reads it. The console lists it under the findings.
- **A trigger explains no role's write.** A trigger's declared access (its own table) used to explain every
  outside update of the table through the no-link rule; now it does not. A trigger runs because a statement
  wrote the table, and `pg_stat_statements` tracks top-level statements only.
- **The system-level graph** (§3.5): the console's Interaction page has Per table / Systems views.
  - Systems draws components, ETL jobs, processes and roles writing into a table and reading out of it.
  - It uses the existing deterministic `layoutGraph`, marks tables with open findings, shows at most 25 tables
    (the most written first), and says how many it left out.
- **Application processes** (open question 4).
  - `ora-live::dictionary::read_all` reads `AD_Process` (value, name, procedure, class, active, active
    `AD_Scheduler` entries) with the `AD_Table` flags, in the same read-only transaction.
  - The Oracle catalog analyzer (`oracle-catalog/6`) writes one keyed `AppProcess` per process. It is a new
    registry row, with an identity namespace of its own, because a process is named after the table it
    maintains or the procedure it runs.
  - `ActorKind::Process` acts through the procedure (by name) or the Java class (`JavaMethod.owner`) it runs:
    only that class's own SQL, not calls into other classes.
- **Lakehouse.**
  - The Oracle engine's `usage_stats` is now `partial` (V$SQL through `interaction observe`), no longer
    `absent`.
  - The warnings panel drops the long reason column; the reason is in the table's evidence.

**Not done, and why.**
- **Deleted code still counts** — *fixed the same day*, see "Deleted code" below.
- **iDempiere's PostgreSQL dictionary:** the dictionary reader is Oracle-only (RFC 0212), so processes come
  from Oracle sources only — *done the same day*, see "iDempiere on PostgreSQL" below.

**Measured.**
- **PostgreSQL** (phase 2 fixture).
  - `first_observed` carried from the first observation (2026-11-11), `calls_since_previous` 0 on a re-read.
  - A Kettle job connecting as `etl_nightly` and updating `customers` made `etl_nightly` proposed as that job
    (`link_basis=connection`), though by tables alone it touches `orders` too.
  - Its update of `customers` became explained; its delete of `orders`, which the job does not declare,
    stayed an `unexplained_writer`.
- **ClickHouse 24.8** (the migrate testbed, scratch database `rfc0256_ix`).
  - The workload: `loader` 5 inserts into `events`; `ekos` 3 selects of `events` and 1 insert into `kinds`;
    3 DDL statements.
  - The facts equal it, with 0 unparsed and `ddl_calls` 3; the reading user `ekos_obs` was left out.
- **Oracle XE 21c** (the iDempiere seed).
  - `ETL_NIGHTLY` 4 updates of `C_BPARTNER` (bind variables `:1`) and `ADEMPIERE` 3 reads and 3 updates of
    `C_ORDER` (rolled back) became exactly those cells.
  - Both are `unexplained_writer`s, the observation is marked partial, and `EKOS_RO`'s 24 calls were left out.
  - 337 `AppProcess`es read, equal to a direct count (4 procedures, 333 classes). With
    `org/compiere/process` (37 classes) added, 8 processes are actors on 13 cells (for example
    `DatabaseTableRename` reads `ad_table`).
- **Phase 4 exit, in a browser** (Playwright on the console, local mode, the lakehouse binary built on this
  EKOS). The fixture's `customers` was given an `updated_at` watermark proven by a trigger, a planted role
  `fixer` updated it, and the run passed 17/17:
  - the Extraction page carried the warning (role `fixer`, update, window, first seen) with the advice "load in
    full", costed at 50 rows, while the plan kept `customers` incremental;
  - "Keep the planned mode" (with a reason) left the mode incremental and answered the warning, recorded with
    who and when; a new plan on the same evidence kept it answered;
  - `etl_nightly` then updated `customers`: the warning reopened for `etl_nightly` only, `fixer`'s stayed
    answered, and the mode stayed incremental;
  - "Load in full" changed the mode to full and answered the open warning;
  - no script error in the page.

### Deleted code (2026-11-12)

A removed `.ktr` stayed an actor, explaining a planted writer's update. There were three causes, one per layer:

1. **The collectors.** `recover`'s file collectors pick the newest observation per target from an append-only
   store, which for a deleted file is its last one. `present_files` now drops an observation whose target
   exists under no observe base (Pentaho, Python, Rust, Perl, Elixir, JavaScript, Java, local documents).
2. **The knowledge artifacts.**
   - A pass that no longer runs (a deleted SQL file's) leaves its output in the store, and a multi-input pass
     writes one artifact per run with different inputs.
   - `compile`'s newest-per-target dedup kept both kinds.
   - `PassManager` now records the knowledge artifacts each pass writes (a `RecordingStore` around the pass's
     store). Each pass's cache manifest keeps them as `outputs`, so a cached pass still names its output.
   - `recover` writes the run's union to `<artifacts>/current-knowledge.json`.
   - `compile` and `resolve` read only those (`ekos_semantic::select_knowledge_artifact_ids`). A pass whose
     manifest predates `outputs` is kept by name until it next runs. `resolve` used to read every artifact
     ever written.
3. **The ledger.** It is append-only, so a deleted file's objects stay there. The interaction map counts a
   code object (procedure, view, trigger, Java method, Kettle node, process) only when the latest compiled
   model holds it.

Measured on the phase 2 fixture: with the `.ktr` removed, the compiled model has 33 objects and no Kettle
node, and the matrix shows no job. With it restored, the job is back, and removing it again drops it again.
EKL, search and documentation still read the whole ledger; whether they should see only current objects is a
separate decision.

### iDempiere on PostgreSQL (2026-11-12)

iDempiere runs on PostgreSQL as well as Oracle, with the same dictionary tables in the application's schema.
`[[databases]] kind = "postgres"` with `dictionary = "idempiere"` now reads the same `AD_Table` flags and the
same `AD_Process` / `AD_Scheduler` processes as Oracle, and the catalog pass produces the same table
`dictionary` flags and the same `AppProcess` objects.

**Built.**
- `ekos_observation_sdk::app_dictionary` (new, pure) holds what both engines share: `DictionaryKind`,
  `TableFlags`, `AppProcess`, the two queries, the row mapping, process-name redaction and the `dictionary`
  artifact (`<scheme>://<database>/<owner>/dictionary`). The queries quote only the owner and leave every
  other identifier unquoted, so the same text runs on both engines (Oracle upper-cases `AD_Table`, PostgreSQL
  lower-cases it). `ora-live::dictionary` re-exports the types and keeps its functions; its query text is
  unchanged (its tests pin it).
- `pg-live::dictionary::read_all`: both queries in one `REPEATABLE READ READ ONLY` snapshot
  (`PgSource::begin_read_only_snapshot` / `end_transaction`, which rolls back), on top of the session's
  `default_transaction_read_only`. A failed query still ends the transaction.
- `ekos-plugin-postgres`: `LiveReader::dictionary`, `read_with(.., dictionary)` (same session as the catalog
  read), `DatabaseReading::dictionaries` (schema → flags and processes); the observer emits one `dictionary`
  artifact per schema. The Oracle plugin emits its artifact through the same SDK function.
- Config: the dictionary is accepted for `oracle` and `postgres` (`check_sources`), refused for the others;
  `ekos database add --dictionary idempiere` and the console's Sources form follow (`sources_write.py`,
  `Sources.tsx` — the choice is shown for PostgreSQL too and is not sent for a kind without one).
- The catalog analyzer needed no change: it was already engine-neutral for dictionary artifacts (tables are
  matched upper-cased, so `c_order` takes `C_Order`'s flags). A test now pins that.

**Tests.** SDK `app_dictionary` 3, `pg-live::dictionary` 3, plugin 1 (one `dictionary` artifact per schema),
analyzer 1 (a PostgreSQL dictionary gives the same flags and processes as Oracle's, ids keyed by engine),
config and `database add` updated. Live (`EKOS_IDEMPIERE_PG_LIVE=1`): `plugins/postgres/tests/dictionary_live.rs`
compares the reader with direct counts and checks the role's write is refused; Oracle's
`the_dictionary_processes_match_a_direct_count` (`ora-live/tests/live.rs`) does the same on Oracle.

**Measured.** The shared iDempiere PostgreSQL has no credential this work could use, so the live run used its
own PostgreSQL 16 (`ekos-live-idem-pg`, 127.0.0.1:56544, database `idempiere`, schema `adempiere`, read-only
role `ekos_ro`): `AD_Table`, `AD_Process` and `AD_Scheduler` with every column and row of the Oracle seed
(dumped through `EKOS_RO`, iDempiere's PostgreSQL types: `numeric`, `varchar`, `char(1)`, `timestamp`), plus
five business tables' structure. The seed has no `AD_Scheduler` rows, so four were planted (three active).
- The reader: 1,056 tables (765 deletable, 718 change-logged) and 337 processes (4 procedures, 333 classes) —
  the same as Oracle's live test on the seed — and 3 active schedules, equal to direct counts.
- End to end (`build` … `commit`, one project per engine, only `[[databases]]`): 337 `AppProcess`es on each;
  name, title, procedure, class, active and origin equal for all 337 (owner `adempiere` vs `ADEMPIERE` and the
  planted schedules differ, as they should); the eight PostgreSQL tables carry the same `dictionary` flags as
  their Oracle namesakes.
- Not measured: a real iDempiere installation on PostgreSQL (the shared `idemseed-pg`), whose rows would
  differ from the Oracle seed's.


# RFC 0257 — The transformation chain: a visual view, comments on a step, and structured edits

**Status:** Accepted (2026-11-11, on the maintainer's instruction to start work); phases 1–2 built on the EKOS side (§9)
**Date:** 2026-11-11
**Depends on:** RFC 0027 / 0028 (Transformation IR), RFC 0195 (gold drafter), RFC 0215 (requirements as mart
metadata), RFC 0228 (marts: propose, merge, build, reconcile), RFC 0247 (annotations), RFC 0248 (terms, triage,
gold changes, hard gates), RFC 0254 (`ekos_probe`), RFC 0198 (console extension pages), RFC 0213 (sign-off)
**Repos:** EKOS (console, `semantic`); ekos-lakehouse (`gold`, `lakehouse-ext`)

---

## 1. Motivation

The first user of the workplace is an analyst. An analyst does not want to hide the transformation: they want to
see it, check it, and say what is wrong with it.

Today a mart is shown as SQL, a list of terms with fidelity, tests and a lineage graph of silver tables (RFC 0228
§2.1). RFC 0248's review screen shows the SQL and the chain from the dashboard down to the source column. What is
not shown is the **inside** of the transformation: which rows a filter removes, where the join is, what is grouped,
in the order it happens. And RFC 0228 leaves "editing a generated mart in the console" out.

EKOS already has the model for this. Transformation IR (`TransformGraph`) holds a transformation as nodes —
`Source`, `Filter`, `Join`, `Aggregate`, `Calculate`, `Sink`, `Unmapped` — with data-flow edges, parsed from SQL,
views, routines and Kettle files. It is used for recovery and for diffs. It has never been drawn.

## 2. Goals and non-goals

**Goals**

1. Every transformation EKOS knows — a generated mart, a recovered legacy job — drawn as a left-to-right chain of
   steps, from one renderer.
2. On a step: what it does, in words and in source text; its evidence; how many rows pass it.
3. **Say what to change** on a step, as a comment or an instruction (RFC 0247), in free text.
4. **Change it**: a small, closed set of structured edits on a mart, each of which becomes a change to the mart's
   requirement and goes through the gates that exist.
5. An assistant that turns free text on a step into a proposed structured edit. It proposes; the gates decide.

**Non-goals**

- A general ETL designer: no blank canvas, no pipeline built by dragging boxes. A mart starts from a requirement
  or a question, as today.
- Editing SQL. SQL stays compiled from terms (RFC 0248 §2.1: the model does not write SQL; here, neither does the
  user). The SQL is shown, read-only, beside the chain.
- Editing bronze or silver. They are generated from the source model; their corrections are step 3 and step 5
  decisions.
- Editing a recovered legacy transformation. It is evidence of what the old system does.

## 3. Design

### 3.1 One graph for every transformation

| Transformation | Where its graph comes from |
|---|---|
| A view, routine, Kettle job | its `TransformGraph`, already in the ledger |
| A generated mart | the compiled dbt model's SQL, parsed by the same `sql-select` front end into a `TransformGraph` |

Parsing the mart's own SQL, instead of drawing its terms, is deliberate: the picture is of what will run. Each
node is then joined back to the term it was compiled from (RFC 0195 knows which term produced which clause), so a
node carries the term, its resolution rule and its fidelity (`exact` or `drafted`, RFC 0228). A node that maps to
no term, or an `Unmapped` node in a generated mart, is a defect and is shown as one.

`ekos transformation graph --object <id> [--json]` and `lakehouse marts graph --mart <name> [--json]` (class
*read*) return the same shape: nodes (id, type, label, columns in and out, source text, term, fidelity, evidence),
edges, and the node ↔ SQL span map.

### 3.2 The view

The console gains one view kind, `graph`, for extension pages (RFC 0198: "a new kind is a console change,
reviewed like any other"): pointers to a node array and an edge array, a layered left-to-right layout, a badge
per node. It is data only, as every view kind is. The intent is to use the layered mode of the graph library the
console already ships, so that no dependency is added; that is to be confirmed in phase 1.

The page shows the chain, and for the selected step:

- **What it does** — a sentence from a deterministic template per node type ("keeps rows where `docstatus` is one
  of CO, CL"), and the source text beneath it;
- **Columns** in and out, each linked to its lineage down to the source column;
- **Evidence and fidelity** — the term, the rule that resolved it, `exact` or `drafted`;
- **Rows** (§3.3);
- **Annotations** on the step, and the box to add one.

Selecting a step highlights its span in the SQL panel, and the reverse.

### 3.3 Row counts per step

The most useful fact about a step is how much it changes: 1,204,330 rows in, 1,180,112 out. For a mart, each
step's count is one aggregate query on Trino over the chain up to that step, run on request under RFC 0254's probe
rules: aggregates only, budgeted, small-cell suppression, never persisted. A join shows rows in from each side,
rows out, and unmatched rows on each side — where fan-out and silent row loss become visible.

Counts are not available for recovered legacy transformations (their source may not be runnable) and the page
says so.

### 3.4 Saying what to change

An annotation on a step is an RFC 0247 record whose target is the step's term (for a mart) or the node (for a
recovered transformation). Nothing new is stored: comments are for people; an instruction from the closed
vocabulary (`default_filter`, `preferred_join`, `unit`, `exclude`, `fiscal_year_start`) applies to later
resolutions; free text reaches a model only as data. This is the first phase, and it is useful on its own: the
analyst marks the step, writes what is wrong, and the mart's author — or §3.6 — acts on it.

### 3.5 Structured edits

A mart is edited by changing its **requirement** (RFC 0215), never its SQL. The edits are a closed set:

| Edit | On | Constraint |
|---|---|---|
| add, change or remove a filter | `Filter`, or a `Source` | column and values resolve; the predicate is from the instruction vocabulary |
| change a join's path | `Join` | only along declared or confirmed keys |
| add or remove a dimension | `Aggregate` | resolves; the grain change is stated |
| add or remove a measure | `Aggregate` | an additive measure from the closed aggregate list; it gets its reconciliation test |
| add or change a calculated column | `Calculate` | arithmetic over resolved terms only |

Each edit:

1. is recorded as a change to the requirement's metadata, with the step it was made on;
2. is classified as RFC 0248 §2.2 classifies a request: additive, or modifying;
3. goes through RFC 0248 §2.3 unchanged — term resolution, zero invented atoms, dbt tests, reconciliation with
   silver, impact on dashboards and annotations, confirmation for a modifying change — and then through RFC 0228's
   propose, merge, build and reconcile;
4. shows a **preview before the proposal**: the chain before and after, and the row counts of §3.3 for both.

An edit that cannot be expressed in the set is refused with the reason, and the user is offered a comment
instead. The set grows by RFC.

A changed requirement invalidates its approval (RFC 0215, 0228: state `requirement`). On a project that requires
sign-off, an edit is therefore a proposal to the approver; the page shows that before the first edit.

### 3.6 The assistant

On a step, the user may write what they want in free text ("exclude cancelled orders here"). The assistant returns
**one proposed structured edit** from §3.5 as schema-checked JSON, or says the request is not expressible. It
receives the step, its columns with their value lists and literal coverage (no row values), and the annotations in
force; user text is passed as data. It does not write SQL and cannot bypass a gate: its output enters step 1 of
§3.5 exactly as a hand-made edit does, and the user sees the edit before it is recorded.

When "cancelled" does not resolve — no value list or annotation says which codes mean cancelled — the result is a
question to the user, not a guess (the RFC 0248 rule), and the answer may be saved as a synonym.

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | Graph export for recovered transformations and marts; the `graph` view kind; the step panel; SQL span map | on iDempiere: every approved mart and three recovered routines render; every mart node maps to a term; a mart with a planted `Unmapped` node is flagged |
| 2 | Annotations on a step | an instruction added on a step changes the next resolution of that term, and is shown on the step |
| 3 | Row counts per step | counts for a mart's steps equal the same aggregates run by hand; a PII column and a small cell are suppressed |
| 4 | Structured edits with preview | each edit kind taken from edit to reconciled mart in a browser run; a modifying edit that fails a gate cannot be merged |
| 5 | The assistant | on a fixed set of 20 written change requests: share turned into the correct edit, share answered with a question, invented atoms = 0 |

Phases 1–3 change nothing and can ship before sign-off of the edit path.

## 5. Tests

- Round trip: a mart's graph, re-rendered to its terms, equals the requirement's terms.
- A join edit along an undeclared, unconfirmed key is refused.
- An edit classified additive that changes the grain of an existing mart is reclassified modifying.
- The assistant's output that fails the schema is dropped and reported; it is never applied.
- A restricted reader sees no step whose source is out of scope.

## 6. Open questions

1. Does RFC 0195 keep the term → SQL clause map today, or must the drafter start recording it? Phase 1 depends on
   it.
2. Row counts per step cost one Trino query each. Run on selection, or all at once behind one button with a
   stated budget?
3. Should a recovered legacy transformation and the mart that replaces it be shown side by side (the
   `ekos_transformation_diff` result, drawn)? It fits the migration use case and is a natural phase 6.
4. Large graphs: a routine with hundreds of nodes. Collapse by statement, with expansion on selection?

---

## 9. Implementation log

### 2026-11-11 — Phases 1 and 2, EKOS side (branch `rfc0257`)

**Scope.** The recovered-transformation half of phase 1 and phase 2. The mart half (`lakehouse marts graph`, the
term → node join, a mart with a planted `Unmapped` node, every approved mart rendering) lives in ekos-lakehouse and
is not built here. Phases 3–5 are not started.

#### What was built

| Piece | Where | What |
|---|---|---|
| Owner and statement on IR nodes | `recovery/src/sql_transform_analyzer.rs` (pass version `v4`) | every node of a `CREATE VIEW` / `CREATE FUNCTION` / `CREATE PROCEDURE` statement records `owner`, `owner_kind` (`view` / `routine`), `owner_ordinal` (which same-named definition in the file: overloads) and, in a routine, `statement` (the body statement it came from). `parse_sql_to_statement_graphs` keeps them; `parse_sql_to_transform_graphs` is unchanged for its callers |
| Two recovery fixes found on LedgerSMB | same file | `JOIN … ON (a = b)` lost its key pairs (`Expr::Nested` was not unwrapped: every join of `order__search` had none); a view's `Sink` now carries the view's output column names |
| The chain | `semantic/src/transform_chain.rs` (pure) | `select` (a `TransformNode`, `View` or `Procedure` id → its graph; a ledger from before this change still finds a view through its `Sink`), `build` → `ChainGraph`: nodes with the template sentence, columns in / out / used, source text, span, evidence, annotations, `term`/`rule`/`fidelity` = `null`, `status: unmapped`; edges; the span map; groups; counts; `rows.available = false` with the reason |
| CLI | `cli/src/commands/transformation.rs`, catalogue rows `transformation graph` / `transformation list` (class *read*) | `ekos transformation graph --object <id> [--expand all\|1,3] [--json]`, `ekos transformation list [--filter] [--limit] [--all] [--json]`. The definition's text is read from its file by the recorded span through `redaction::read_redacted` (RFC 0043) |
| MCP | `ekos_transformation_graph` in `SCOPED_TOOLS` (and `web/api/app/mcp_policy.py`) | read-only, through the call's scoped store; the definition text is read only for an owning `View`/`Procedure` that store returned |
| Console | `routes/transformations.py` (`read`, not `scoped`), `readproc` shapes, `pages/Transformations.tsx`, `chain-shared.ts`, a "Transformations" tab, "Open as a chain of steps" on an object panel | list → chain (layered, left to right) → step panel (sentence, source text, columns, evidence, notes, the add-a-note form) ↔ source text with every step's span marked; clicking either side selects the step |
| Annotations on a step (phase 2) | `AddForm` exported from `pages/Annotations.tsx` with a fixed target | the step's node id is the target; the write is the existing `POST /workspaces/{id}/annotations` → human-only `ekos annotate add`. The graph document carries each step's current annotations (`annotate list`'s rendering), so the step shows them |

#### The JSON shape, for `lakehouse marts graph`

`ekos.transformation-graph/1`, documented field by field in `semantic/src/transform_chain.rs`'s module docs and
typed in `web/ui/src/pages/chain-shared.ts`. A mart's command prints the same document with `transformation.recovered
= false`, each node's `term` / `rule` / `fidelity` filled, `status: unmapped` on a node mapped to no term (the defect
the RFC asks to flag), and `rows.available` set when §3.3 is built. An extension page can draw it with the existing
RFC 0207 `graph` view kind: `nodes: "/nodes"`, `edges: "/edges"`, `id: "/id"`, `label: "/label"`, `group: "/group"`,
`badge: "/fidelity"`, `from: "/from"`, `to: "/to"` (`drafted` and `unmapped` now read as attention badges).

#### Decisions

- **The view kind (§3.2, to be confirmed in phase 1).** The `graph` view kind already exists (RFC 0207): a
  deterministic layered left-to-right SVG layout (`layoutGraph`: layer = longest path, one lane per group). The chain
  page reuses that layout; no dependency is added. `react-force-graph-2d` also has a `dagMode="lr"`, but it is a
  force simulation on a canvas — positions not reproducible and nothing a jsdom test can read — so it is not used.
- **Open question 4 — large graphs.** Collapse by statement, with expansion on selection. Above 40 nodes a routine's
  body statements are each drawn as one `Statement` node, joined by dashed `order` edges (body order, not data flow),
  and a run of consecutive statements none of which recovery understood is one block. Selecting a statement and
  "show its steps" asks for it in full (`--expand <n>`); "show every step" is `--expand all`. Measured: LedgerSMB's
  `payment_post`, 64 nodes (62 not understood), is drawn as 5.
- **Spans are found, never guessed.** The IR keeps expressions reprinted by `sqlparser`, not their offsets, so a node's
  span is its recorded text located in the definition ignoring case and whitespace runs. A reprinted expression
  (`'CO', 'CL'` for `'CO','CL'`) is not found and the step has no span.
- **Source columns** are the qualified columns other steps name through the read's alias or table name; an unqualified
  column is not placed.
- **Read scope.** The console routes run the CLI unscoped, so they are not `scoped` (a restricted reader is refused);
  the MCP tool is scoped. RFC test (§5) "a restricted reader sees no step whose source is out of scope":
  `transformation::tests::a_restricted_reader_sees_no_step_whose_source_is_out_of_scope`.
- **Open questions 1 and 2** are the mart's (term → clause map) and phase 3's (row counts); nothing was decided here.
- **Phase 2 for recovered transformations.** A comment or instruction on a step is stored and shown; for a recovered
  transformation nothing resolves, so the exit "an instruction changes the next resolution of that term" belongs to
  the mart side. A note goes stale when the step's content changes (RFC 0247 digest), e.g. after a re-recover changes
  the node.

#### Measured (LedgerSMB `sql/`, scratch project, `postgres`, no model)

Recover found 1,442 transformations with a graph that recovery understood something of (753 standalone statements
of which it understood nothing are not listed by default): 17 views, 559 routines, 866 queries. Every view and
routine was rendered with `ekos transformation graph --expand all --json`, 576 calls, 0 failures, median 3.6 s
(debug build, the whole ledger read per call).

| | graphs | definition text found | steps understood | of them with a span | not understood |
|---|---|---|---|---|---|
| views | 17 | 17 | 59 | 58 (98%) | 11 |
| routines | 559 | 558 | 961 | 911 (95%) | 2,201 (70% of routine steps) |

Spans by step type (routines): Source 409/410, Join 162/164, Filter 201/217, Calculate 120/146, Aggregate 19/24 — the
misses are expressions `sqlparser` reprinted differently from the file. 258 of the 559 routines are wholly "not
understood" (PL/pgSQL bodies); 217 are wholly understood (mostly `LANGUAGE sql`).

The three rendered for the exit, from `sql/modules/`:

- view `file_order_links` (Files.sql) — 7 steps, all understood, 7/7 spans: reads `file_secondary_attachment` as
  `sl`, joins `oe` where `sl.ref_key = oe.id`, joins `oe_class`, "keeps rows where `sl.source_class` is `2`",
  defines the view with its 7 output columns;
- routine `order__search` (OrderEntry.sql, `LANGUAGE sql`) — 17 steps, all understood, 16/17 spans: six joins (two
  inner, four left, keys recovered after the `ON (…)` fix), the search filter, three calculated columns;
- routine `payment_post` (Payment.sql, PL/pgSQL) — 64 steps, 62 not understood, drawn collapsed as 5 nodes
  (statements 1–24, a Source, 26–29, a Calculate, 31–64).

Phase 2: `ekos annotate add 057d18cb… comment "source_class 2 is orders; should quotations (class 3/4) be here
too?" --as rfc0257-test` on the Filter step of `file_order_links`; the next `ekos transformation graph` shows it on
that step (`active`), and the MCP tool `ekos_transformation_graph` over stdio returns the same document with the
annotation.

#### Not done

- The mart side of phases 1 and 2 (ekos-lakehouse): `lakehouse marts graph`, node ↔ term map, planted-`Unmapped`
  check, every approved mart on iDempiere.
- Phases 3 (row counts), 4 (structured edits), 5 (the assistant); open question 3 (side by side).
- A PL/pgSQL body is split by the RFC 0027 SQL front end and is mostly "not understood" (see the counts above);
  RFC 0163's procedural IR describes these routines statement by statement but is not lowered into the
  Transformation IR (RFC 0164). Drawing routines from it is the obvious next step.
- Columns linked to their lineage down to the source column (§3.2) — columns are shown, not linked.
- The console page was verified by its vitest suite, not in a browser.

### 2026-11-12 — Phases 1–5, mart side (ekos-lakehouse, worktree `work-0257`, uncommitted)

**Scope.** Everything above that lives in ekos-lakehouse: the mart's chain (phase 1), instructions on a mart's
step (phase 2), row counts (phase 3), structured edits with preview and gates (phase 4), the assistant (phase 5).
No EKOS source changed: the console's existing RFC 0207 `graph`/`table` views and RFC 0226 action inputs were
enough, and a mart's document is the `ekos.transformation-graph/1` built by `transform_chain::build` itself.

#### What was built

| Piece | Where (ekos-lakehouse) | What |
|---|---|---|
| Term → clause map (open question 1) | `gold/src/lib.rs` `Clause`, `Compiled::clauses` | `compile` records, from the inputs it writes the SQL from, every clause and the terms it serves: each `source` (a table read for the terms on it and for those reached through it), each `join`, each `filter`, each `dimension`/`measure` expression with its output name. RFC 0195 did **not** keep this before; the drafter now does |
| The mart's chain | `lakehouse-ext/src/mart_graph.rs`, `marts.rs::chain` | the committed dbt model (else the proposal branch's), refs replaced by model names, parsed by the `sql-select` front end, lowered and drawn by `transform_chain`; `recovered = false`, `kind = mart`, `key = <model path>@committed\|proposed\|preview`; each step joined to its terms **by what it says** (normalised SQL text, never position); `status: unmapped` and the reason in the sentence for a step no term produced; a `badge` (fidelity, else status) and `defects` added for the console |
| Two IR changes before drawing | `mart_graph.rs` | a `WHERE` of `AND`ed predicates is one `Filter` step per predicate (one term each; each removes its own rows); the mart is the last step, a `Sink` mapped to the grain |
| CLI | `lakehouse marts graph --mart <m> [--proposed] [--json]` (*read*), `marts rows` (*external*), `marts edit` (*review*), `marts suggest` (*external*, may call a model) | catalogue rows in `command_classes` |
| Console | `console.rs` Marts page | panels **Steps** (RFC 0207 `graph` view over `/nodes`, `/edges`, badge `/badge`) and **What each step does** (table: step, kind, sentence, term, rule, fidelity); actions **Count rows per step**, **Preview an edit**, **Make the edit** (with a consent sentence), **Ask the assistant** |
| Instructions on a step (phase 2) | `gold::Instruction`, `gold::resolve_with`, `marts.rs::notes` | RFC 0247 annotations on a silver `Table` object of the project's ledger; a **shared, approved, still-valid** `default_filter`/`exclude` becomes filter terms of the description's own grammar on every mart reading that table, so it resolves, compiles, is checked for invention and reconciles like a written filter (`how` says `instruction by <author> (annotation <id>)`). Comments, personal and stale instructions are shown on the steps reading the table, never applied. Used by `graph`, `show`, `propose` (`gold_cmd::draft`) and `reconcile` |
| Row counts (phase 3) | `lakehouse-ext/src/mart_rows.rs` | one aggregate per count over the chain up to the step (a join: rows out, matched → unmatched left, unmatched right; fan-out flagged); RFC 0254's rules: counts only, budget 60 queries per request, cells under `[probe] min-cell` (5) hidden, a filter removing a small cell shown as `<in> − <5` and that display carried to every later step, a step reading a personal column (plan `pii_candidate` or the shared `ekos_common::pii` name rules) withheld with everything after it, nothing stored |
| Structured edits (phase 4) | `gold/src/edits.rs` (pure), `lakehouse-ext/src/mart_edit.rs` | the closed set `add_filter`, `change_filter`, `drop_filter`, `change_join`, `add_dimension`, `drop_dimension`, `add_measure`, `drop_measure` (JSON with `deny_unknown_fields`); the step must be of the kind §3.5 names; classified additive (a new column) or modifying, a grain change reclassifies; static gates (resolution, the edit's constraint, invention), engine gates (Trino plans it, grain unique, every additive measure reconciles — over the edited SQL, never a stored table); preview = the chain before and after with counts; recorded only when every gate passes, and a modifying edit only with `--confirm`, as a requirement-log line naming the step — which lapses the approval |
| The assistant (phase 5) | `gold/src/assist.rs` (pure), `lakehouse-ext/src/mart_assist.rs` | prompt = requirement, the step, the mart's tables' columns with value lists (codes only) and foreign keys, annotations in force and the request as delimited data; the answer must be one JSON object (`edit` / `question` / `not_expressible`) or it is dropped and reported; columns and values are grounded (a value no list has, or a column the tables lack = invented → asked back; a listed code the person did not write = a meaning chosen by the model → asked back); the edit is then run through `apply` and the static gates and shown as an edit only if they would pass. Local Ollama only: a non-loopback URL is refused |
| Fixture | `lakehouse-ext/tests/fixtures/rfc0257_change_requests.json` | the 20 written change requests with expected answers |

#### Decisions

- **Open question 1** — the drafter did not keep the map; it does now (`Compiled::clauses`), and steps are joined to
  it by text, so a hand edit of the committed model shows as `unmapped`, never silently mapped.
- **Open question 2** — all steps at once behind one button (`marts rows`, the *Count rows per step* action), with a
  stated budget of 60 queries; on selection would hide the join counts the button exists for.
- **The graph view.** The extension page uses the existing RFC 0207 `graph` view; the step panel is a table beside
  it. The EKOS Transformations page (span highlighting, notes form) reads `ekos transformation graph` only, so a
  mart is not drawn there.
- **Where an instruction on a mart's step lives.** On the silver table's `Table` object in the project ledger: RFC
  0247 type-checks a rule against its target's columns, and a term is not a ledger entity. Only predicate rules
  apply to marts (`default_filter`, `exclude`); `unit`, `fiscal_year_start` and `preferred_join` are shown, not
  applied (`unit` would also have to change reconciliation).
- **Gates of a preview** reconcile the edited SQL against silver under its own joins and filters (RFC 0228's query,
  its gold table replaced by the SQL). A failing gate, or a modifying edit without confirmation, records nothing;
  so a failing modifying edit never reaches approve, propose or merge.
- **Calculated columns (§3.5 row 5) are not in the set**: gold has no arithmetic term kind outside model-drafted
  prose, and an edit must not depend on a model. Refused with the reason, like anything outside the set.
- **The assistant never shows an edit the gates would refuse**: llama3 answered "a 3-month moving average" with
  `add_measure` "avg of count of c_order over 3 months"; it is now turned into a question / not-expressible before
  the person sees it.

#### Measured (iDempiere marts from `ekos-lakehouse-oracle`, copied into scratch projects)

- **Phase 1.** All 11 committed marts render (7 iDempiere: `pipeline/` 4, `pipeline-sales/` 3; Pagila 4): 3–18 steps,
  **0 steps mapped to no term**, every step with a span except the added `Sink`. Planted on `sales_by_month` (a
  committed hand edit): a predicate (`ISACTIVE = true`) and a column (`max(DOCUMENTNO)`) → `2 mapped to no term`,
  both named in the sentence. Unit tests: `mart_graph::tests::{every_step_of_a_generated_mart_maps_to_a_term,
  a_planted_step_no_term_produced_is_flagged}`, `gold every_clause_of_a_compiled_mart_names_its_terms`.
- **Phase 2.** On the scratch project's ledger (1,836 `Table` objects after build → commit, debug, ~45 min):
  `ekos annotate add <C_INVOICE> instruction … --rule {default_filter POSTED eq true} --shared` → shown on the
  `Source` step of `invoices_by_month`, not applied; after `ekos annotate approve` the next resolution (`marts
  graph --proposed`) has a new step 3 `Filter C_INVOICE.POSTED = true`, mapped, with the instruction shown on it.
  Found: a dbt-recovered silver `Table` lists only the columns its `bronze_sources.yml` tests name, so a rule on
  `DOCSTATUS` was refused by RFC 0247's type check ("has no column DOCSTATUS"). Tests:
  `marts::note_tests::*`, `gold an_instruction_on_a_table_adds_its_filter_terms_to_the_marts_reading_it`.
- **Phase 3.** Own Trino (`ekos-0257-trino`, memory catalog with the 9 silver tables built from the silver dbt models
  over the shared Oracle XE through a read-only `oracle` catalog): hand-written aggregates for `sales_by_month` and
  `category_region_sales` vs `marts rows`: **13/13 consistent** (8 exact equal; 5 hidden, each hand count inside
  the shown range). The seed is small (8 orders), so most counts after the filters are `<5`. A planted filter on
  `C_LOCATION.ADDRESS1` withholds that step and the four after it. Tests: `mart_rows::tests::*`.
- **Phase 4.** Browser run (Playwright, the console from this worktree, `EKOS_BIN` = the lakehouse binary): all 8
  kinds on `sales_by_month`, each preview → edit → approve (Requirements page) → propose → merge → build → reconcile:
  **8/8 reached a reconciled mart** (every measure `equal`), every gate passed. A modifying `change_join` via
  `c_order.docstatus` (not a key) was refused, nothing recorded, and Merge then failed (no proposal). **Build ran
  through dbt-trino against the scratch Trino, not Dagster** (no Dagster here), so the marts' state shows `merged`,
  not `reconciled` (no `run-report`). Screenshots: `.local-0257/shots/` (52). The preview is the run's text output
  (gates, both chains with counts), not two drawn chains. Tests: `gold step_edits::*` (incl. RFC §5's
  undeclared-key and grain-reclassification tests), `mart_edit::tests::*`.
- **Phase 5.** The 20 written requests (13 edits, 4 questions, 3 not expressible), local Ollama only, an isolated
  copy of the pipeline (`.local-0257/assist_eval.py`; a first run shared its project with the browser run and is
  discarded as contaminated). One call 40–175 s on this machine (GTX 1650, 4 GB).

  | | llama3 (8B) | qwen2.5-coder:3b |
  |---|---|---|
  | correct edit | 9/13 | 7/13 |
  | question when the request names a meaning no list states | 4/4 | 4/4 |
  | "not expressible" when it is not | 0/3 (2 asked back, 1 dropped) | 0/3 |
  | all | **13/20** | **11/20** |
  | answers outside the schema, dropped and reported | 1 | 1 |
  | atoms the model invented | 5 | 4 |
  | invented atoms shown to the person | **0** | **0** |
  | model guesses turned into questions by grounding | 7 | 9 |
  | wrong edits shown | 1 | 2 |

  Of the 3 wrong edits shown, 2 were on the wrong kind of step (a filter removal answered on the aggregate step);
  after the runs these are turned into a question as well (`mart_assist::suggest`: `marts edit` would refuse them),
  which leaves 1 (qwen: "drop the sales-transaction condition" → drop the `docstatus` filter, a valid edit of the
  wrong filter, which the preview and the person must catch). Not re-run after that change. Neither model ever says
  "not expressible"; the grounding turns their attempts into questions instead, so the share of correct edits is
  the number that matters and it is modest on small local models. Tests: `gold assistant::*` (schema drop, the
  "cancelled" question, gate-refused edits), `mart_assist::tests::only_a_local_model_is_called`.

#### Not done

- The preview drawn as two chains side by side; the step's annotations form on the Marts page (an instruction is
  written with `ekos annotate add` or the Notes tab on the table).
- Marking a committed mart that an approved instruction would now change (the committed chain shows the note on the
  source step but no "pending" flag).
- Calculated-column edits; `unit` / `fiscal_year_start` / `preferred_join` applied to marts.
- A dbt-recovered silver table's columns beyond those its YAML tests name (limits RFC 0247 rules, phase 2 above).
- Open question 3 (legacy and mart side by side).

# RFC 0257 — The transformation chain: a visual view, comments on a step, and structured edits

**Status:** Accepted (2026-11-11, on the maintainer's instruction to start work)
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

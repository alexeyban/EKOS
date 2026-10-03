# RFC 0169 — Views as first-class ledger objects

**Status:** Accepted (2026-10-03)
**Date:** 2026-10-03
**Supersedes:** none
**Related:** RFC 0027 (Transformation IR — a view's *logic*), RFC 0075 (data lineage), RFC 0163
(procedural IR; `procedure_lineage`, `plpgsql_footprint`), RFC 0135 Part D (custom-kind registry),
RFC 0146 (PostgreSQL dialect)

---

## Summary

A `CREATE [OR REPLACE] [MATERIALIZED] VIEW` in an observed SQL file becomes a `Custom("View")`
object: its name, declared columns, whether it is materialized, the exact source text and line, and
what its query **reads** and **calls**, from a real parse. Views join the name space the existing
linkers resolve against, so a routine or a transformation that reads a view links to it, and a view
links to the tables and views it is built on (`DependsOn`), so impact analysis on a table reaches the
views over it.

## Motivation

EKOS has no object for a view. `sql_transform_analyzer` lowers a view's query into Transformation IR
nodes whose `Sink` carries the view name as a string — the view's logic exists, the view does not.
Everything that names a view therefore dead-ends:

- **RFC 0163's linker** could not resolve 23 references from LedgerSMB routines to its views
  (`account_heading_tree`, `periods`, `cash_impact`, …) — the largest class of unlinkable names
  (devlog_230).
- **RFC 0075's data lineage** cannot link a view's own `Sink` node, or any `Source` node reading a
  view, for the same reason.
- **Impact analysis** on a table stops at the table: "what breaks if I change `acc_trans`" cannot
  reach the views over it, nor anything built on those views.

## Design

### Recovery — `ViewAnalyzerPass`

One pass per `.sql` file, deterministic, no LLM, in `crates/recovery/src/view_analyzer.rs`.

1. **Split the file into statements** on `ekos-plpgsql`'s lexer tokens (the same splitter
   `routines()` uses, generalised as `statements()`), so a semicolon inside a string, comment or
   dollar quote never splits a statement, and each statement keeps its byte offset → line.
2. **Select view definitions** by their leading tokens: `CREATE [OR REPLACE] [TEMP|TEMPORARY]
   [MATERIALIZED] [RECURSIVE] VIEW`.
3. **Parse each one on its own** with the file's resolved `sqlparser` dialect (RFC 0031/0039 — the
   same resolution the other SQL passes get). One unparseable view costs that view's footprint,
   never the file.
4. **Emit a `View`** for every definition, parsed or not. The name comes from the AST when it
   parsed, from the tokens after `VIEW` when it did not — a view is never dropped because its body
   uses syntax `sqlparser` lacks. Properties:

| Property | Meaning |
|---|---|
| `materialized`, `temporary`, `or_replace` | From the statement |
| `columns` | The declared column list, when the statement has one |
| `reads`, `calls` | The query's footprint (`plpgsql_footprint`, generalised to any dialect): relations read minus CTE names; functions called, table functions included |
| `footprint` | `parsed` or `unparsed`, with `footprint_errors` when unparsed |
| `source_path`, `line`, `span_start`, `span_end` | Where the definition is |

Evidence is the definition's source text (capped, span always exact).

### Identity — keyed by file and name

`View` is **structurally keyed**: `(source path, lower-cased qualified name)`, one REGISTRY row,
`structurally_keyed: true`. A view is a definition in a file, exactly like a routine (RFC 0163), and
gets the same treatment.

The alternative — key by name alone, as `Table` is — was rejected on evidence: when two files define
the same name, the CKM carries two objects with one id (`SEM002 duplicate object id`) and the ledger
flips between them on every commit (devlog_229/230 observed this for three LedgerSMB tables). With
file keys, a view redefined across files is several objects, and a bare reference to it is
**ambiguous** and is not linked — the same honest outcome redefined routines already get. In
LedgerSMB that is 3 of 16 views. A later definition in the *same* file replaces the earlier one
(`CREATE OR REPLACE` semantics, as for routines).

### Linking

No new linker. The two existing whole-graph linkers learn about views:

- **`semantic::procedure_lineage`** — `View` joins the relation index tables are resolved against
  (so `ReadsFrom`/`DependsOn` from a statement or routine can target a view), and each `View` is
  linked like a routine from its own footprint: `DependsOn` view → table/view (`access: "read"`),
  `Calls` view → routine. Unique names only, one edge per id, unchanged rules.
- **`semantic::data_lineage`** (RFC 0075) — `View` joins `Table`/`Dataset` as a link target, so a
  `TransformNode` `Sink` named after a view gets its `WritesTo`, and a `Source` reading one gets
  `ReadsFrom`.

### Scope

In: views defined in `.sql` files, any dialect the registry resolves. Out, and stated:

- Views introspected live (RFC 0157's catalog) — the catalog is not written to the ledger today.
- dbt models materialized as views — `dbt_analyzer` already emits them as `Table`s (RFC 0117).
- Downstream registries (docs-gen pages / API grouping, `llm_description`, `doc_links`) — a
  follow-up, as for `Procedure` (RFC 0147's list; not CI-enforced).
- Column-level lineage through a view.

## Testing

- Statement splitting finds every view form (`OR REPLACE`, `MATERIALIZED`, `TEMP`, `RECURSIVE`)
  with exact offsets, and nothing that is not a view (`COMMENT ON VIEW`, `DROP VIEW`, a view name in
  a string).
- A parsed view records columns, `materialized`, reads minus CTE names, and calls; an unparseable
  one is still emitted, named from tokens, with `footprint: unparsed` and the parser's error.
- Keys: two files defining one view → two objects; one file defining it twice → one, the later.
- Linking: a routine reading a view links to it; a view links to its tables (`DependsOn`); a view
  defined in two files is not linked by bare name; the RFC 0075 `Sink` of a view links to it.
- Determinism: same input, same ids (objects, evidence, edges); a second `commit` writes nothing.
- REGISTRY guard: `every_pipeline_custom_kind_is_registered` covers `View`.
- Corpus: on LedgerSMB, every view definition becomes a `View`, and the view names devlog_230 left
  unresolved resolve, except the redefined ones.

## Alternatives considered

- **A built-in `ObjectKind::View` variant.** Cleaner at the type level, but every exhaustive match
  on `ObjectKind` across the workspace changes, and `Custom` + REGISTRY is the documented extension
  path (RFC 0135 Part D) that the identity guard already enforces. Both serialize to `"View"`, so a
  later promotion stays wire-compatible.
- **Treat views as `Table`s.** Simplest for linkers, but a view is not storage: docs, coverage and
  migration (RFC 0154: a view is *logic* to migrate, not rows to move) all need to tell them apart.
- **Emit the `View` from `SqlTransformAnalyzerPass`.** It already sees `CREATE VIEW`, but it gets
  the dialect-*preprocessed* file (RFC 0146) — so no exact line — and loses the whole file to the
  per-fragment fallback when any statement fails. A view must not depend on its neighbours parsing.
- **Key by name alone, like `Table`.** Rejected above, on observed ledger churn.

## Acceptance criteria

- [x] `Custom("View")` emitted for every view definition in an observed `.sql` file, with a REGISTRY
      row (`structurally_keyed: true`). (devlog_231)
- [x] Views are link targets for `procedure_lineage` and `data_lineage`, and link to their own
      dependencies; unique names only. (devlog_231)
- [x] On LedgerSMB: every definition is a `View` (17, all parsed); 24 previously unresolved view
      references link, the one left is `cash_impact`, defined in two files; a second `commit` writes
      nothing new. (devlog_231)

## Implementation note (2026-10-03)

Adding `View` objects surfaced the first identity conflict involving them: LedgerSMB's view
`employee_search` and two routines named `employee_search`. SQL keeps relations and routines in
separate namespaces, so `identity` gained `is_expected_view_routine_pair`, narrowing — never widening
— the conflict detector for exactly `{View, Procedure}`. `View` beside `Table` still conflicts
(shared relation namespace), and `Table` beside `Procedure` is not excluded until observed.

## Downstream integration (2026-10-03, devlog_232)

`Procedure` and `View` now reach every downstream consumer RFC 0147 lists: curated docs-gen entity
pages (`View`s also in Data Stores, `Procedure`s in API.md under their `File`), documentation
coverage, `doc_links` (a backticked routine/view name in a doc links to it) and `llm_description`
(symbol scope). Each object hangs off its `File` (`Contains`) and carries `source_span`, without
which `llm_description` would skip it silently — a test builds them through the real analyzers and
asserts they are described. `COMMENT ON FUNCTION|PROCEDURE|[MATERIALIZED] VIEW` in the same file
becomes the object's evidence-backed `description` (LedgerSMB: all 330 function comments and every
view comment on an observed file attach), matched by name and then arity, never guessed.

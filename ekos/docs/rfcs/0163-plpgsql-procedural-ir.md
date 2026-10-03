# RFC 0163 — PL/pgSQL → a procedural IR, deterministically

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (the coverage requirement this RFC exists to satisfy), RFC 0027 (Transformation
IR — extended, not replaced), RFC 0028 (`ekos_transformation_explain`), RFC 0146 (PostgreSQL
dialect), RFC 0148/0150 (the structure-then-statements precedent), RFC 0164 (the only consumer that
may involve an LLM), RFC 0031 (`SqlDialectParser`)

---

## Summary

A real, in-process, deterministic PL/pgSQL parser producing a **procedural IR** — statements,
control flow, exception handling, cursors and variable assignment — with each embedded SQL statement
lowered into the existing Transformation IR. No LLM anywhere in this RFC.

Every recovered object carries a **fidelity label** stating what was actually recovered, and nothing
is labelled higher than it earned.

## Motivation

`crates/recovery/src/sql_transform_analyzer.rs` is candid about the present state. A
`CREATE FUNCTION … AS $$…$$` body is a single opaque string literal to `sqlparser` (lines 650–657);
procedure bodies fail whole-file structured parsing (lines 13–17); the result is partial and
duplicate `Unmapped` fragments, because "the procedure's control flow was never going to be modeled"
(lines 310–312).

For a documentation product that is an acceptable limit. For a migration it is disqualifying, in a
specific and dangerous way: RFC 0164 wants to check that generated logic invents nothing by
requiring every generated predicate to map to a source IR node. Against `Unmapped`, **everything
maps**. The check passes on invented logic and the system reports green — the exact failure the
whole of EKOS Migrate exists to prevent.

RFC 0148 hit this wall for compiled binaries and RFC 0150 broke through it the same way: write a
real in-process decoder first, label fidelity honestly, and only then let a model near the output.
This RFC is that decoder for PL/pgSQL. It is also useful on its own — `ekos_transformation_explain`
over a stored procedure is valuable whether or not anything is ever migrated.

## Design

### Why a second IR, not more `TransformNode` variants

The existing `TransformNode` (`crates/semantic/src/transform_ir.rs`) is a **dataflow** graph:
`Source`, `Filter`, `Join`, `Aggregate`, `Calculate`, `Sink`, `Unmapped`. Pentaho and SQL both lower
into it, and that shared shape is why RFC 0027 works.

PL/pgSQL is **imperative**: ordered effects, conditions, loops, early returns, exceptions. Forcing
control flow into a dataflow graph would either lose the ordering (making the IR wrong) or
contaminate every existing consumer with node kinds that mean nothing for a Pentaho transformation.

So the procedural IR is additive and layered, following the convention CLAUDE.md records for the
simulation crate — build on existing primitives rather than replacing them:

```rust
/// crates/semantic/src/procedure_ir.rs
pub struct ProcedureIr {
    pub name: String,
    pub language: ProcLanguage,          // PlPgSql | Sql | Other(String)
    pub signature: ProcSignature,        // args with modes, return type, volatility, strictness
    pub declarations: Vec<VarDecl>,
    pub body: Vec<ProcStmt>,
    pub fidelity: Fidelity,
}

pub enum ProcStmt {
    /// An embedded SQL statement, lowered into the EXISTING Transformation IR.
    Sql { graph: TransformGraph, into: Option<Vec<String>>, span: Span },
    Assign   { target: String, expr: String, span: Span },
    If       { branches: Vec<(String, Vec<ProcStmt>)>, else_branch: Option<Vec<ProcStmt>>, span: Span },
    Case     { operand: Option<String>, branches: Vec<(String, Vec<ProcStmt>)>, else_branch: Option<Vec<ProcStmt>>, span: Span },
    Loop     { kind: LoopKind, body: Vec<ProcStmt>, label: Option<String>, span: Span },
    Exit     { label: Option<String>, when: Option<String>, span: Span },
    Return   { value: Option<String>, query: Option<TransformGraph>, next: bool, span: Span },
    Raise    { level: String, message: String, span: Span },
    Block    { declarations: Vec<VarDecl>, body: Vec<ProcStmt>, exception: Vec<ExceptionHandler>, span: Span },
    Cursor   { op: CursorOp, name: String, query: Option<TransformGraph>, span: Span },
    Perform  { graph: TransformGraph, span: Span },
    /// Dynamic SQL. The IR records that a statement is CONSTRUCTED, and from what.
    DynamicExecute { expr: String, using: Vec<String>, into: Option<Vec<String>>, span: Span },
    /// Parsed position known, semantics not recovered. Never silently omitted.
    Unrecovered { raw: String, reason: String, span: Span },
}

pub enum LoopKind {
    Plain,
    While   { condition: String },
    ForRange{ var: String, from: String, to: String, by: Option<String>, reverse: bool },
    ForQuery{ var: String, graph: TransformGraph },
    ForEach { var: String, array: String, slice: Option<u32> },
}
```

`ProcStmt::Sql` holding a `TransformGraph` is the load-bearing join between the two IRs: the
procedural layer owns *order and condition*, the existing dataflow layer owns *what each statement
reads and writes*. Everything already built on `TransformNode` — `ekos_transformation_explain`,
`ekos_transformation_diff`, the dbt emitter — keeps working unchanged and gains procedure bodies for
free.

`Span` carries byte offsets into the original body, so every recovered statement cites its source
text exactly the way RFC 0150 cites IL offsets. A claim about behaviour that cannot point at a span
is not a claim this system makes.

### Parsing

**Dollar-quoting first.** `$$ … $$`, `$tag$ … $tag$`, nesting, and dollar quotes inside string
literals and comments. This is a lexing problem `sqlparser` does not solve and it must be solved
before anything else — every downstream stage depends on knowing where the body actually ends.
`ekos_sql_dialect_sdk::lex` is the existing home for this kind of pre-tokenization.

**A hand-written recursive-descent parser** for the PL/pgSQL statement grammar. It is small, the
grammar is stable and documented, and the alternative — shelling out to `plpgsql_check` or a
PostgreSQL server — is excluded by the no-shell-out precedent RFC 0147 established and by the
requirement that recovery be deterministic and offline.

**Embedded SQL is handed to the existing dialect parser** (RFC 0031/0146) and lowered by the
existing `sql_transform_analyzer` machinery. PL/pgSQL variable references inside those statements
are substituted with typed placeholders before parsing, so `WHERE id = v_customer_id` parses as SQL
and the binding is recorded rather than lost.

**Recovery is local.** A statement that fails to parse becomes one `ProcStmt::Unrecovered` with its
span and reason; the parser resynchronizes at the next statement boundary and continues. This is the
direct fix for the current "several partial/duplicate `Unmapped` fragments" behaviour: one
unrecovered statement costs one statement, not the whole procedure.

### Fidelity labels

```rust
pub enum Fidelity {
    /// Signature, language, volatility. Body not parsed (e.g. C, PL/Python).
    Signature,
    /// Body parsed; some statements are Unrecovered. Carries the exact count and spans.
    Partial { recovered: usize, unrecovered: usize },
    /// Every statement recovered; every embedded SQL statement lowered without Unmapped nodes.
    Statements,
}
```

Rules, and they are the part that matters most:

1. **Nothing is labelled `Statements` that contains a single `Unrecovered` node or a single
   `Unmapped` node in an embedded graph.** Asserted by a constructor invariant, not by convention.
2. `Partial` always carries the counts, so a consumer can decide for itself and a report can state
   "41 of 44 statements recovered" rather than "recovered".
3. `DynamicExecute` does **not** reduce fidelity — it is a faithful recovery of a construct whose
   target genuinely is not statically known. It is labelled as a boundary, and RFC 0164 treats it as
   one.

This mirrors RFC 0150's levels exactly, including the discipline that the label is computed from the
IR rather than asserted by the producer.

### Triggers

Trigger functions parse as ordinary PL/pgSQL. The **trigger** itself — the `CREATE TRIGGER` binding
of timing, event, level and condition — is recovered as its own fact, and classified by what the
function's IR actually does: `Audit` (writes to a log-shaped table only), `DerivedColumn` (assigns
to `NEW.*`), `Validation` (raises on a condition), `Cascade` (writes to other tables), or `Mixed`.

Classification is structural — derived from the IR, not from a model's reading of it — and is
deliberately conservative: anything not clearly in one class is `Mixed`, which RFC 0164 treats as
requiring a human. No trigger is ever auto-translated; neither ClickHouse nor Delta enforces them,
and a translation would silently change *when* logic runs.

### Where it runs

A `PlPgSqlAnalyzerPass` in `crates/recovery`, deterministic and side-effect-free like every other
recovery pass. It consumes the function bodies RFC 0157's catalog introspection and RFC 0146's file
parsing already surface, and emits `Custom("Procedure")` and `Custom("ProcedureStatement")` objects
plus `Calls` edges from procedure to the tables, functions and procedures it touches.

Both kinds are structurally keyed — `(source path or catalog oid, qualified name)` and
`(procedure, statement index)` — so both get `structurally_keyed: true` rows in
`ekos_kir::custom_kinds::REGISTRY`. This pass lives in `recovery/src`, which the identity guard
already scans, so the CI check applies without extending it.

## Testing

- **Corpus-driven, and the bar is a number.** LedgerSMB's PL/pgSQL is the primary corpus; Pagila is
  the fast one. The suite asserts a floor on the proportion of functions reaching `Statements`, and
  that floor only ever goes up. A regression that drops a function from `Statements` to `Partial`
  fails CI with the function named.
- Dollar-quoting: nested tags, a dollar quote inside a string literal, inside a line comment, inside
  a block comment, and an unterminated one.
- Every `ProcStmt` variant has a minimal fixture asserting the parsed shape, including labelled
  loops, `EXIT WHEN`, nested blocks with exception handlers, `RETURN QUERY`, `RETURN NEXT`, cursors
  and `FOREACH … SLICE`.
- Variable substitution: a statement filtering on a declared variable lowers to a `Filter` with the
  binding recorded.
- Local recovery: a fixture with one deliberately malformed statement yields exactly one
  `Unrecovered` node and parses the rest.
- Fidelity invariant: constructing `Statements` with any `Unrecovered` or `Unmapped` node fails.
- Trigger classification: one fixture per class, and an ambiguous one that must land in `Mixed`.
- Determinism: the same input yields byte-identical IR across runs (RFC 0135 Part C).

## Alternatives considered

- **Shell out to PostgreSQL or `plpgsql_check` to parse.** Rejected: contradicts the no-shell-out
  precedent (RFC 0147), requires a live server for an offline recovery stage, and makes recovery
  non-deterministic in the dependency sense.
- **Extend `TransformNode` with control-flow variants.** Rejected — it would push imperative
  semantics into a dataflow IR that Pentaho and plain SQL also use, and every existing consumer would
  need to handle node kinds meaningless in its own domain.
- **LLM-assisted parsing of the body.** Rejected outright, and this is the central point of the RFC:
  a non-deterministic parser cannot be the ground truth that constrains a non-deterministic
  generator. Structure first, model second, exactly as RFC 0150 sequenced it.
- **Only parse what is needed for migration.** Rejected: the parser is cheaper to write correctly
  once than to grow incrementally under the pressure of a specific corpus, and the fidelity label
  system needs full coverage to be meaningful.
- **Bind `sqlparser`'s PL/pgSQL support.** It has none worth binding; the dialect handles the
  statement envelope, not the body.

## Open questions

- [ ] `%TYPE` and `%ROWTYPE` declarations need catalog lookup to resolve. Resolve at parse time when
      a catalog is available, or keep unresolved in the IR and resolve in `compile`?
- [ ] Should PL/Perl and PL/Python bodies get `Signature` fidelity only, or is a Perl body worth
      routing to the RFC 0147 Perl connector?
- [ ] Does `ekos_transformation_explain` render procedural IR directly, or does it need a sibling
      `ekos_procedure_explain` tool?

## Acceptance criteria

- [x] Every `ProcStmt` variant is produced from a fixture and asserted. (devlog_220)
- [x] The fidelity invariant is enforced at construction, asserted by test. (devlog_220)
- [ ] A floor for functions reaching `Statements` on LedgerSMB and Pagila is established and
      enforced as a ratchet. **LedgerSMB done: 212/212** loaded routines, from source
      (`tests/ledgersmb_corpus.rs`, devlog_228); live fixture schema 5/5 (`tests/corpus.rs`).
      Pagila not yet.
- [x] One malformed statement costs exactly one `Unrecovered` node. (devlog_220)
- [ ] Trigger classification covers every class plus the ambiguous case.
- [x] `Procedure` and `ProcedureStatement` have REGISTRY rows with `structurally_keyed: true`.
      (devlog_229 — and `PlPgSqlAnalyzerPass` runs in `ekos recover`)
- [x] Output is byte-identical across runs — asserted on every LedgerSMB routine (devlog_228).

## Amendment 2026-10-02 — the corpus, measured from source

The parser was first measured on five routines read back through `pg_get_functiondef`, which
normalizes exactly the things hand-written source does not: `LANGUAGE` before `AS`, no comments
inside clauses, dollar-quoted bodies. Run against the 212 PL/pgSQL routines LedgerSMB's `LOADORDER`
installs, it fully recovered **44 of 57 it recognised — and failed to recognise 165 at all**. Nine
bugs, all fixed with regression tests (devlog_228); the corpus now recovers 212/212 and is a
ratchet.

Two findings change how the rest of this RFC should be read:

1. **A wrong `Statements` label is possible without any `Unrecovered` node.** `DROP TABLE IF
   EXISTS` was counted as an `IF` opener, so every later statement — a `RETURN` included — was
   swallowed into that statement's SQL text, and the routine was labelled complete. Fidelity is
   computed from the IR, but the IR can be wrong in a way the label cannot see. The corpus test
   therefore also asserts that every span covers exactly one statement; that check, not the label,
   caught it. RFC 0164's lowering should treat an `Sql` text containing a top-level `;` as a parser
   defect, never as one statement.
2. **Spans are now computed, not searched for.** Every fragment the parser handles is a subslice of
   one comment-masked body, and its span is its address within it. The earlier
   `parent.find(fragment)` approach produced plausible offsets pointing at the wrong text for every
   nested statement — the failure *Fidelity labels* warns about. Comments are blanked to spaces of
   equal byte length before parsing, so spans index the original source exactly.

Also supported now: pre-8.0 single-quoted bodies (`AS ' … '`, spans mapped through the collapsed
`''`), `ELSEIF`, `=` assignment, `E'…'` strings, labelled `END LOOP x`, and every SQL command
PL/pgSQL executes directly (`CALL`, `NOTIFY`, `LOCK`, …).

## Amendment 2026-10-02 (b) — the analyzer pass, as built

`PlPgSqlAnalyzerPass` (`crates/recovery/src/plpgsql_analyzer.rs`, devlog_229) runs once per `.sql`
file. Decisions *Where it runs* left open, and what was chosen:

- **Which files.** Only files resolved to the `postgres` dialect or that name `plpgsql`. A T-SQL or
  MySQL procedure has no `LANGUAGE` clause and would "parse" as a `Partial` PL/pgSQL routine whose
  gaps are really a different language.
- **Which text.** The redacted file as written, not the dialect-preprocessed text the other SQL
  passes get: `preprocess` rewrites the file, and statement spans and line numbers must cite it.
- **Every routine gets a `Procedure`**, in any language; only PL/pgSQL gets statements. A
  `LANGUAGE sql` routine is `Signature`-fidelity, never an empty body.
- **Statements at every depth**, numbered in pre-order, one `ProcedureStatement` each, with
  `Contains` edges procedure → statement → nested statement. Each child records its **branch**
  (`then:N`, `else`, `body`, `handler:N`) on both the object and the edge, so the stored tree keeps
  the routine's control flow and not only its membership. A statement's properties are its own
  serialized IR form minus spans and nested bodies, so a new `ProcStmt` variant needs no change here.
- **Evidence.** A leaf cites its full source text (capped at 4 KiB, exact span always recorded); a
  compound statement cites its first line only, since its body is cited statement by statement.
- **Keys.** `Procedure` = (path, lower-cased name, argument list) — overloads are distinct routines.
  `ProcedureStatement` = (routine key, pre-order index). A later `CREATE OR REPLACE` of the same
  routine in the same file replaces the earlier one, as it does in the database.
- **No `Calls` edges to tables or routines yet.** A per-file pass sees neither the embedded SQL's
  lowered form nor other files' tables; both belong to RFC 0164's lowering plus a whole-graph,
  unambiguous-name link in the style of RFC 0075. The statement text needed is on every object.
- `catalog oid` keying for routines read live (RFC 0157) is not used: pg-live keeps routine bodies
  in the Migrate catalog, not the ledger, so source files are the only consumer today.

Measured end to end on LedgerSMB `sql/` (259 files, mock LLM): 556 routines, 254 PL/pgSQL (251
complete, 3 partial — all in the unloaded `Business_Dates.sql`), 1556 statements; zero compile
warnings from the new kinds; after a second full `recover`→`commit` on unchanged input every one of
the 2,112 objects still has exactly one ledger version.

## Amendment 2026-10-02 (c) — what routines touch, and the links

*Where it runs* promised "`Calls` edges from procedure to the tables, functions and procedures it
touches". Built as two stages (devlog_230):

1. **Footprint, per file** (`recovery/src/plpgsql_footprint.rs`). Each statement's own SQL and
   expressions are parsed with `sqlparser`'s PostgreSQL dialect and the AST walked with its
   visitor: writes = `INSERT`/`UPDATE`/`DELETE`/`MERGE`/`TRUNCATE` targets; reads = every other
   relation minus the statement's CTE names; calls = every function, a table function
   (`FROM setting_get(…)`) included as a call, not a table. Expressions are parsed as
   `SELECT <expr>`, as PL/pgSQL evaluates them, so variables need no placeholder substitution —
   the deviation from *Parsing*'s "typed placeholders", which proved unnecessary. A fragment that
   does not parse is recorded with the parser's error (`footprint: unparsed`), never guessed at.
   `LANGUAGE sql` routines get their body's footprint at routine level.
2. **Linking, whole graph** (`semantic/src/procedure_lineage.rs`, run by `ekos commit`). Not
   `Calls` to tables as first written — `Calls` is the routine call graph `callers` traverses, so:
   `ReadsFrom`/`WritesTo` statement → table (the precise citation), `DependsOn` routine → table
   (what `dependents`/`impact` traverse), `Calls` routine → routine. A name links only when it names
   **exactly one** object (RFC 0060/0075's judgment); one edge per (kind, from, to).

Parsing the IR's own text as SQL turned out to be the strongest check the parser has had: it found
four more bugs that corrupted IR text under a `Statements` label (`INSERT INTO t` read as the
binding `INTO`, loop queries losing a closing parenthesis, the old `SELECT INTO a, b expr` form,
`EXECUTE … USING` on its own line). On LedgerSMB, 1137/1141 statements' SQL now parses — the four
left are `sqlparser` 0.53 grammar gaps — and that number is a ratchet
(`recovery/tests/plpgsql_ledgersmb.rs`).

## Downstream integration (2026-10-03, devlog_232)

`Procedure` and `View` now reach every downstream consumer RFC 0147 lists: curated docs-gen entity
pages (`View`s also in Data Stores, `Procedure`s in API.md under their `File`), documentation
coverage, `doc_links` (a backticked routine/view name in a doc links to it) and `llm_description`
(symbol scope). Each object hangs off its `File` (`Contains`) and carries `source_span`, without
which `llm_description` would skip it silently — a test builds them through the real analyzers and
asserts they are described. `COMMENT ON FUNCTION|PROCEDURE|[MATERIALIZED] VIEW` in the same file
becomes the object's evidence-backed `description` (LedgerSMB: all 330 function comments and every
view comment on an observed file attach), matched by name and then arity, never guessed.

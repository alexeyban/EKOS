# RFC 0146 — PostgreSQL dialect coverage for hand-written schemas

**Status:** Phases 1, 2 and 3 implemented and verified (2026-09-16)
**Author:** EKOS team
**Created:** 2026-09-16
**Builds on:** RFC 0031 (`SqlDialectParser` trait + dialect registry), RFC 0057/0058/0059
(dialect-specific `preprocess` precedent — MySQL `DELIMITER`, ClickHouse `CREATE DICTIONARY`,
Postgres `CREATE SEQUENCE`/`UNLOGGED`/`NOT VALID`)

---

## Motivation

`PostgresDialectParser` was built and tuned against **`pg_dump` output** (RFC 0059's evidence is
`analytics/priv/repo/structure.sql`, a machine-generated dump). `pg_dump` emits a deliberately
narrow, mechanical subset of PostgreSQL. **Hand-written schemas do not**, and EKOS currently
recovers almost nothing from them.

Measured on LedgerSMB (a Perl 5 / PostgreSQL double-entry accounting ERP, 260 `.sql` files,
2026-09-16) with `default-dialect = "postgres"` correctly configured:

| | Today |
|---|---|
| `Table` objects from `sql/Pg-database.sql` — the core schema | **0** of 158 |
| `ForeignKey` relationships from the same file | **0** |
| `.sql` files whose whole-file parse succeeds | 129 / 260 |
| `Table` objects, whole workspace | 8 (all from test fixtures and two migrations) |

The core schema of the application compiles to nothing. This is not a misconfiguration and not a
missing dialect — it is `PostgreSqlDialect` in `sqlparser 0.53` lacking grammar for constructs a
human-maintained schema uses constantly, combined with `parse_ddl_structural`'s all-or-nothing
whole-file parse (`crates/recovery/src/sql_analyzer.rs:206`): **one unsupported statement discards
every table in the file.** It surfaces only as a buried `SQL001: no tables found` warning, never an
error, and nothing downstream reports that the schema is gone.

The failure is silent in a second, worse way. `sql_analyzer` does not model `COMMENT ON` at all,
so LedgerSMB's **631 hand-written `COMMENT ON TABLE`/`COLUMN`/`FUNCTION` descriptions** are
discarded — and then `SqlAnalyzerPass`'s own LLM semantic-naming step is asked to *invent*
`description` values for those same tables. EKOS pays for generated prose while deleting the
authoritative text sitting in the source.

### Why this is the right layer to fix it

RFC 0031 put a `preprocess(&self, sql: &str) -> String` seam on `SqlDialectParser` precisely for
"real-world dialect text `sqlparser` cannot swallow." RFC 0057/0058/0059 already use it for
exactly this class of problem. This RFC continues that line; it invents no new mechanism.

## Evidence

All numbers below are measured, not estimated. Method: a harness parsing every `.sql` file under
`LedgerSMB/sql` with the real `PostgresDialectParser` (its current `preprocess` included), once
whole-file and once statement-by-statement, histogramming failures by shape. The proposed
transforms were then implemented in the harness and the corpus re-measured; the transformed core
schema was finally run through the real `ekos build/recover/resolve/compile` to confirm the
object counts end to end.

Failure classes, by count of failing statements across the corpus:

| Construct | Statements | sqlparser 0.53 status |
|---|---|---|
| `COMMENT ON FUNCTION\|VIEW\|TYPE\|INDEX\|SEQUENCE\|TRIGGER\|ROLE\|CONSTRAINT\|AGGREGATE … IS …` | 354 | `Expected: comment object_type` — only a fixed object-type set is known |
| `COMMENT ON TABLE\|COLUMN … IS $$…$$` | ~277 | `Expected: literal string` — dollar-quoted bodies rejected |
| `f(arg := value)` named call arguments | 75 | `Expected: ), found: :=` — only `=>` is known |
| `CREATE FUNCTION … SECURITY DEFINER\|INVOKER` | 37 | `Expected: end of statement, found: SECURITY` |
| `\echo` / `\set` / `\copy` psql meta-commands | 37 | not SQL grammar at all |
| `CREATE TABLE … INHERITS (…)` | 25 | `Expected: end of statement, found: INHERITS` |
| `RETURNS SETOF <type>` | 18 | `Expected: end of statement, found: <type>` |
| `DO $$ … $$` anonymous blocks | 11 | `Expected: an SQL statement, found: DO` |
| `ALTER TABLE … NO INHERIT …` | 11 | `Expected: ADD, RENAME, PARTITION, … after ALTER TABLE` |
| `COPY … FROM stdin` + inline data + `\.` | 1 block | no grammar for the payload rows |
| `CREATE RULE …` | 3 | `Expected: an object type after CREATE, found: RULE` |

`INHERITS` is the only construct that breaks `CREATE TABLE` statements *themselves* (24 of the 26
that fail individually). Everything else in the table breaks `CREATE TABLE` recovery **indirectly**,
by failing the whole-file parse that the DDL analyzer depends on. That indirection is why the
symptom is "the entire schema is missing" rather than "a few tables are missing."

## Design

### Phase 1 — extend `PostgresDialectParser::preprocess`

Eleven transforms, appended to the existing chain in `plugins/sql-dialect-postgres/src/lib.rs`.
Each is either a **whole-statement strip** (for constructs EKOS models no facts from, matching
RFC 0058/0059's `CREATE DICTIONARY`/`CREATE SEQUENCE` precedent) or a **clause strip** (keeping the
statement, matching `strip_unlogged_before_table`/`strip_not_valid_clause`).

| # | Transform | Kind | Rationale |
|---|---|---|---|
| P1 | `INHERITS (…)` | clause | Keeps the table. Inheritance is not a KIR fact today. |
| P2 | `ALTER TABLE … NO INHERIT …` | statement | Same; nothing modeled. |
| P3 | `COMMENT ON <unsupported type> … IS …` | statement | See Phase 2 — `TABLE`/`COLUMN` are *captured* first, not discarded. |
| P4 | `\echo` / `\set` / `\copy` lines | line | psql client syntax, not SQL. **`\.` is excluded — see Decisions.** |
| P5 | `DO $tag$ … $tag$` | statement | PL/pgSQL, never a KIR fact. |
| P6 | trailing `SECURITY DEFINER\|INVOKER` | clause | Keeps `CREATE FUNCTION` parseable for the Transformation IR. |
| P7 | `RETURNS SETOF <t>` → `RETURNS <t>` | rewrite | Preserves the return type; only the set-ness is lost, which is unmodeled. |
| P8 | `:=` → `=>` in call argument lists | rewrite | Both are real Postgres named-argument syntax; `=>` is the one sqlparser knows. |
| P9 | `COPY … FROM stdin` through its `\.` terminator | block | Payload rows are seed data, not schema. |
| P10 | `CREATE RULE …` | statement | No grammar; not modeled. |
| P11 | leading-comment-aware statement keyword detection | mechanism | Shared by P2/P3/P5/P10 — see Decisions. |

All transforms must be quote-, comment- and dollar-quote-aware, reusing the character-scanner
style already established by `strip_not_valid_clause`. Naive regex or `sql.split(';')` is not
adequate: `COMMENT ON … IS $$ … ; … $$` and PL/pgSQL bodies both carry `;` inside dollar quotes.

### Phase 2 — capture `COMMENT ON TABLE`/`COLUMN` as real descriptions

`preprocess` can only *remove* text; it cannot produce facts. So before P3 strips them,
`sql_analyzer` extracts `COMMENT ON TABLE <t> IS <text>` and `COMMENT ON COLUMN <t>.<c> IS <text>`
(single-quoted **or** dollar-quoted) and attaches the text as a `description` property on the
corresponding `Table`/`Column` KIR object, with the source file and line as `KirEvidence`.

This converts author-written descriptions from discarded text into evidence-backed facts, and gives
`SqlAnalyzerPass`'s LLM naming step real input to defer to instead of inventing prose. It is the
only part of this RFC that touches `crates/recovery/`; everything else lives in the dialect plugin.

### Phase 2 as built (2026-09-16)

`crates/recovery/src/sql_comments.rs` extracts, `sql_analyzer.rs` applies. Measured on the real,
unmodified `LedgerSMB/sql/Pg-database.sql`:

| | Before | After |
|---|---|---|
| Tables with a description | 0 | **113 of the 113 distinct tables that carry one** |
| Columns with a description | 0 | **82 of 85** |

**Precedence.** An author-written `COMMENT ON` description outranks a generated one. The model
still contributes `entity_name`/`entity_type` — which the schema states nowhere — but never
overwrites text a human wrote about their own table; replacing an observed fact with an inferred
one is the opposite of what this compiler is for. The model's version is kept under
`llm_description` so the two stay comparable, and the provenance is recorded separately as
`sql_comment` so a consumer can distinguish observed from inferred without reading evidence
records. Each recovered description adds a `KirEvidence` with a real `SourceLocation::at` line —
the DDL path previously recorded no line numbers at all.

**The three missing columns are not a defect.** `eca_note.ref_key`, `file_incoming.ref_key` and
`file_internal.ref_key` are commented in the source but inherited, not declared: their tables are
`CREATE TABLE ... INHERITS (note)` with no column list of their own. Phase 1 strips `INHERITS`
because the KIR has no relationship for table inheritance, so the column genuinely does not exist
on the child object and the extractor declines to invent one. This is the documented "drop rather
than invent" rule, and this is exactly what it costs: three columns out of 85. Modelling
inheritance as a real `RelationshipKind` would recover them, and is the natural follow-up.

**Shared lexing.** The extractor needs the same dollar-quote-aware scanning as Phase 1, but lives
in `recovery`, which cannot depend on the dialect plugin. Rather than keep two copies of subtle
quote-tracking logic — the drift hazard this codebase has been bitten by before — the primitives
moved to `ekos-sql-dialect-sdk::lex` (`skip_non_code`, `dollar_quote_tag_end`, `statement_spans`,
`is_word_at`, `skip_trivia`) and both consumers use them. The postgres plugin's tests stayed green
across that move, which is what makes the dedup safe to claim.

### Phase 3 — a token budget that fits the work, and a diagnostic when it doesn't

`SqlAnalyzerPass` hardcoded `max_tokens: 4096` and no config could reach it: `[ai] max-tokens`
governs the read-side `ekos ask` runtime, so a workspace could plainly declare `max-tokens = 8192`
and have the compiler ignore it. That default is generous for a 3-table migration and far too small
for a 158-table schema, whose response needs one JSON object per table and per foreign key.

Three changes, each addressing a different half of the failure:

1. **`[llm] max-tokens`** — an explicit ceiling for compiler-pass LLM calls, distinct from
   `[ai] max-tokens` and documented as such.
2. **A computed default.** `enrichment_token_budget(tables, relationships)` =
   `512 + 80 × items`, floored at 4,096 (so small files are untouched) and capped at 32,768.
   The per-item figure comes from the prompt's own output schema — an entity line runs 40-60
   tokens — with headroom for the hidden reasoning tokens a reasoning model spends from the same
   budget.
3. **`SQL004`.** Partial enrichment was completely silent: on a real run the model named 24 of 192
   tables and nothing reported it, which is indistinguishable from a schema that simply has no
   descriptions. The pass now compares tables named against tables present and warns, naming the
   shortfall and — when `output_tokens` reached the ceiling — saying so and what to raise.
   Coverage counts *matched* tables, so a hallucinated table cannot make coverage look complete.

**The ceiling was calibrated by getting it wrong first.** A first cut capped at 16,384, which
silently clamped the formula's own 30,272 estimate to half and reproduced the original failure
exactly — empty response, `SQL002`. An explicit `[llm] max-tokens = 32768` then named 157 of 158
tables, proving the formula had been right and the guard wrong. With the ceiling raised to 32,768,
the same file needs no configuration at all:

| `Pg-database.sql` | Tables named by the LLM |
|---|---|
| hardcoded 4,096 | 0 (empty response, `SQL002`) |
| computed, capped at 16,384 | 0 (empty response, `SQL002` naming the ceiling) |
| explicit `max-tokens = 32768` | 157 of 158 (`SQL004` reports the one) |
| **computed, capped at 32,768 (shipped)** | **158 of 158, no diagnostics** |

### Non-goals

- **YAML frontmatter in `sql/consistency/*.sql`** (25 files — `--- yaml frontmatter` / `title:` /
  `description:` headers above the SQL). This is a LedgerSMB repo convention, not PostgreSQL
  grammar. Teaching a shared dialect plugin one project's file format would be a layering error.
- **`:slschema` psql variable substitution** (216 statements, all in `sql/upgrade/sl2.8.sql`, a
  legacy SQL-Ledger 2.8 upgrade script). A psql client feature, in one low-value file.
- **PL/pgSQL body parsing.** `sqlparser` has no PL/pgSQL grammar; function bodies stay opaque.
  This caps Transformation IR coverage and is out of reach at this layer.
- **Per-statement fallback for `parse_ddl_structural`.** Complementary and still worth doing —
  `sql_transform_analyzer` already has one and the DDL path does not, which is the asymmetry that
  turns any single unsupported statement into total schema loss. It deserves its own RFC, and this
  one deliberately does not depend on it: fixing the dialect removes the *cause*, while a fallback
  would only contain the *blast radius*. Note that the existing fallback splits on `sql.split(';')`
  (`sql_transform_analyzer.rs:322`) and is therefore wrong inside dollar-quoted bodies and string
  literals; that RFC should reuse this one's quote-aware statement scanner rather than repeat it.
- **Upstreaming to `sqlparser`.** Several of these are real upstream gaps and worth filing, but
  EKOS pins `sqlparser = "0.53"` and cannot wait on a release cycle.

## Expected outcome

Measured by re-running the harness with all eleven transforms implemented, over the same corpus:

| Metric | Today | With RFC 0146 |
|---|---|---|
| `sql/Pg-database.sql` whole-file parse | fails | **succeeds** |
| `CreateTable` statements parsed from it | 0 | **158 / 158** |
| `.sql` files parsing whole-file | 129 / 260 | **205 / 260** |
| Statements parsing individually | 4835 / 6179 | **5200 / 5516** |

End-to-end through the real pipeline (`ekos build → recover → resolve → compile` on the
transformed core schema, no `SQL001`, no parse warning):

| | Today | With RFC 0146 |
|---|---|---|
| `Table` objects | 0 | **158** |
| `ForeignKey` relationships | 0 | **214** |

The 26 files still failing whole-file parse afterwards are the documented non-goals: YAML
frontmatter (25) and the `sl2.8.sql` psql-variable script (1).

## Decisions

**Whole-statement strip vs. clause strip.** Follows the rule RFC 0058/0059 set: strip the clause
when the remaining statement still yields a fact EKOS models (`INHERITS` on a real `CREATE TABLE`,
`SECURITY DEFINER` on a real `CREATE FUNCTION`); strip the whole statement only when nothing in it
is modeled (`DO`, `CREATE RULE`, `COMMENT ON` of an unsupported object type). Nothing already
captured is lost either way.

**`\.` is not a meta-command.** Found the hard way while measuring this RFC: a naive "strip lines
beginning with `\`" removes the `\.` that terminates a `COPY … FROM stdin` data block. sqlparser
then consumes the entire rest of the file as COPY payload and **returns `Ok`** — the parse appears
to succeed while every subsequent statement silently vanishes. In the first measurement pass this
turned 158 tables into 63 with no error anywhere. P4 must exclude `\.`, and P9 must remove the
`COPY` header and its payload together as one unit. A "successful" parse returning implausibly few
statements is the only symptom; the harness should assert statement counts, not just `is_ok()`.

**Keyword detection must skip leading comments (P11).** Deciding what a statement *is* via
`stmt.trim_start()` misses every statement carrying a comment header — which in a hand-written
schema is most of them. One `COMMENT ON … IS $$…$$` preceded by three `--` lines was enough to
keep failing the whole-file parse after every other transform was in place.

**Dollar-quote awareness is mandatory, not defensive.** Both the statement splitter and every
transform must track `$tag$ … $tag$`. LedgerSMB has 631 `COMMENT ON` statements and hundreds of
PL/pgSQL bodies; `;` inside them is common.

**Keyword matching must be case-insensitive (found during implementation).** Every RFC 0059
scanner uses a case-*sensitive* matcher, which is correct for its inputs: `pg_dump` emits uppercase
keywords exclusively. Hand-written schemas mix case freely, and the LedgerSMB histogram shows it
plainly — `INHERITS` 15 / `inherits` 10, `SECURITY DEFINER` 25 / `security definer` 12. Reusing the
existing matcher silently handled some of a project's tables and not others. RFC 0146's passes use
a new `is_word_boundary_match_ci`; the RFC 0059 passes keep the case-sensitive one, so their
`pg_dump` behaviour is provably unchanged.

## Test plan

Per the mandatory workflow, tests precede implementation.

1. **Unit tests in `plugins/sql-dialect-postgres`** — one per transform, each asserting that a
   minimal real statement pair (before → after) parses under `PostgreSqlDialect` after
   `preprocess`, plus negative cases: a `$$` body containing `;` and `--`, a `COMMENT ON TABLE`
   that must survive to Phase 2, and a `COPY … FROM stdin` block whose `\.` must be preserved by
   P4 and removed by P9.
2. **Regression guard on RFC 0059's fixtures** — the `pg_dump` inputs that motivated the existing
   `CREATE SEQUENCE`/`UNLOGGED`/`NOT VALID` transforms must parse exactly as before. These
   transforms compose with the new ones and must not be reordered into conflict.
3. **Statement-count assertion** — parse a fixture with a known statement count and assert the
   count, not just `is_ok()`. This is the only thing that would have caught the `\.` bug.
4. **Corpus fixture** — `tests/fixtures/hand-written-schema.sql`, asserted to yield a fixed
   non-zero `CREATE TABLE` count. Guards against silent regression to zero, the failure mode this
   whole RFC exists to prevent. **Implemented as a hand-authored file reproducing each construct in
   the shape LedgerSMB writes it, rather than the excerpt this RFC originally proposed** —
   LedgerSMB is GPL, and vendoring an excerpt into this repo would import that obligation for no
   testing benefit.
5. **Phase 2** — `COMMENT ON TABLE`/`COLUMN` with both single-quoted and dollar-quoted bodies
   produce `description` properties with correct `KirEvidence` source paths.

## Benchmark

`preprocess` runs once per `.sql` file and is linear single-pass character scanning. The corpus is
260 files / ~2.6 MB; current `ekos recover` on LedgerSMB is 4m01s, overwhelmingly pass-scheduling
overhead rather than parsing. No benchmark is required by the PR checklist, but
`benchmark/benches/sql_analyzer.rs` should be re-run to confirm no regression, since Phase 1
strictly *increases* the number of statements that reach the DDL builder.

## Implementation notes (Phase 1, 2026-09-16)

All of Phase 1 lives in `plugins/sql-dialect-postgres/src/lib.rs`; no other crate changed.

The RFC 0059 passes are left exactly as they were and still run first, so their `pg_dump`
behaviour is unchanged — `postgres_dialect_parses_the_real_analytics_structure_sql_after_preprocessing`
(the 42-table Plausible fixture) still passes untouched. RFC 0146's passes are built on three new
shared primitives rather than extending the old ones:

| Primitive | Why it is new rather than an extension |
|---|---|
| `skip_non_code` | The single place dollar quoting is understood. The RFC 0059 scanners track only `'`/`"`/`--`, which is enough for `pg_dump` and not for hand-written SQL. |
| `edit_statements` | Statement-boundary-aware map/drop over a file. Needed because `DO` and `CREATE RULE` are single common words — matching them anywhere in the text, the way `strip_statements_starting_with` matches, would cut unrelated statements in half. |
| `is_word_boundary_match_ci` | See the case-sensitivity decision above. |

Dropped statements are replaced by their own newlines rather than deleted, so line numbers in
`sqlparser` error messages still line up with the user's real file — which matters a great deal
when debugging exactly this class of problem.

Verification: 45 tests in the crate, 1838 across the workspace, `cargo clippy --workspace -D
warnings` and `cargo fmt --check` clean. The unmodified `LedgerSMB/sql/Pg-database.sql` run through
the real `ekos build → recover → resolve → compile` produces **158 `Table` objects and 214
`ForeignKey` relationships** with no `SQL001` and no parse warning, matching this RFC's predicted
numbers exactly.

## Migration and compatibility

No public API change: `SqlDialectParser` is untouched, and `preprocess` is an existing method.
No config change — workspaces already setting `default-dialect = "postgres"` get the improvement on
the next `ekos recover`.

Because the ledger is append-only, existing workspaces do not retroactively gain the tables; they
need a re-`recover`/`compile`/`commit`. Newly recovered `Table` objects carry structurally-derived
identities, so `DefaultResolver` treats them as it does any other DDL-recovered table. Worth
watching on the first real run: LedgerSMB's 158 tables produced 42 `SameAs` candidates in the
verification run, all routed to review as `unconfirmed` rather than merged (RFC 0060/0063).

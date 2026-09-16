# Devlog 187 — RFC 0146: the Postgres dialect only ever understood `pg_dump` (LedgerSMB: 0 → 192 tables)

**Date:** 2026-09-16
**PRs:** (local, not pushed) `5fc8e67` RFC 0146 — Phase 1 (dialect coverage), Phase 2 (`COMMENT ON` capture) and Phase 3 (enrichment token budget), plus this docs commit
**Branch:** main (local)

---

## Summary

Started from a plain request: analyse `/home/legion/PycharmProjects/LedgerSMB` (a Perl 5 / PostgreSQL
double-entry accounting ERP) with EKOS. The pipeline ran clean and produced **8 `Table` objects and 1
`ForeignKey`** for a repo whose `sql/Pg-database.sql` declares **158 tables**. `[recover.sql]
default-dialect` was correctly set to `postgres`; nothing was misconfigured.

The cause: `PostgresDialectParser` was built and tuned entirely against **`pg_dump` output** — RFC 0059's
evidence file is a machine-generated dump — and `pg_dump` emits a deliberately narrow, mechanical subset of
PostgreSQL. Hand-maintained schemas do not. Combined with `parse_ddl_structural`'s all-or-nothing whole-file
parse, a single `COMMENT ON TABLE … IS $$…$$` discarded every table in the file, reported only as a buried
`SQL001`.

RFC 0146 fixes it in three phases, all measured against the real repo rather than estimated: eleven
`preprocess` transforms (Phase 1), `COMMENT ON` captured as evidence-backed descriptions instead of being
deleted and re-invented by an LLM (Phase 2), and an enrichment token budget that scales with the schema plus a
diagnostic for partial coverage (Phase 3).

**Result on LedgerSMB:** 8 → **192** `Table`, 1 → **218** `ForeignKey`, 0 → **182** described tables (127 of
them in the schema authors' own words), 8 → **470** data-lineage links, Transformation IR 16% → **34%** mapped.

---

## PR — RFC 0146 Phase 1: dialect coverage for hand-written schemas

### Problem / motivation

`sqlparser 0.53`'s `PostgreSqlDialect` has no grammar for constructs a human-maintained schema uses
constantly. Measured across LedgerSMB's 260 `.sql` files with a purpose-built harness (parse each file
whole, then statement-by-statement, histogram the failures by shape):

| Construct | Failing statements |
|---|---|
| `COMMENT ON FUNCTION\|VIEW\|TYPE\|INDEX\|SEQUENCE\|…` | 354 |
| `COMMENT ON TABLE\|COLUMN … IS $$…$$` | 277 |
| `f(arg := value)` named call arguments | 75 |
| `CREATE FUNCTION … SECURITY DEFINER\|INVOKER` | 37 |
| `\echo` / `\set` / `\copy` psql meta-commands | 37 |
| `CREATE TABLE … INHERITS (…)` | 25 |
| `RETURNS SETOF <type>` | 18 |
| `DO $$ … $$` anonymous blocks | 11 |
| `ALTER TABLE … NO INHERIT …` | 11 |
| `CREATE RULE …` | 3 |
| `COPY … FROM stdin` + inline data | 1 block |

Only `INHERITS` breaks `CREATE TABLE` statements *themselves*. Everything else breaks table recovery
**indirectly**, by failing the whole-file parse the DDL analyzer depends on — which is why the symptom is
"the entire schema is missing" rather than "a few tables are missing".

### What was built

Eleven transforms appended to `PostgresDialectParser::preprocess`, each either a whole-statement strip (for
constructs EKOS models no facts from, matching RFC 0058/0059's precedent) or a clause strip (keeping a
statement that still yields a fact).

| # | Transform | Kind |
|---|---|---|
| P1 | `INHERITS (…)` | clause |
| P2 | `ALTER TABLE … NO INHERIT …` | statement |
| P3 | `COMMENT ON …` | statement |
| P4 | `\echo` / `\set` / `\copy` lines | line |
| P5 | `DO $tag$ … $tag$` | statement |
| P6 | trailing `SECURITY DEFINER\|INVOKER` | clause |
| P7 | `RETURNS SETOF <t>` → `RETURNS <t>` | rewrite |
| P8 | `:=` → `=>` in call arguments | rewrite |
| P9 | `COPY … FROM stdin` through its `\.` | block |
| P10 | `CREATE RULE …` | statement |
| P11 | leading-comment-aware keyword detection | mechanism |

### Implementation details worth remembering

The RFC 0059 passes are untouched and still run first, so their `pg_dump` behaviour is provably unchanged —
the 42-table Plausible fixture test passes as-is. Phase 1's transforms sit on three new primitives rather
than extending the old ones:

- `skip_non_code` — the single place dollar quoting is understood. The RFC 0059 scanners track only
  `'`/`"`/`--`, sufficient for `pg_dump`, not for hand-written SQL.
- `edit_statements` — statement-boundary-aware map/drop. Required because `DO` and `CREATE RULE` are single
  common words; matching them anywhere the way `strip_statements_starting_with` does would cut unrelated
  statements in half. Dropped statements are replaced by their own newlines so `sqlparser` error line numbers
  still line up with the user's real file.
- `is_word_boundary_match_ci` — see Knowledge Captured.

### Decisions

**Preprocessing rather than a per-statement DDL fallback.** `sql_transform_analyzer` already has a
per-statement fallback and `parse_ddl_structural` does not; that asymmetry is what turns one unsupported
statement into total schema loss. Adding the fallback is still worth doing and has its own RFC waiting, but
it only contains the blast radius — fixing the dialect removes the cause. Noted for whoever writes it: the
existing fallback splits on `sql.split(';')` (`sql_transform_analyzer.rs:322`), which is wrong inside
dollar-quoted bodies, and should reuse this RFC's scanner.

**Explicitly out of scope**, with reasons: the YAML frontmatter in `sql/consistency/*.sql` (25 files) is a
LedgerSMB repo convention, not PostgreSQL grammar — teaching a shared dialect plugin one project's file
format would be a layering error; `:slschema` psql variables (216 statements, all in one legacy SQL-Ledger
2.8 upgrade script); PL/pgSQL bodies, which `sqlparser` cannot parse at all.

---

## PR — RFC 0146 Phase 2: `COMMENT ON` becomes an evidence-backed description

### Problem / motivation

`sql_analyzer` does not model `COMMENT ON` at all. LedgerSMB's `sql/` tree carries **631** of them, 277 on
tables and columns. So EKOS deleted the schema's authoritative, human-written documentation — and then asked
an LLM to invent replacements for the same tables. Paying for generated prose while discarding the real text
is close to the opposite of what this compiler exists to do.

### What was built

`crates/recovery/src/sql_comments.rs` extracts; `sql_analyzer.rs` applies. Measured on the real, unmodified
`Pg-database.sql`: **113 of the 113 distinct commented tables** and **82 of 85 columns** now carry
descriptions, up from zero.

### Implementation details worth remembering

Extraction runs in `SqlAnalyzerPass::new`, on the **raw** SQL, *before* `preprocess`. That ordering is the
whole design constraint: `SqlDialectParser::preprocess` can only remove text, never produce facts, so
extraction cannot live in the dialect crate and must happen upstream of it.

Each description adds a `KirEvidence` with a real `SourceLocation::at` line — the DDL path previously
recorded no line numbers at all.

### Decisions

**Author text outranks generated text.** `description` holds the human's words; the model's version is kept
separately as `llm_description` rather than discarded, so the two stay comparable; `sql_comment` records
provenance so a consumer can tell observed from inferred without reading evidence records. The model still
contributes `entity_name`/`entity_type`, which the schema states nowhere. It simply never overwrites text a
human wrote about their own table — that would replace an observed fact with an inferred one.

**Shared lexing promoted to the SDK.** The extractor needs the same dollar-quote-aware scanning as Phase 1
but lives in `recovery`, which cannot depend on the dialect plugin. Two subtly different quote scanners would
have to agree *exactly* — otherwise the extractor reads text the stripper did not remove, or misses text it
did. The primitives moved to `ekos-sql-dialect-sdk::lex` and both consumers use them; the plugin's 44 tests
stayed green across the move, which is what makes the dedup safe to claim rather than assert.

**The three missing columns are not a defect.** `eca_note.ref_key`, `file_incoming.ref_key` and
`file_internal.ref_key` are commented but *inherited* — `CREATE TABLE … INHERITS (note)` with no column list.
Phase 1 strips `INHERITS` because the KIR has no relationship for table inheritance, so the column genuinely
is not on the child object and the extractor declines to invent one. That is the documented "drop rather than
invent" rule, and this is exactly what it costs.

---

## PR — RFC 0146 Phase 3: an enrichment budget that fits the work

### Problem / motivation

`SqlAnalyzerPass` hardcoded `max_tokens: 4096` and **no config could reach it**: `[ai] max-tokens` governs
the read-side `ekos ask` runtime, so a workspace could plainly declare `max-tokens = 8192` and the compiler
would ignore it. On a real run the model named **24 of 192** tables with **no diagnostic at all** — silent
partial coverage, which is indistinguishable from a schema that simply has no descriptions.

### What was built

| Change | Effect |
|---|---|
| `[llm] max-tokens` | explicit ceiling for compiler-pass LLM calls, distinct from `[ai] max-tokens` |
| `enrichment_token_budget(tables, rels)` | `512 + 80 × items`, clamped to `[4096, 32768]` |
| `SQL004` | reports tables named vs present, and names the ceiling when `output_tokens` reached it |

Coverage counts *matched* tables, so a hallucinated table cannot make coverage look complete.

### Decisions

The ceiling was calibrated by getting it wrong first — see Knowledge Captured.

---

## Knowledge Captured

- **`\.` is not a psql meta-command.** It terminates a `COPY … FROM stdin` data block. Strip it as if it were
  one and `sqlparser` consumes the entire rest of the file as COPY payload and **returns `Ok`** — the parse
  looks successful while every later statement silently vanishes. This turned 158 tables into 63 with no error
  anywhere. **Assert statement counts, never just `is_ok()`**; that is the only thing that catches it.

- **A cost guard can silently defeat the fix it guards.** Phase 3's formula computed 30,272 tokens for
  `Pg-database.sql`; a first-cut `MAX_TOKENS: 16_384` clamped it to half and reproduced the original failure
  exactly. An explicit `max-tokens = 32768` then named 157/158, proving the formula right and the guard wrong.
  Shipped at 32,768: **158/158, zero diagnostics**. The test asserts the exact value with the measurement in
  its doc comment, so a future "lower the ceiling to save money" change fails loudly.

- **Keyword matching must be case-insensitive for hand-written SQL.** Every RFC 0059 scanner is
  case-*sensitive*, correct for `pg_dump`'s all-uppercase output. LedgerSMB writes both `INHERITS` (15) and
  `inherits` (10), `SECURITY DEFINER` (25) and `security definer` (12). Reusing the old matcher silently
  handled some of a project's tables and not others.

- **Statement keyword detection must skip `--` / `/* */` banners.** Reading a statement's first characters
  misclassifies every statement carrying a comment header, which in a hand-maintained schema is most of them.
  One `COMMENT ON … IS $$…$$` behind three `--` lines kept failing the whole-file parse after every other
  transform was already in place.

- **A warm pass cache invalidates a before/after comparison.** Cached passes do not re-emit diagnostics, so a
  re-run over an existing `.ekos` under-reports warnings. Two figures reported mid-session (`SQL001` 266 → 186,
  IR 16% → 26%) were measured that way and were wrong; clean-build numbers are **266 → 251** and **16% → 34%**.
  Compare clean builds to clean builds, or compare nothing.

- **`[observe] paths` without `"."` silently yields 0 git commits.** Observers run once per `paths` entry with
  that entry as their workspace root, and `GitObserver::is_git_repo` is a literal `root.join(".git").exists()`
  — no upward walk. An enumerated 21-path config lost LedgerSMB's entire 19.5k-commit history until `paths`
  became `["."]`. `ignore-patterns` match exact path *components* (files included, so `favicon.ico` works), not
  globs. `ekos config preview-scan` reporting `across 1 root(s)` is the signal git will be observed.

- **`DeepSeek V4 Flash` spends hidden reasoning tokens from `max_tokens`.** Its empty responses were never
  random: at 4,096 and again at 16,384 it burned the whole budget reasoning and emitted nothing, producing
  `SQL002: EOF while parsing a value at line 1 column 0`. The `SQL004`/`SQL002` ceiling message now names this
  directly instead of leaving a bare JSON error.

- **The LLM cache already handles a raised ceiling correctly — do not add `max_tokens` to the cache key.**
  `cache_key` deliberately omits it; `MAX_TOKENS_FIELD` + `truncated_below` (added 2026-09-15) refresh only
  entries that actually hit their own limit, while complete answers still replay. Adding it to the key
  invalidates every entry on any budget change and re-bills work that was fine. This was attempted here and
  reverted after reading the surrounding code.

- **`SQL004` immediately found a shape the formula misses.** Three of the eight partial-coverage warnings are
  *small* files hitting the 4,096 floor: a one- or two-table migration whose body is a long PL/pgSQL function
  still makes a reasoning model work hard. The budget scales with table count; reasoning cost scales with
  input size, which the formula ignores. Now visible instead of silent.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0146-postgres-dialect-coverage.md` | New RFC — motivation, measured evidence, all three phases, decisions |
| `ekos/plugins/sql-dialect-postgres/src/lib.rs` | Phase 1: eleven `preprocess` transforms + tests; local lexing primitives moved to the SDK |
| `ekos/plugins/sql-dialect-postgres/tests/fixtures/hand-written-schema.sql` | New fixture — every construct in the shape LedgerSMB writes it (hand-authored, not excerpted: LedgerSMB is GPL) |
| `ekos/crates/sql-dialect-sdk/src/lex.rs` | New shared lexing module — `skip_non_code`, `dollar_quote_tag_end`, `statement_spans`, `is_word_at`, `skip_trivia` |
| `ekos/crates/sql-dialect-sdk/src/lib.rs` | `pub mod lex` |
| `ekos/crates/recovery/src/sql_comments.rs` | New — `COMMENT ON TABLE`/`COLUMN` extraction from raw SQL |
| `ekos/crates/recovery/src/sql_analyzer.rs` | Phase 2 application + author-over-LLM precedence; Phase 3 budget, `with_max_tokens`, `SQL004` |
| `ekos/crates/recovery/src/lib.rs` | `pub mod sql_comments` |
| `ekos/crates/compiler-core/src/config.rs` | `[llm] max-tokens` |
| `ekos/crates/cli/src/commands/recover.rs` | Threads `config.llm.max_tokens` into `SqlAnalyzerPass` |

**Gates:** `cargo fmt --check` clean, `cargo clippy --workspace -- -D warnings` exit 0, **1877 tests pass**
(from 1838 at the start of the session), 0 failures.

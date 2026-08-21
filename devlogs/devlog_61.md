# Devlog 61 — Fixing the three real gaps devlog_60 found

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

devlog_60 ran the full EKOS pipeline cold against `analytics/` (Plausible Analytics) and honestly
reported three real gaps rather than fixing them in the moment: a Postgres `sqlparser` parse
failure, identity resolution over-merging real people/tables/documents, and `ekos ask` retrieval
failing on natural-language questions. This session fixes all three (RFC 0059, RFC 0060, RFC
0061), each with a real regression test built from the exact data that exposed the bug, and each
re-verified live against a genuinely cold rebuild of the same real repo — not just unit tests in
isolation. Net result on `analytics/`: the Postgres schema (42 real tables) is now recovered for
the first time; `ekos resolve` false-positive merges dropped from 19 to 8; a full-sentence question
that previously retrieved nothing now correctly answers from real evidence.

---

## RFC 0059 — Postgres `CREATE`/`ALTER SEQUENCE`, `UNLOGGED`, `NOT VALID`

### Problem
`priv/repo/structure.sql` (the real Postgres application schema — `sites`, `api_keys`, and every
other core table) failed to parse whole-file: `Expected: end of statement, found: INCREMENT at
Line: 116`. `SqlAnalyzerPass` discards every table in a file on any single statement's failure, so
EKOS had zero structured knowledge of this schema.

### Root cause
`sqlparser`'s `parse_create_sequence_options` checks `INCREMENT`/`MINVALUE`/`MAXVALUE`/`START`/
`CACHE`/`CYCLE` in that fixed order, **once each, no loop**. Real `pg_dump` output emits `START
WITH` *before* `INCREMENT BY` — the parser matches `START WITH 1` on its later-in-order check,
never gets back to `INCREMENT`, and leaves `INCREMENT BY 1 ...` unconsumed, which then fails the
caller's end-of-statement check. A real, still-open upstream ordering bug, not a missing grammar
rule. Investigating further (the same "keep re-running the scratch parse test after each fix" loop
RFC 0057→0058 used) found two more real gaps in the same file: `CREATE UNLOGGED TABLE` (the
dispatcher has no case for `UNLOGGED` at all, even though it's a real, tokenizable keyword used
elsewhere) and a trailing `NOT VALID` clause on `ADD CONSTRAINT ... CHECK (...)` (zero grammar for
it anywhere in the crate).

### Fix
Three preprocessing passes in `plugins/sql-dialect-postgres/src/lib.rs` (previously the identity
function — no preprocessing had ever been needed for Postgres before this file):
`strip_statements_starting_with` (generalizes RFC 0058's `strip_create_dictionary_statements` to
an arbitrary leading-keyword sequence) removes whole `CREATE`/`ALTER SEQUENCE` statements —
sequences were never modeled in the KIR anyway, same "nothing captured is lost" reasoning as RFC
0058's dictionaries. `strip_unlogged_before_table` and `strip_not_valid_clause` are narrower and
more careful: unlike sequences, `CREATE UNLOGGED TABLE`/`ALTER TABLE ... CHECK` carry real,
already-modeled information (a real table's real columns; a real constraint statement), so these
strip only the unsupported keyword/clause and keep the rest of the statement intact — strictly more
information-preserving than a whole-statement drop.

One implementation snag, found and fixed before finalizing: an early version of
`strip_statements_starting_with` wasn't aware of `pg_dump`'s `-- Name: x; Type: SEQUENCE; ...`
comment headers, which routinely contain both a literal `;` and the matched keyword text *inside
the comment itself* — this silently defeated the strip entirely on the real file (a statement-
boundary heuristic got confused by the comment's embedded `;`). Fixed by having the scanner copy
`--`-comments through to the next newline verbatim, never scanning their content for `;` or
keywords — the same class of "don't let literal punctuation inside quoted/commented text look like
real SQL structure" discipline RFC 0057/0058 already established for string literals.

### Verified live
Rebuilt `target/release/ekos`, reran `ekos recover` against the real `analytics/` repo:
`sql-analyzer` now reports `objects=42 relationships=0` for `priv/repo/structure.sql` (was 0
before). `ekos query find "public.sites"` / `"api_keys"` now return real compiled `Table` objects.

---

## RFC 0060 — Identity resolution: raise the merge threshold, strip Table schema qualifiers

### Problem
devlog_59/60 found `crates/identity`'s `DefaultResolver` (0.85 default `merge_threshold`, RFC
0007) merging genuinely distinct real objects across every kind that falls back to
`structural_score`'s flat `1.0` "no comparable data" signal: ClickHouse `imported_*` tables, real
git contributors with similar names, unrelated documents, unrelated CI pipelines. Two tests already
in the crate (`unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`,
`distinct_pdf_tables_in_one_document_do_not_all_merge`, added 2026-08-03) had already documented
one instance of this exact shape and were left `#[ignore]`d.

### Approach: verify with real numbers, not guesses
Before writing any fix, computed real Jaro-Winkler/Jaccard scores for 17 real pairs read directly
from `analytics/`'s git history and compiled schema — 8 `Person` proposals (3 genuinely correct
merges, 5 genuinely wrong), 5 `Table` proposals, 2 `Document`, 2 `Pipeline` — using the crate's own
`similarity` functions, via a scratch diagnostic test (written, run, and deleted once its answer
was captured — never committed). At 0.85, 16 of 17 known-wrong merges clear the bar. At 0.90, all
3 known-correct merges survive while 14 of 17 known-wrong ones are rejected. **No single threshold
on the current formula separates every case** — 3 pairs (e.g. `Build Private Images GHCR`/`Build
Public Images GHCR`, two real, different pipelines) score *higher* than one of the three
known-correct Person merges. This is stated plainly in the code rather than chased further on a
17-example sample.

A second, independent root cause was found while re-verifying against real schema-qualified names
(`plausible_events_db.imported_visitors`, not bare `imported_visitors`): the shared qualifier
prefix, present on every table in one source, inflates Jaro-Winkler's prefix bonus regardless of
the threshold — `imported_visitors`/`imported_browsers` scores 0.8905 name-similarity bare vs.
0.9507 fully qualified, enough to flip a merge decision at 0.90. The exact same shape
`unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`'s doc comment already named for file
paths ("block on the file basename rather than the full relative path"), just for SQL's dotted
qualifier convention.

### Fix
`DEFAULT_MERGE_THRESHOLD` raised 0.85→0.90. `name_for_similarity` strips a `Table`'s
schema/database qualifier (text after the last `.`, guarded against file-path-shaped names via a
`/`-exclusion check) before name comparison — scoped to `Table` only, since `Document` names are
paths with a different structure the threshold change already handles on its own.

### Verified live
Fresh cold rebuild of `analytics/` (both this fix and RFC 0059 present): `ekos resolve` merge
proposals dropped from **19 to 8**. All 5 known-wrong real `Table` proposals gone. 5 of 8
known-wrong `Person` proposals gone. All 3 known-correct `Person` merges intact.
`ekos query find "imported_browsers"` and `"Niklaas"` both now return the real objects directly —
previously absent, merged away under a different identity's name. **Not a complete fix**: the 3
residual known-wrong pairs from the diagnostic still merge, and the real re-run shows the 27-object
`Document` cluster from devlog_60 split into a 5-object and a 22-object cluster — smaller, not
eliminated. Documented honestly in RFC 0060 and `DEFAULT_MERGE_THRESHOLD`'s doc comment; the
natural next step (extending RFC 0029's reviewable-not-auto-merged flow to same-source merges) is
named as real follow-on work, not attempted here.

### Bonus: a pre-existing ignored test now passes
`unrelated_documents_sharing_a_folder_prefix_do_not_all_merge` (ignored since 2026-08-03) now
passes unmodified — the confidence in its original bug report (0.90) sat exactly at the new
threshold's boundary. Un-ignored, with its doc comment updated to explain why.
`distinct_pdf_tables_in_one_document_do_not_all_merge` remains ignored — a different failure shape
(deterministic `"{path}: table {n}"` naming scoring 0.99+ on name alone) this fix doesn't touch.

---

## RFC 0061 — `ekos ask`: extract search keywords from natural-language questions

### Problem
Every full-sentence question tested against `analytics/`'s compiled ledger retrieved zero context
and answered with an honest but wrong "I don't have enough information" — even for objects
trivially findable by name.

### Root cause
`Ledger::find_objects` (SQLite FTS5-backed) escapes any query containing a character outside
`[alphanumeric, space, *]` — including ordinary sentence punctuation like `?` — into one literal
FTS5 **phrase** query. A phrase query requires that exact text to appear contiguously in indexed
content, which no natural sentence ever does. `AiRuntime::gather_context` was passing the raw
question straight through with no translation, even though the same underlying search
(`ekos_search` over MCP) already tells callers in its own tool description to use "2-3 keywords,
not natural-language questions" — `ask` is specifically the one caller meant to accept natural
language, and translating it was missing.

### Fix
`AiRuntime::search_for_question`: extract keywords (stopword/punctuation-stripped, split on `_` to
match FTS5's own default tokenizer, deduped), try an FTS5 AND query first for precision, fall back
to OR for recall, fall back to the original raw question as a last resort (so any caller already
passing bare keywords — the pre-existing working case — sees identical behavior).

### Verified live
"Who is Niklas Hambüchen and what did they contribute to this repository?" now correctly retrieves
the real `Person` object and answers "Niklas Hambüchen is a contributor to this repository, having
made 2 commits" with real evidence citation — previously empty. Separately confirmed via `ekos
query find "columns OR imported OR browsers"` that the OR-fallback path this RFC adds correctly
surfaces `plausible_events_db.imported_browsers` for that question too — the LLM's own response to
that specific question was hard to parse (the small local `llama3:latest` model used in this
environment produced a confused, non-committal answer despite having the right context), a model-
capability confound distinct from the retrieval fix itself, worth naming plainly rather than
glossing over.

### What's still open
The related `README.md`-ambiguous-filename finding from devlog_60 (a different root cause —
relevance ranking among 25 same-basename matches, not phrase-escaping) is unaddressed. Genuinely
aggregate questions ("top contributors by commit count") still correctly retrieve nothing after
this fix — no keyword reformulation can satisfy a question with no single matching object; that
needs `ekos_ekl`, not `ekos_search`/`ask`, and an agent host that reads MCP's own tool descriptions
would already route there.

---

## Knowledge Captured

- **Verify threshold/scoring changes against real numbers computed from real data, not intuition.**
  The 0.90 identity threshold wasn't picked by feel — 17 real pairs were scored with the crate's
  own similarity functions first, and the threshold was chosen as the value minimizing
  misclassification against that real sample, with the residual imperfection stated honestly
  rather than hidden or chased with more tuning.
- **A "no comparable data" fallback that returns a *positive* score (rather than a neutral one) is
  a common, easy-to-miss class of bug.** `structural_score`'s flat `1.0` for "no columns to
  compare" was treated by the combined formula as confirming evidence of similarity, when it's
  actually the *absence* of evidence — the same shape can recur anywhere a multi-signal score
  falls back to "assume similar" instead of "no opinion" when one signal is unavailable.
  `crates/identity`'s Union-Find/blocking design already had four prior instances of this exact
  failure family (Section, TransformNode, RustSymbol/RustModule, Crate — each a *kind exclusion*);
  this one needed a *threshold/scoring* fix instead because, unlike those four, `Person`/`Document`/
  `Table`/`Pipeline` genuinely do have legitimate same-kind merges and can't be blanket-excluded.
- **A shared, uninformative prefix (folder path, schema qualifier, any convention every object in
  a source shares) inflates Jaro-Winkler independent of whatever threshold is chosen** — worth
  checking explicitly whenever name-similarity scoring is applied to naturally-qualified/namespaced
  names, not just tuning the number that gets compared against.
- **A retrieval bug and a model-quality issue can look identical from the outside** (both produce
  a confusing or wrong answer) but require completely different fixes and completely different
  verification — this session separated them explicitly (verifying retrieval via `ekos query find`
  independent of the LLM's response) rather than crediting or blaming the wrong layer.
- **`sqlparser`'s single-pass, fixed-order option parsers (`CREATE SEQUENCE`'s being the second one
  found in this codebase, after nothing analogous in `CREATE TABLE`) are a recurring shape of
  upstream gap** — worth checking for the same "checks each option once, in one fixed order, no
  loop" pattern before assuming a clause is simply unsupported.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0059-postgres-sequence-and-ddl-preprocessing.md` | New RFC |
| `ekos/docs/rfcs/0060-identity-resolution-merge-threshold.md` | New RFC |
| `ekos/docs/rfcs/0061-ai-runtime-question-keyword-extraction.md` | New RFC |
| `ekos/plugins/sql-dialect-postgres/src/lib.rs` | `strip_statements_starting_with`, `strip_unlogged_before_table`, `strip_not_valid_clause`, `preprocess_postgres_ddl`; 15 tests total |
| `ekos/plugins/sql-dialect-postgres/tests/fixtures/analytics-structure.sql` | Real, unmodified `analytics/priv/repo/structure.sql`, vendored as a regression fixture |
| `ekos/crates/identity/src/lib.rs` | `DEFAULT_MERGE_THRESHOLD` (0.85→0.90), `name_for_similarity`; 9 new/updated tests, 1 pre-existing test un-ignored |
| `ekos/crates/runtime/src/ai.rs` | `extract_search_terms`, `QUESTION_STOPWORDS`, `AiRuntime::search_for_question`; 6 new tests |
| `TODO.md` | Three items marked fixed, with honest "not a complete fix" notes preserved |
| (in `analytics/`, not this repo) `.ekos/ledger`, `.ekos/ckm` | Rebuilt from a genuinely cold state with all three fixes present |

## Still open (tracked, not silently dropped)

- **Identity resolution**: 3 of 17 known-wrong real pairs and two `Document` over-merge clusters
  still incorrectly merge. Extending RFC 0029's review-before-merge flow to same-source merges is
  the natural next step, not attempted here.
- **`ask` retrieval**: the `README.md`-ambiguous-filename ranking issue (relevance ranking among
  same-basename matches) is a separate, still-open root cause.
- **Elixir/Phoenix business logic**: still shallow file-level recovery only (devlog_60's finding),
  unrelated to any of this session's three fixes and not attempted here.

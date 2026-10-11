# Devlog 250 — Documentation pages as a source of checkable column claims

**Date:** 2026-10-11
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos migrate assess` checked documentation against the data, but only documentation written as a column comment. Teams also write these rules in a data dictionary: a Markdown file or a Confluence page. A sentence that names exactly one `table.column` in code formatting is now a third documentation source. Its claim is counted against the data like a comment's, and a contradicted one becomes a `doc_vs_data` conflict citing the page's `path:line`. This closes the last open item of RFC 0172.

---

## RFC 0172 — documentation pages

### What was built

| Component | |
|---|---|
| `recovery/src/column_mentions.rs` (new) | `parse_text` (Markdown), `parse_html` (Confluence), `without_mention`; pure |
| `local_docs_analyzer` (`v3`), `confluence_analyzer` (`v3`) | a `Section` / `Page` with mentions carries `column_mentions` and `source_path` |
| `commands/migrate.rs` | `page_column_docs`, `page_claims_for`; `assess_doc_claims` adds `doc_page` sources after the comments |

### Implementation details worth remembering

- **Recover records candidates, assess decides.** Recovery cannot know which tables exist. It keeps every sentence with one qualified identifier in code formatting; a mention of `` `schema.sql` `` is recorded and never matched, because no assessed table is called `schema` with a column `sql`.
- **One column per sentence.** "`invoice.total` must agree with `invoice.qty`" is about neither, so it is dropped at recovery.
- **A table row is one statement.** `| `invoice.qty` | always positive |` is read as a whole, where prose is split into sentences. A dot inside a code span does not end a sentence.
- **The claim is read without the column's name.** `extract` matches words such as "unique" and "required"; a column called `unique_id` would otherwise claim uniqueness about itself.
- **Fenced code is skipped.** A query names columns without saying anything about them.
- **Comments first.** At most one claim per kind and per column is checked, and the sources are tried in order: repository comment, live comment, pages. A page adds the kinds the comments do not state.
- **Only sections with a mention change.** The property is added only when non-empty, so every other `Section` object is byte-identical and appends nothing on re-commit. Both pass versions were bumped so cached recover output is not reused.

### Verified live

Sandbox `migrate-pg`, schema `docpages`, one repository comment and a Markdown data dictionary:

- 5 claims checked, 4 contradicted.
- `invnumber` never null: `sql/schema.sql:10` (the comment).
- `invnumber` unique, `status` one of open/closed/void: `docs/data-dictionary.md:5`, two sentences on one line.
- `qty` greater than 0: `docs/data-dictionary.md:9`, a table row.
- `discount` between 0 and 100 was checked and held: a finding, no conflict.
- The two-column sentence and the fenced query produced nothing.
- `ekos conflicts show` prints the page claim as `(docs/data-dictionary.md:5; doc_page)` beside the measured side and its query.

The fixture schema was dropped and the container stopped afterwards.

### Decisions

- **Code formatting is required.** Prose such as "the invoice status is one of …" names a column only by interpretation. Requiring `` `table.column` `` keeps this deterministic and keeps false conflicts out of a reviewer's queue.
- **No new conflict type or rule id.** A page claim is a `DQ.CONSIST.DOC` finding and a `doc_vs_data` conflict; only the claim's `source` differs.

---

## Knowledge Captured

- **`ekos migrate init` prints the `[migrate] project = …` lines; it does not write them.** Without `project` in `ekos.toml` every later command needs `--project`.
- **`--source-secret-env` on `init` is not enough for `discover`.** The connection alias needs `secret-env` under `[migrate.connections.<alias>]`, or the connect fails with "password missing".
- **A page claim is matched by table and column name only.** A data dictionary for another database with a same-named table would be read too. It is a TODO item.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/column_mentions.rs` | new module, 5 tests |
| `ekos/crates/recovery/src/{lib,local_docs_analyzer,confluence_analyzer}.rs` | mentions recorded, pass versions `v3`, 1 test |
| `ekos/crates/cli/src/commands/migrate.rs` | page sources in `assess_doc_claims`, 1 test |
| `ekos/docs/rfcs/0172-conflicting-evidence.md` | "Documentation pages" section |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | documented; TODO item ticked |

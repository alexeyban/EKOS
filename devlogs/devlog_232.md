# Devlog 232 — Routines and views, documented like code

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`Procedure` (RFC 0163) and `View` (RFC 0169) were in the ledger and linked, but invisible to
everything downstream. Generated docs had no pages for them, documentation coverage didn't count
them, docs couldn't link to them, and LLM descriptions skipped them. This closes RFC 0147's list of
downstream registries for both kinds, and adds the piece that makes the pages worth reading: the
schema author's own `COMMENT ON FUNCTION|PROCEDURE|VIEW` text, attached as an evidence-backed
`description`.

LedgerSMB `sql/` end to end: **556 routine pages and 17 view pages** in the curated docs.
**330 of 556 routines and 7 of 17 views carry the author's description**, which accounts for **all
330 function comments, and every view comment, in an observed file**. Views appear among the Data
Stores with the routines that use them, routines appear in API.md under their file, and 0 identity
conflicts.

---

## What was built

| Component | Change |
|---|---|
| `recovery/src/sql_comments.rs` | `extract_object_comments` (`COMMENT ON FUNCTION\|PROCEDURE\|[MATERIALIZED] VIEW`, routine argument lists kept) and `match_object_comments` (name, then arity; ambiguous → nothing; later wins). The table extractor is untouched. 2 tests |
| `recovery/src/plpgsql_analyzer.rs`, `view_analyzer.rs` | `with_file(key)` → `Contains` from the owning `File`; `source_span`; `description` + its own evidence at the comment's line. 2 tests. Logic versions bumped (cache) |
| `cli/commands/recover.rs` | The `File` key, computed exactly as `build` keys `File`s (base-relative, project-qualified) |
| `docs-gen` | `Procedure` + `View` entity pages; `Procedure` in API symbols; both doc-bearing; Data Stores lists views (`(view)`), shows bare-name columns, counts "used by N routine(s)/view(s)", and keeps routine statements out of the "transformation" counts. 1 test |
| `semantic/src/doc_links.rs` | `Procedure`, `View` in `CODE_KINDS` |
| `recovery/src/llm_description.rs` | `Procedure`, `View` in `SYMBOL_KINDS`; an end-to-end test through the real analyzers |

---

## Decisions

- **A separate extractor for routine and view comments.** `sql_analyzer` matches every
  `SqlComment` against `Table` names by bare name. If `COMMENT ON VIEW` had gone into the same
  `CommentTarget` enum, a view's comment would have landed on a same-named table.
- **Match by name, then by arity, and never guess.** LedgerSMB overloads routine names. A comment
  whose argument count still matches more than one overload is left unattached.
- **The `File` key is computed the way `build` computes it.** `ekos_common::project`'s docs record
  this rule being broken twice before (stripping only the workspace root). The key here is
  base-relative and project-qualified.
- **Data Stores excludes routine statements from "transformations".** Routine statements now write
  `ReadsFrom`/`WritesTo` onto tables (RFC 0163), so counting every such edge would have reported
  routine statements as transformations. The filter *excludes* `ProcedureStatement` sources rather
  than requiring `TransformNode` ones. An existing test passes transformation nodes only through
  the edges, and the exclusion keeps the old behaviour byte for byte.

---

## Verification

- **The silent-skip risk, pinned.** Listing a kind in `SYMBOL_KINDS` isn't enough:
  `llm_description` needs `source_span` *and* a `Contains` path up to a `File`, or it skips the
  object without a word. The new test builds a routine and a view through the real analyzers, with
  no hand-set properties, and asserts both are described from their real source, with the author's
  comment passed on. Mutation-checked: without the `File` edge, 1 of 2 is described and the test
  fails.
- **Real pipeline** on a fresh copy of LedgerSMB `sql/`, then `docs generate --layout curated`:
  - 595 entity pages = 556 `Procedure` + 17 `View` + 22 others. Pages are sharded into two-letter
    subdirectories, which briefly looked like only 60 routine pages.
  - `payment_bulk_post`'s page shows the author's text, lines 469–761, fidelity, its reads, writes
    and calls, and the one statement sqlparser can't parse, with the reason. `account_heading_tree`'s
    page lists the four reporting routines that use it.
  - Description coverage was **checked in the compiled model, not read off the pages.** Every page
    has a `## Definition` heading, falling back to the definition's evidence when there is no
    comment, so counting headings gave a false "556/556". The real figure is 330/556 routines, which
    matches the 330 `COMMENT ON FUNCTION` statements exactly. For views it is 7 of the 8 comments;
    the 8th sits in `changes/mc/views.sql@1`, whose extension is not `.sql`, so it's never observed.
  - `compile`'s warning count rose to 597. All of them are the documented, expected `File`-object
    references (the new `Contains` edges, resolved at `commit`), and 0 are anything else.
- `cargo test` for plpgsql, recovery, semantic, kir, identity, docs-gen and the `ekos` CLI: green.
  `clippy -D warnings` and `fmt --check` are clean. Both LedgerSMB corpus floors hold.

---

## Knowledge Captured

- **Verify a coverage number in the data, not in the rendering.** A rendered heading can mean
  "has a description" or "has a fallback". Count the property in the compiled model, then reconcile
  it against the source (here: 330 comments → 330 descriptions).
- **A downstream registry entry is only as good as the properties its consumer reads.** For
  `llm_description` that is `source_span` plus a `Contains` chain to a `File`. Test it with objects
  that come out of the real producer, never hand-built ones.
- **A file named `*.sql@1` is not SQL to EKOS**, because extension matching is exact. LedgerSMB
  keeps alternate migration versions that way.
- **docs-gen shards entity pages** into `entities/<kind>/<two-letter prefix>/`. Count with
  `find -type f`, not `ls`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/sql_comments.rs` | Routine/view comment extraction + matching, 2 tests |
| `ekos/crates/recovery/src/plpgsql_analyzer.rs`, `view_analyzer.rs` | File `Contains`, `source_span`, `description`; 2 tests |
| `ekos/crates/recovery/src/llm_description.rs` | `SYMBOL_KINDS` + end-to-end test |
| `ekos/crates/semantic/src/doc_links.rs` | `CODE_KINDS` |
| `ekos/crates/docs-gen/src/lib.rs` | Entity/symbol/doc-bearing lists; Data Stores views + counts; 1 test |
| `ekos/crates/cli/src/commands/recover.rs` | `File` key |
| `ekos/docs/rfcs/0163-…`, `0169-…` | Downstream-integration notes |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented |

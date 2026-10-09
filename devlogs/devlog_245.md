# Devlog 245 — Documentation vs data: RFC 0172 Phase 3 (`DQ.CONSIST.DOC`)

**Date:** 2026-10-09
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0158 promised a `DQ.CONSIST.DOC` rule: where documentation makes a checkable claim about a column, check it against the data. It waited on a `ConflictingEvidence` path that RFC 0172 has now built. `ekos migrate assess` reads each column's repository and live comments, extracts the checkable claims (never null, unique, one of a set, a numeric range), and counts the rows that contradict each one. Every claim becomes a finding, and a contradicted one becomes a `doc_vs_data` conflict carrying both sides. Verified live against the Migrate sandbox: 6 of 6 planted contradictions found, a declared `NOT NULL` skipped, and a claim-less comment ignored. A dismissal survives re-assessment, and a data fix drops the conflict.

---

## What was built

| Component | Change |
|---|---|
| `migrate-dq/src/doc_claims.rs` (new) | `extract(text) -> Vec<DocClaim>` (not null / unique / one of / range, with suppressing phrases), `Claim::violation_sql` (counts only, quoted identifiers, literals with doubled quotes), `describe`; 5 tests |
| `cli/src/commands/migrate.rs` | `assess_doc_claims` after the completeness report: repo comments (`path:line`) + live comments, skip `NOT NULL`-enforced claims, measure, write `DQ.CONSIST.DOC` findings, emit `doc_vs_data` conflicts on the repo `Table` (or the `MigrationUnit`), `report_doc_claims` |
| `semantic/src/conflicts.rs` | `DOC_VS_DATA`, public `conflict(...)` builder for detectors outside the module |
| `cli/src/commands/conflicts.rs` | `.ekos/conflicts/migrate.json` (table → ids), `record_migrate`; "current" is now the union of the commit and Migrate manifests; `show` prints the measured count and query |
| `pg-live/tests/live_doc_claims.rs` (new) | live: claims from real comments, every generated query run on PostgreSQL, exact counts asserted |
| RFC 0172 (Phase 3 section, status), RFC 0158 (delivered note) | docs |

## Live verification

Sandbox: `docker compose -f docker-compose.migrate.yml up -d migrate-pg`; fixture schema `docclaims` (CLI run) and `ekos_docclaims` (test).

- **First assess: 6 claims checked, 6 contradicted.**
  - `invnumber` never null and unique: from `sql/schema.sql:11`, the repository comment.
  - `status` one of open, closed, void: the live comment only.
  - `discount` between 0 and 100: `:12`.
  - `qty` positive: live.
  - `category` `A=asset,L=liability`: `:13`.
  - `ref` "never null" was skipped (declared `NOT NULL`), and `note` ("Free text notes") produced nothing.
- **Survives commit:** `ekos commit` kept all six (separate manifest), and `ekos status` showed `6 open`.
- **Review and data fix:** dismissed `status` ("draft was added in 2025; the comment is stale"), then fixed `qty` and `category` in the data and re-assessed. Result: 4 contradicted. The two fixed ones were no longer current, and `status` stayed `dismissed`.
- **Live test:** the inclusive bound (`100.00` within "between 0 and 100"), NULLs never violating `one_of`/`range`, `0` not positive, and a quoted identifier (`"Inv No"`) all behave as asserted.

## Decisions

- **Only checkable claims, and the opposite phrasing suppresses.** "May be null for drafts; never null once posted" yields nothing rather than a false `not_null`. A missed claim costs nothing; a misread one sends a reviewer chasing a contradiction that is not there.
- **Repository comment wins over the live one for the same claim kind.** It carries `path:line`. Comment-vs-comment disagreement is not this rule.
- **The measured side is qualitative** ("has NULL values"), and the count lives in `measured`. Otherwise every new row would change the signature and reopen a dismissed conflict.
- **Never blocking.** A stale comment is a documentation problem, not a reason to stop a migration unit; it is `warn`.
- **Own manifest.** `commit` rewrites `current.json`; doc-vs-data conflicts are produced by `assess`, so they live in `migrate.json` and are replaced per table on re-assessment.

---

## Knowledge Captured

- **`text::NOT IN` is how a fixed set is checked across types.** `char(1)` pads, `int` would need numeric literals; comparing `col::text` against string literals works for both. Verified live, including `char(1)` codes.
- **A declared constraint makes a documented claim untestable, not true-by-default.** Skip it rather than report "0 violations", which reads as evidence the documentation was checked.
- **`cargo test` lockfile churn.** Adding a dev-dependency changes both `ekos/Cargo.lock` and `tests/integration/Cargo.lock` (dev-deps are locked). Commit both, or `scripts/audit.sh`'s stale-lockfile check fails.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-dq/src/doc_claims.rs`, `lib.rs` | new module |
| `ekos/crates/cli/src/commands/migrate.rs` | `assess_doc_claims`, `report_doc_claims` |
| `ekos/crates/cli/src/commands/conflicts.rs` | Migrate manifest, union of current ids, measured in `show` |
| `ekos/crates/semantic/src/conflicts.rs` | `DOC_VS_DATA`, `conflict()` |
| `ekos/crates/pg-live/tests/live_doc_claims.rs`, `pg-live/Cargo.toml`, both `Cargo.lock` | live test + dev-dependency |
| `ekos/docs/rfcs/0172-*.md`, `0158-*.md` | Phase 3 section, delivered note |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | docs |

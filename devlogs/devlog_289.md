# Devlog 289 — Oracle profiles, log state, and three stale-state bugs (RFC 0211 Phase 4)

**Date:** 2026-10-23
**PRs:** none (local)
**Branch:** `main` (local, not pushed)

---

## Summary

RFC 0211's last phase measures the data without reading a value out of it. `[[databases]] profile`
chooses P0 (optimizer statistics, the default), P1 (exact row counts) or P2 (one aggregate scan per
table: exact null counts, number and date bounds, text lengths). On the iDempiere seed, P2 covers all
903 tables and 18,520 columns in 6 s, and a seeded marker row proves no text value leaves the crate.
The profile answers the question Phase 3 left open: of 116 "mandatory but nullable" columns, 11
hold nulls (the model's rule is broken by real rows) and 105 hold none — every count re-checked
against the database. Running it exposed three bugs that kept stale state alive: databases skipped
by the file fingerprint, every past catalog read merged at compile, and dropped tables never leaving.

---

## RFC 0211 Phase 4 — profiles and log state

### Problem / motivation
Phase 3 found 116 columns the model calls mandatory and the database leaves nullable, and could not
say whether any row actually breaks the rule. The CDC part of the plan also needs to know whether
archive logging and supplemental logging are on.

### What was built

| Component | What it does |
|---|---|
| `ora-live/src/profile.rs` | `ProfileTier`, `ScanSource`, `scan` (P1/P2 inside `SET TRANSACTION READ ONLY`), `log_state` (`V$DATABASE`, `DBA_LOG_GROUPS`; refusal = `unknown`) |
| `ora-live` catalog query `column_statistics` | P0 from `*_TAB_COL_STATISTICS`, read in the catalog snapshot; never `LOW_VALUE`/`HIGH_VALUE` |
| `ekos_common::pii` | moved from `pg-live` (re-exported there); also matches run-together names (`CREDITCARDNUMBER`) |
| `[[databases]] profile` | `p0` / `p1` / `p2`, default `p0` |
| `plugins/oracle` | profiles after the snapshot; catalog artifact carries `profile`, `log_state`, and the list of objects read |
| `OracleCatalogPass` | `profile` on tables and columns; `log_state` on `OracleCatalog`; drops objects the newest catalog no longer lists |
| `semantic::live_conflicts` | RFC 0172 `measured` block on `mandatory_but_nullable` (violating rows, rows, exactness, re-count query); summary split |
| `ekos conflicts show` | `measured_line` prints what a measurement contradicts and how it was counted |

### Measured (iDempiere 12 seed on Oracle XE 21c)

| | |
|---|---|
| P2, live suite | 903/903 tables, 18,520/18,520 columns exact, 6 s, 0 failed |
| P0 statistics | 901/903 tables analyzed, 18,471 columns with `NUM_NULLS` |
| bounds withheld by PII name rules | 32 columns |
| log state | `NOARCHIVELOG`, supplemental logging off, force logging off, 0 log groups |
| whole tree, `profile = "p2"` | `ekos build` 10 s; mandatory-but-nullable: 11 hold nulls, 105 none, 0 unmeasured, 116 exact |
| re-count check | 116 of 116 recorded counts equal an independent `SELECT COUNT(*) … IS NULL` |

Largest violations: `AD_PrintFormatItem.AD_PrintFormatChild_ID` (2,711 null rows),
`C_Currency.RoundOffFactor` (175), `C_BP_Customer_Acct.C_Receivable_Services_Acct` (18).

### Implementation details worth remembering
- A text column is never an argument of `MIN`/`MAX` — the rule is tested on the generated SQL
  (`no_text_column_is_ever_an_argument_of_min_or_max`), not on results.
- `COUNT(lob)` is `ORA-00932`; non-scalar columns are counted with
  `COUNT(CASE WHEN c IS NOT NULL THEN 1 END)`. `LONG` columns are skipped (`COUNT(long)` is illegal).
- Select lists split at 900 expressions (Oracle's limit is 1,000).
- An ODPI-C call timeout closes the session (`DPI-1067`), so after one the remaining tables are
  marked skipped instead of each failing.
- The marker gate seeds through a plain `oracle::Connection` in the test: `OraSource` has no write
  method and should not gain one.

### Decisions
- **P0 by default.** It scans nothing and already answers most null questions (statistics exist for
  901 of 903 tables), marked inexact. P1/P2 are opt-in per database.
- **`measured`, not a new conflict kind.** The disagreement (model mandatory, column nullable) is
  the same whether or not rows break it; the count is evidence about it, in the shape RFC 0172
  already uses for doc-vs-data.
- **PII rules shared, not copied.** `ora-live` cannot depend on `pg-live` (it pulls the `postgres`
  driver and the migrate crates), so the module moved to `ekos-common`.

---

## The three stale-state bugs

### Problem / motivation
The first whole-tree P2 run reported tier p0 and unknown log mode, and no conflict got a
measurement, though the artifacts held the profiles.

### What was found and fixed

| Bug | Fix |
|---|---|
| Database observers ran inside the per-source loop: skipped when the file fingerprint matched, and once per source directory | `read_databases`: once per build, always; failure is a build error; `--source` accepts a database name |
| `compile` dedupes a multi-input pass by its exact input set, so every past catalog read survived and was merged — which table version won depended on sort order | `KnowledgeContent::supersedes_earlier_runs` (skipped when false: existing ids unchanged); `compile` keys such output by pass name and drops that pass's older unflagged outputs |
| The artifact store keeps every version, and recover picks the newest per target — a dropped table has no newer version, so it would be recovered forever | the catalog artifact lists the objects it read; the pass drops the rest (`stale_objects`) |

### Decisions
- **Opt-in supersession.** Making every multi-input pass supersede by name could drop real output
  of a pass that writes several artifacts per run. Only a pass whose output is its whole answer
  sets the flag; `dependency_analyzer` probably qualifies and is left for a measured follow-up.

---

## Knowledge Captured

- **A live source must never sit behind a file fingerprint.** The build cache answers "did the
  files change"; a database changes without any file changing. Anything live runs every build.
- **Content-addressed stores need an explicit "current set" for live sources.** Newest-per-target
  handles change but never deletion: the catalog read has to say what exists now.
- **Multi-input passes accumulate at compile** unless their output declares that it supersedes. The
  symptom is subtle — the newest output is present, but an older one may win the merge.
- **The full `gvenzl/oracle-xe:21.3.0` image has the same single time-zone file (v35) as the slim
  one**, so it does not fix `ORA-30094` for the iDempiere seed's v26 data.
- **iDempiere's v12 seed is in `NOARCHIVELOG` with supplemental logging off** — LogMiner-based CDC
  needs a DBA change before anything else.
- `cognitive-complexity.sh` flags a function that grows past the limit; extracting the new block
  (`read_databases`) kept `run_selected` at its earlier 31.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/ora-live/src/profile.rs` | new: tiers, scans, log state, 6 tests |
| `ekos/crates/ora-live/src/{catalog,session,lib}.rs` | P0 statistics query; `ScanSource` for `OraSource` |
| `ekos/crates/ora-live/tests/live.rs` | marker-row gate (P2 + log state) |
| `ekos/crates/common/src/pii.rs` | moved from `pg-live`; run-together names |
| `ekos/crates/pg-live/src/lib.rs` | re-exports `ekos_common::pii` |
| `ekos/crates/compiler-core/src/{config,lib}.rs` | `DatabaseProfile`, `[[databases]] profile`; a stray doc line removed |
| `ekos/plugins/oracle/src/lib.rs` | profile + log state per owner; object list in the catalog artifact |
| `ekos/crates/cli/src/commands/build.rs` | `database_observers` + `read_databases`, outside the fingerprint loop |
| `ekos/crates/cli/src/commands/{recover,commit,conflicts}.rs` | profile line, nullable split, `measured_line` |
| `ekos/crates/recovery/src/oracle_catalog_analyzer.rs` | profiles carried, stale objects dropped, supersedes earlier runs, logic version 3 |
| `ekos/crates/artifact/src/lib.rs` | `KnowledgeContent::supersedes_earlier_runs` |
| `ekos/crates/semantic/src/lib.rs` | dedup by pass for superseding output |
| `ekos/crates/semantic/src/live_conflicts.rs` | `measured` on `mandatory_but_nullable`, stats split |
| `ekos/docs/rfcs/0211-live-oracle-connector.md` | §3.4 rewritten, Phase 4 results |

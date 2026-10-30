# Devlog 311 — Cross-reference integrity check, Phases 0–1 (RFC 0233)

**Date:** 2026-10-30
**PRs:** none (local `main`, not committed)
**Branch:** `main`

---

## Summary

EKOS already resolved names across systems (routine and view footprints, transformation steps, SQL in Java
methods) and dropped every name that resolved to nothing, or to several objects, in silence. RFC 0233, written
from the 2026-10-30 draft "Cross-Reference Integrity Check", turns those names into diagnostics:
`UnresolvedReference` / `AmbiguousReference` with the referring file and line, the closest names, the evidence, a
confidence and a fingerprint that survives recompiles. `ekos check refs` reports them (table, JSON, SARIF 2.1.0,
baselines, `--fail-on`) and `ekos commit` writes them to an integrity index file, never to the ledger. Measured on two
real repositories: LedgerSMB gives **2 errors, both real defects**. iDempiere without its database gives
**0 errors / 0 warnings / 609 info**, all checked by hand.

---

## RFC 0233 Phases 0–1

### Problem / motivation
`procedure_lineage` only counted ambiguous names in a commit line. `java_analyzer::link_sql` kept an unresolved
table only inside a method's `sql` property, and dbt logged and moved on. Those dropped names are what no
single-language linter sees: a Java query for a table no schema has, a routine writing a table a migration
dropped.

### What was built
| Component | What |
|---|---|
| `semantic/src/xref.rs` | the pure engine: reference collection (`Procedure`/`View` `reads`/`writes` with lines from `ProcedureStatement`s, `TransformNode` Source/Sink, `JavaMethod.sql[]`, dbt `source()` tables), resolution by `procedure_lineage::NameIndex`, the precision rules, metrics, fingerprints |
| `compiler-core` `[xref]` | `enabled` (default on), `min-confidence`, `ignore-file`, `[xref.severity]` per kind |
| `cli/src/commands/check.rs` | `ekos check refs` (`--scope`, `--min-confidence`, `--format table\|json\|sarif`, `--fail-on`, `--baseline <time\|report.json>`, `--show-suppressed`), `xref-ignore.toml` (reason mandatory), the commit step → `<state>/integrity/current.json` |
| `freshness::committed_manifest` | the source files the last commit saw; references from files no longer observed are skipped |
| catalogue | `check refs` → `read` |
| RFC | `ekos/docs/rfcs/0233-cross-reference-integrity-check.md` |

### Implementation details worth remembering
- The fingerprint is SHA-256 over kind, source kind, file, source name, access and the normalized target. It never
  uses ids or lines. A `TransformNode`'s name carries its step index, so its fingerprint leaves the name out. A
  test reverses the input, changes every id and moves every line.
- `--baseline <RFC 3339 time>` reruns the check over `all_objects_at(time)`: the ledger's own time travel, so no
  history is stored. `--baseline file.json` reads a saved report, for CI jobs that rebuild the ledger every run.
- A `JavaMethod` has no `source_path`. Its file comes from its first evidence record, through the `Options::locate`
  callback, so the engine stays pure.
- A SQL file's transformation steps repeat what its views' and routines' footprints name. A `sql-transform`
  reference is dropped when a footprint in the same file already names the same target with the same access.

### Decisions
- **Findings are not in the ledger.** They are derived and change every commit, and an append-only ledger would
  grow by every finding of every run forever.
- **`[xref]` in `ekos.toml`**, not the draft's `.ekos/xref.toml`: one validated, schema-documented config.
  Suppressions stay a separate file (`<state>/xref-ignore.toml`, the draft's path in a plain workspace).
- **Routine calls are out of Phase 1.** The footprint records built-ins (`coalesce`, `nextval`) as calls, and there
  is no per-dialect built-in list to tell them apart yet.
- **dbt: declared sources against the other systems' schemas**, not `ref()`: dbt itself already fails on a missing
  `ref()`. This needed no analyzer change.
- **The draft's "RFC 0136/0137" prerequisites are RFC 0135 Parts C/B here**, landed 2026-09-04. RFC 0136 in this
  repository is the console graph.

---

## Measured

| Repository | First run | Final | Notes |
|---|---|---|---|
| LedgerSMB (fresh compile, 1,366 refs) | 17 errors, 15 false | **2 errors, both real** | `budget__reject` deletes from `budget_to_project` / `budget_to_department`: no file creates them; the schema has `budget_to_business_unit` |
| iDempiere `master` bd4f848, code + scripts, no DB (11,966 refs, 3,126 Java → schema) | 597 errors, 1,349 warnings | 0 / 0 / 609 info | 336 from migration scripts, 265 coverage gaps, 8 test code; Java → schema resolution 97.7 % |

The false findings and the rule each one led to:

| False finding | Rule |
|---|---|
| `CREATE TEMPORARY TABLE` / `CREATE TABLE … AS` inside a routine (LedgerSMB 5) | relations a statement's `sql` creates are targets |
| `EXECUTE 'CREATE OR REPLACE VIEW file_links AS …'` | the dynamic statement's `expr` is scanned too |
| `SELECT * FROM report__begin()` as a transformation Source (5) | a `sql-transform` name matching a routine, overloads included, is resolved; `generate_series` & co. are system |
| a view in a module and in a migration (4) | an ambiguous name with exactly one definition in the reference's own file resolves |
| DDL `Table` + `ModelTable` of the same table (iDempiere 1,090) | a `ModelTable` is a claim, consulted only when no schema defines the name |
| one view in `db/oracle/views` and `db/postgresql/views` (256) | candidates with the same full name and kind are one relation |
| `*_trl`, `W_Click`, `W_Store` (577) | a table the migration scripts change but nothing defines is a coverage gap; one they drop is an **error** naming the script |
| views in migration scripts | a migration script describes the schema of its own time → info |
| `inexistent_table` in `DBTest.java` (8) | test code → info |
| `user_tab_partitions`, `user_ind_partitions` | more Oracle dictionary views |

---

## Knowledge Captured

- **A stale ledger shows up as false findings.** The demo `work/LedgerSMB` ledger (2026-10-10) predates RFC 0176's
  per-statement DDL fallback, and `holidays`/`overpayment` were "missing" there. A fresh compile defines both.
  Measure precision on a current compile only.
- **iDempiere's base schema is not in its repository as DDL.** It ships as a database dump. The `X_` classes
  cover most tables but not the `*_trl` translation tables or the old web-store `W_*` tables. Without the live
  catalog, iDempiere's missing tables cannot be decided, so error-level precision there needs the testbed with
  `[[databases]]`. Still open.
- **SQL transformation node names are `<path>#<statement>:<index>`**, so the file is the part before the `#`. Reading
  the whole name as a path made 509 references look like files no longer observed.
- `ProcedureStatement` carries `procedure`, `sql` (static) and `expr` (a dynamic `EXECUTE`'s argument). Use
  `procedure`, not the `name#n` prefix, to find the routine.
- Tables from `sql_analyzer` carry no `source_path`, only evidence. Views and routines do.
- `ekos check refs` takes 0.7 s on LedgerSMB. The commit step showed 0 s on both repositories (iDempiere: 135,615
  objects).

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0233-cross-reference-integrity-check.md` | new RFC, with the measured rules |
| `ekos/crates/semantic/src/xref.rs` | new: the engine + 19 tests |
| `ekos/crates/semantic/src/lib.rs` | `pub mod xref` |
| `ekos/crates/compiler-core/src/config.rs` | `XrefConfig` / `[xref]` |
| `ekos/crates/cli/src/commands/check.rs` | new: `ekos check refs`, suppression, baseline, SARIF, commit step + 5 tests |
| `ekos/crates/cli/src/app.rs` | `Commands::Check` / `CheckCommands::Refs`, dispatch, machine-output flag |
| `ekos/crates/cli/src/commands/{mod,catalogue,commit}.rs` | module, catalogue row, commit step + summary line |
| `ekos/crates/cli/src/freshness.rs` | `committed_manifest` |
| `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md` | the check documented; RFC counter 0233 |

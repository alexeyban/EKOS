# Devlog 343 — Talend projects in the ledger (RFC 0262–0264, Slice 1 Phase 1)

**Date:** 2026-11-21
**PRs:** none (pushed to `main` after local gates)
**Branch:** main

---

## Summary

EKOS reads Talend projects. Work started on the plan `talend-plugin-plan.md` (2026-11-19): Part 0 (fixtures and
an independent census), RFC 0262 (umbrella), RFC 0263 (the project model) and RFC 0264 Phase 1 (objects,
relationships, Transformation IR with column references). A real Talend Open Studio 8.0.1 project (fixture B:
14 jobs, 272 components, 232 links) reads exactly as the census counts, goes through `build → recover → compile →
commit` with 0 diagnostics and 0 model calls, all 128 data components are mapped, all 22 queries fold to SQL, and
every database input and output links to its DDL table. No Talend secret reaches `.ekos/`.

---

## Talend reader, observer, analyzer

### Problem / motivation
Talend Open Studio was retired in January 2024 and estates built with it are being migrated, usually to dbt. EKOS
reads Pentaho (RFC 0027, 0200–0205) but saw a Talend project as files only.

### What was built

| Component | What |
|---|---|
| `scripts/talend-fixtures.sh` | fetches fixtures A (documents only) and B (a real project) by commit hash into `.local/talend-fixtures/` (git-ignored) — never vendored, neither repository is cleared for redistribution |
| `scripts/talend-census.py` → `evals/talend/census.json` | counts only, with Python's own XML parser: the denominator for coverage and the oracle for the reader |
| `crates/talend` (`ekos-talend`) | pure model: `read_project(root, files)`; job items (components, TABLE parameters as rows, schemas, tMap, links, triggers, context groups, implicit context load), `.properties`, contexts, connections, schemas, routines; component catalogue (category, logical name, engine); Java string folding with named holes; `tRunJob` resolution; run-time context |
| `plugins/talend` | observer: one artifact per file of every directory holding `talend.project`, with project root and role; vendor files by path only |
| `recovery/src/talend_analyzer.rs` | 8 keyed kinds; `Contains`, `Calls`, `RowLink`, `Triggers`, `UsesConnection`, `UsesContext`, `UsesRoutine`; IR merged per job |
| `recovery/src/talend_ir.rs` | data components → Transformation IR; tMap → `Join` per lookup, `Filter` per filtered output, `Calculate` per output column with `column_refs` |
| `common/src/redaction.rs` | `talend-encrypted-value`, `talend-password-parameter`, `xml-password-attribute`; `PIPELINE_LOGIC_VERSION` 7 |
| `semantic/src/data_lineage.rs` | table names matched with identifier quotes removed |

### Implementation details worth remembering
- **A TABLE parameter is a flat list.** `elementValue{elementRef, value}` with no row element: a row ends when an
  `elementRef` already in the row comes again.
- **A lookup is a `FLOW` link with `lineStyle="8"`.** There is no `LOOKUP` connector name.
- **tMap wiring is by name.** An output links on through the outgoing connection's `metaname`; expressions refer
  to inputs by the incoming connections' labels (`row1`).
- **Implicit context load is per job** (`IMPLICIT_TCONTEXTLOAD` under `<parameters>`), on in 13 of 14 jobs.
- **A unique name does not say the type** (`tDBInput_1` is a `tPostgresqlInput`), and names repeat across jobs:
  ids are keyed by project root, job, version and unique name.
- **Connection items store `Password="context.password"`**, a reference: the attribute pattern keeps dotted
  identifiers, as `generic-assigned-secret` does.
- **A file input's name is a run-time value** (`globalMap.get("tFileList_1_CURRENT_FILEPATH")`): the Source
  records the iterating `tFileList` with its folded directory and masks instead of a guessed file.
- **The pass sets `supersedes_earlier_runs`:** it reads "all of Talend", so a deleted job must leave.

### Decisions
- **Row links are `RowLink`, not `FeedsInto`.** Ten consumers (dbt generation, docs-gen, `transformation explain`)
  assume `FeedsInto` joins `TransformNode`s.
- **Only data components are IR nodes.** Orchestration, connections, code and logging stay `TalendComponent`
  objects with triggers, so "mapped" is over data components (128 of 272 on fixture B), not diluted by `tPrejob`.
- **Kinds are a constant list with their own guard** (`KINDS`, `every_talend_kind_has_a_keyed_registry_row`,
  `every_emitted_kind_is_listed`): `every_pipeline_custom_kind_is_registered` sees only literal
  `ObjectKind::Custom("…")`.
- **Quoted DDL names now link.** `CREATE TABLE "DIM_CLIENT"` keeps its quotes in the `Table` name and Talend names
  `DIM_CLIENT`; `data_lineage` compared whole lowercased strings, so 0 of 43 outputs linked. It now strips
  `"…"`, `` `…` `` and `[…]` per dot-separated part. Matching stays unique-only and case-insensitive. This adds
  links for any source whose DDL quotes identifiers (on fixture B, 12 → 34 `ReadsFrom`, 0 → 43 `WritesTo`).
- Plan decisions taken as recommended: D1 knowledge path public, translation in the extension; D5 skip
  `poms/`; D7 fetch fixtures by hash.

---

## Knowledge Captured

- **The end-to-end run found what the unit tests could not.** `context_refs` sliced `text[i..]` at a byte inside
  a multi-byte character; the fixture's French comments (`é`) panicked `recover`. Every synthetic item was ASCII.
  The census-equality test passed because it never called `context_refs`. Run real fixtures through the binary,
  not only through the model.
- **Java identifiers are Unicode.** `context.année` is one name; matching ASCII only returned `ann`.
- **The IR's `Calculate` already has an `output` property** (`out.AMOUNT`); an extra property of the same name
  silently overwrote it. Extra keys from an analyzer must not reuse the IR's (`object_name`, `columns`, `output`,
  `excerpt`, `keys`, …): the tMap output name is `tmap_output`.
- **Generated Java beside a Talend project floods the ledger.** Fixture B's `poms/` holds 302,089 lines of
  generated Java, which the Java observer reads: of 7,563 objects committed only ~930 are Talend. Commit took
  ~10 minutes, mostly writing those. Excluding `poms/` under a Talend project is in TODO.
- **`ekos ekl … COUNT` prints the number on line 2**, not the last line (`1 row(s).` is the footer).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0262-talend-projects.md`, `0263-talend-project-model.md`, `0264-talend-in-the-ledger.md` | new RFCs |
| `ekos/crates/talend/**` | new crate `ekos-talend` + gated fixture-B tests |
| `ekos/plugins/talend/**` | new observer `ekos-plugin-talend` |
| `ekos/crates/recovery/src/talend_analyzer.rs`, `talend_ir.rs`, `lib.rs`, `Cargo.toml` | the pass and IR lowering |
| `ekos/crates/kir/src/custom_kinds.rs` | 8 `Talend*` rows |
| `ekos/crates/common/src/redaction.rs`, `lib.rs` | 3 patterns; `PIPELINE_LOGIC_VERSION` 7 |
| `ekos/crates/semantic/src/data_lineage.rs` | `name_key`: identifier quotes removed before matching |
| `ekos/crates/cli/src/commands/build.rs`, `recover.rs`, `Cargo.toml` | observer registered; pass wired; summary line |
| `ekos/Cargo.toml`, `Cargo.lock` | two workspace members |
| `scripts/talend-fixtures.sh`, `scripts/talend-census.py`, `evals/talend/census.json`, `.gitignore` | Part 0 |
| `README.md`, `CHANGELOG.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | docs |

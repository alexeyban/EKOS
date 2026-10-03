# Devlog 233 — RFC 0163 triggers: recovered, linked, and classified by what they do

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

RFC 0163's *Triggers* section is built. Every `CREATE [CONSTRAINT] TRIGGER` becomes a `Trigger`
object linked to the table it fires on and the function it runs, and is **classified structurally
from that function's recovered IR**, never from names: `Validation`, `DerivedColumn`, `Audit`,
`Cascade`, `Mixed` or `Unknown`, each with the reasons that produced it.

LedgerSMB (42 triggers, 17 functions): **15 Validation, 15 DerivedColumn, 4 Cascade, 7 Mixed,
1 Unknown**, 0 identity conflicts, and every trigger at one ledger version after a second commit.

The first real run gave **34 Unknown**. Three successive findings on real data took it to 1, and
none of them involved guessing which function body runs.

---

## What was built

| Component | Change |
|---|---|
| `recovery/src/trigger_analyzer.rs` | `TriggerAnalyzerPass`: timing, events (`UPDATE OF` columns), level, `WHEN`, function, constraint flag; token fallback when `sqlparser` can't parse. 5 tests |
| `recovery/src/sql_objects.rs` | New shared `file_kir_id` and `clip`, replacing copies in three analyzers |
| `recovery/src/plpgsql_footprint.rs` | `inserts`/`updates`/`deletes` alongside `writes`. 1 test |
| `recovery/src/plpgsql_analyzer.rs` | Trigger functions record `assigns_new`, `raises_exception`, `returns_null`, `pass_through`. 1 test |
| `semantic/src/triggers.rs` | Linking + classification, run inside `SemanticCompilerPass`. 6 tests |
| `semantic/src/procedure_lineage.rs` | `NameIndex` shared within the crate; `candidates()` |
| `identity` | `{View, Procedure, Trigger}` narrowing (was `{View, Procedure}`). 1 test |
| `kir` REGISTRY | `Trigger`, structurally keyed |
| `docs-gen` | `Trigger` entity pages |
| `cli/commands/recover.rs` | Wiring + `Triggers: N (P parsed, T read from tokens) — classified at compile` |

---

## From 34 Unknown to 1 — three findings on real data

1. **`resolve` failed with 6 conflicts.** A trigger named after its own function
   (`trigger_workflow_user` runs `trigger_workflow_user()`) is a near-universal convention, and
   trigger names live in a per-table namespace. The narrowing grew from `{View, Procedure}` to
   `{View, Procedure, Trigger}`. A `Table` in the group still conflicts.
2. **Most functions are defined more than once** (module + migrations), so "ambiguous → Unknown"
   was honest but useless. Now every definition is classified, and the class holds only if they all
   agree, because then it holds whichever body runs. If they disagree, the result is `Mixed`,
   naming each definition's class and file.
3. **Placeholders were outvoting real bodies.** LedgerSMB creates triggers against stub functions
   (`-- dummy; actual function defined in modules/triggers.sql` → `RETURN new;`, and in one migration
   an empty `BEGIN END;`). A body that is empty or only `RETURN NEW|OLD` carries no logic. When a
   real definition exists the stubs are set aside, and the reasons say so. A *conditional* return is
   deliberately not a placeholder (`Pg-database.sql`'s `gl_audit_trail_append` stub branches on
   `tg_op`), because `RETURN NULL` in a `BEFORE` trigger drops rows.

The remaining 7 `Mixed` are real: one function that sets `NEW`, raises, inserts *and* calls a helper;
two that only `NOTIFY`, or call a routine whose effects aren't inlined; and an audit function whose
4 definitions differ.

---

## Decisions

- **Classification at compile, not commit.** A trigger and its function are usually in different
  files, and both are first together in `SemanticCompilerPass`. Classifying there means each
  `Trigger` carries its class from its first ledger version, with no later re-write.
- **`Audit` is structural:** the function only *inserts* elsewhere. RFC 0163's "log-shaped table"
  would have needed a name heuristic.
- **`Unknown` is separate from `Mixed`.** "We can't see the body" and "the body does several things"
  are different findings, and RFC 0164 treats both as needing a human.
- **Calls into recovered routines make a trigger `Mixed`.** The callee's effects aren't inlined, so
  "only sets NEW columns" can't be claimed. Built-ins name no routine and don't count. This is
  conservative and stated in the reasons.
- **No `Calls` edge for an ambiguous function.** Classification can use all the candidates honestly;
  a single edge would assert one of them.

---

## Knowledge Captured

- **sqlparser 0.53 requires `FOR EACH` in `CREATE TRIGGER`, but PostgreSQL defaults to `STATEMENT`
  without it.** The token fallback reads timing, events and level, so such a trigger is still fully
  described.
- **sqlparser 0.53 parses `UPDATE` inside a CTE but not `DELETE`.** A unit test written around a
  `WITH … AS (DELETE …)` silently tests nothing, because the whole statement comes back empty.
- **Schemas create triggers against placeholder functions and replace them later.** Any analysis
  that combines definitions across files has to recognise placeholders, or the stub outvotes the
  real body.
- **Measure a new classifier's distribution on real data before trusting it.** 34 of 42 `Unknown`
  looked "honest" and was mostly a missing rule.

---

## Verification

- 14 new tests (5 analyzer, 1 footprint, 1 routine facts, 6 classification, 1 identity), with every
  class, the multi-definition agree/disagree cases, placeholder handling and determinism covered.
- LedgerSMB end to end on a fresh copy (no LLM key reachable): 42 triggers all parsed, classes as
  above, 42 `DependsOn` (fires-on) and 8 `Calls` edges, 0 conflicts. After a second full run,
  `ekos ledger audit` shows 42 × "1 version", and the re-run writes only the pre-existing baseline
  (6 objects / 2 relationships: rollups and the duplicate-id tables).
- `cargo test` for plpgsql, recovery, semantic, kir, identity, docs-gen and the `ekos` CLI: green.
  `clippy -D warnings` and `fmt --check` are clean. Both LedgerSMB corpus floors hold.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/trigger_analyzer.rs`, `sql_objects.rs` | New |
| `ekos/crates/recovery/src/plpgsql_footprint.rs`, `plpgsql_analyzer.rs`, `view_analyzer.rs`, `lib.rs` | Per-op writes, trigger facts, shared helpers |
| `ekos/crates/semantic/src/triggers.rs`, `procedure_lineage.rs`, `lib.rs` | Linking + classification |
| `ekos/crates/identity/src/lib.rs` | Narrowing extended to `Trigger` |
| `ekos/crates/kir/src/custom_kinds.rs` | `Trigger` row |
| `ekos/crates/docs-gen/src/lib.rs` | `Trigger` pages |
| `ekos/crates/cli/src/commands/recover.rs` | Wiring + summary |
| `ekos/docs/rfcs/0163-plpgsql-procedural-ir.md` | Criterion ticked; Amendment (d) |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Capability documented |

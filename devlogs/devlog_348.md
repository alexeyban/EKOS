# Devlog 348 — Talend Slice 3 begins: the ETL IR shared with Kettle (RFC 0266)

**Date:** 2026-11-21
**PRs:** none (pushed to `main` in both repositories)
**Branch:** main (EKOS); main (ekos-lakehouse, `95c1b02`, `a16af69`; lakehouse devlog_24)

---

## Summary

Slice 3 of the Talend plan translates Talend jobs to dbt, in the extension (decision D1). The plan's decision D2
was to reuse the Pentaho translator's typed expressions, Java parser, relational IR and SQL printer by
extracting them into engine-neutral crates, not copying them.

RFC 0266 does that in ekos-lakehouse, with two new crates, `etl-expr` and `etl-rel`. It is gated on the Pentaho
translator's output, which stays byte-identical on three corpora.

---

## RFC 0266 — engine-neutral expression and relational IR

### Problem / motivation
A Talend translator needs the same pieces the Pentaho one has. Two copies of an IR drift apart, and every
rendering fix would have to be made twice.

### What was built

| Piece | Where |
|---|---|
| RFC 0266 | `ekos/docs/rfcs/0266-engine-neutral-etl-ir.md` |
| `etl-expr`: `Expr`, `Func`, `KType`, `Fidelity`, `Gap`, `Lowered`, the Java lexer, parser and statements | ekos-lakehouse `crates/etl-expr` |
| `etl-rel`: the IR (`Op`, `RelNode`, `Ir`, …), `Catalog`, `Connection`, `render` (IR → SQL) | ekos-lakehouse `crates/etl-rel` |
| Golden gate: hashes of every file written, the report and each IR | `crates/pentaho-dbt/tests/golden.rs`, `tests/golden/` |
| Guard: the neutral crates depend on no tool's crate | `crates/etl-rel/tests/neutral.rs` |

### Implementation details worth remembering
- The moved modules' old files are one-line `pub use` re-exports, so `ekos_lakehouse_kettle_expr::Expr` and every
  other path still compile. Serialized forms do not change.
- `java.rs` splits at `NOTE_NPE`: syntax above, Kettle's meaning (the `Lowerer`) below. Talend's lowering will be
  a separate `Lowerer` over the same parser, because Talend runs plain Java: `==` compares references, there is
  no empty-string-is-null rule, and a null dereference raises a `NullPointerException`.

### Decisions
- **Golden manifests of hashes, not output files.** The corpora are third-party, so nothing from them is
  vendored. Hashes catch every byte of difference without storing any of the text.
- **`KType` keeps its name.** A neutral name can come later, as a rename with no change in behaviour.

---

## Results

| Corpus | Manifest lines | After the move |
|---|---|---|
| bench-gold (in the repo, CI) | 5 | identical |
| Pentaho samples (`pentaho-kettle` @ `67c23984`, 127 `.ktr` + 17 `.kjb`) | 286 | identical |
| AdventureWorks (8 transformations, 2 jobs) | 48 | identical |

The lakehouse workspace's tests, `clippy -D warnings` and `fmt --check` pass.

---

## Knowledge Captured

- **Inherent methods have to move with the type.** Rust lets only the crate that defines a type add inherent
  methods to it. Moving a type out of a crate breaks every `impl Type { … }` left behind; such methods become free
  functions (`iso_dow`, `note_only_logging`).
- **The golden check lives in the lakehouse repo.** Run it locally with
  `EKOS_PENTAHO_GOLDEN_CORPORA="samples=<dir>:adventureworks=<dir>"`. Capture only before a change that must not
  alter output (`EKOS_PENTAHO_GOLDEN=capture`).
- **The Pentaho samples are sparse-fetched** into `EKOS/.local/pentaho-samples/pk`, which is ignored.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0266-engine-neutral-etl-ir.md` | new RFC |
| `CLAUDE.md`, `TODO.md` | RFC pointer, lakehouse crate list, TODO |
| ekos-lakehouse (see its devlog_24) | `etl-expr`, `etl-rel`, re-exports, golden gate |

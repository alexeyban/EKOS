# RFC 0266 — Engine-neutral expression and relational IR, extracted from the Kettle crates

**Status:** Accepted (2026-11-21)
**Umbrella:** RFC 0262 (Slice 3; decision D2). **Builds on:** RFC 0202 (Kettle expressions and relational IR), RFC 0204 (Pentaho → dbt).
**Code:** `ekos-lakehouse` — new crates `etl-expr`, `etl-rel`; `kettle-expr`, `kettle-rel`, `pentaho-dbt` changed

## Problem

Slice 3 translates Talend jobs to dbt. Most of what that needs already exists in the Pentaho translator, but it
lives in crates named, and partly built, for Kettle:
- a typed expression tree with a closed function set and Trino rendering;
- a fidelity label and a gap with its text;
- a Java expression and statement parser;
- a relational IR (`Source`, `Project`, `Filter`, `Join`, `Lookup`, `Aggregate`, `Window`, `Sink`, …);
- the printer from that IR to dbt SQL.

The plan's decision D2 is to extract rather than copy: two copies of an IR drift, and every fix would have to be
made twice.

## Design

### What moves, and where

| New crate | Taken from | Contents |
|---|---|---|
| `ekos-lakehouse-etl-expr` | `kettle-expr` | `expr.rs` (`Expr`, `Func`, `Lit`, `BinOp`, `UnOp`, `Unit`, `infer`, quoting, Trino printing), `types.rs` (the value type, still named `KType` so no caller changes), `Fidelity`, `Gap` and `Lowered` from `semantics.rs`, and the Java lexer, expression parser and statement parser (`java::{Tok, lex, lex_with, JExpr, Parser, parse_expr, JType}`, `stmt.rs`) |
| `ekos-lakehouse-etl-rel` | `kettle-rel`, `kettle` | `model.rs` (`Field`, `TableRef`, `SourceQuery`, `JoinKind`, `Op`, `RelNode`, `Ir`, `Draft`, `Sink`, …) and the database `Connection` description |

What stays Kettle's:
- in `kettle-expr`, the Kettle condition tree, Calculator, Formula and JavaScript-step parsers, the Java
  `Lowerer` (it applies Kettle's rules: empty string is null, null-safe comparison, null-skipping
  concatenation), and `kettle_compare` / `kettle_concat`;
- in `kettle-rel`, the step lowering (`build.rs`, `steps.rs`, `source.rs`);
- in `pentaho-dbt`, the medallion layout, the catalogue and drafting.

The printer from the IR to SQL (`pentaho-dbt/src/render.rs`) reads only the IR, so it moves to `etl-rel`
(`etl_rel::render`). A Talend translation then prints through the same code.

### No caller changes

`kettle-expr` and `kettle-rel` re-export what moved (`pub use ekos_lakehouse_etl_expr::…`). `kettle` re-exports
`Connection` from `etl-rel`, so `kettle` now depends on `etl-rel`, a pure crate with `serde` only. Every existing
`use ekos_lakehouse_kettle_expr::Expr` keeps compiling. Serialized forms do not change: the moved types keep
their names, fields and serde attributes.

### Golden gate

This is a pure move, and it is checked as one. Before any code moves, the Pentaho translator's output is
captured on three corpora:
- `pentaho-dbt`'s test package;
- Pentaho's own samples (`pentaho-kettle` `assemblies/samples`, fetched by commit);
- the AdventureWorks package (`etl_adventureworks_sales_purchases_datamart`).

The capture is a manifest of every written file's path and SHA-256, plus the per-step report. It holds hashes
only: the corpora are third-party and never vendored. After the move, the same run must produce the same
manifest byte for byte.

The check is `scripts/pentaho-golden.sh capture|check <corpus>…`, in the lakehouse repo, with the manifests
under `tests/golden/`. The in-repo corpus (`bench-gold`) is checked by every `cargo test --workspace`. The lakehouse repository has no
CI workflow, so that run is local and comes before each push. The other two corpora are run locally too, and the
result is recorded in the devlog.

## Not in scope

- Talend's own lowering, which is RFC 0267. Talend's Java semantics (`==` on references, `equals`, null
  propagation) differ from Kettle's, so Talend gets its own `Lowerer` over the shared parser.
- Renaming `KType`. A neutral name can come later; it is a rename with no change in behaviour.
- Moving the medallion layout out of `pentaho-dbt`. RFC 0268 decides what a Talend project's dbt shape is.

## Exit

- The three corpora's manifests are identical before and after the move.
- `cargo test --workspace`, `clippy -D warnings` and `fmt --check` pass in `ekos-lakehouse`.
- `etl-expr` and `etl-rel` depend on neither `kettle` nor `pentaho-dbt`, and a test checks that from their
  `Cargo.toml`.

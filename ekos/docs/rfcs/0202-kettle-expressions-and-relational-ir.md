# RFC 0202 — Kettle expressions and the relational IR

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0200 (umbrella), RFC 0201 (deep Kettle model)
**Code:** `ekos-lakehouse` — `crates/kettle-expr`, `crates/kettle-rel`

## Goal

Turn every step whose meaning is relational into a typed node of a relational IR, with typed
expressions, so RFC 0204 can print Trino SQL without guessing — and say, for every step, how
faithful that is.

## Expressions (`kettle-expr`)

One typed `Expr` (column, literal, unary/binary operators, function call over a closed set of
canonical functions, `CASE`, `CAST`, null test, `IN` list, `LIKE`, regex match) with a type per
node from Kettle's types (`String`, `Number` = double, `BigNumber` = decimal, `Integer` = bigint,
`Date`, `Timestamp`, `Boolean`, `Binary`). Each source of expressions has its own parser:

| Source | Parser |
|---|---|
| Filter rows / Java filter conditions | the `<condition>` tree; Kettle evaluates a condition list **left to right without precedence**, so the IR nests it that way |
| Calculator | a table of `calc_type` codes, each a function of fields A, B, C |
| Formula step | an OpenFormula (LibFormula) parser: `[field]`, `;` separators, `&`, `=`/`<>` |
| User Defined Java Expression, Java filter | a Java expression parser (shared with RFC 0203's UDJC statements) |
| Value mapper, String operations, Replace in string, If null, Null if, Number range, Concat fields, Add constants, Set field value, Select values | the step's own fields |

### Kettle semantics made explicit

Each rule below is applied by the lowering and named in the step's notes, so a reviewer sees
every place the SQL differs in form from the Kettle step:

- **Empty string is null.** Kettle (default `KETTLE_EMPTY_STRING_DIFFERS_FROM_NULL=N`) reads `''`
  as null. String columns entering a translation pass through `nullif(c, '')`; a `''` literal is
  null.
- **Comparisons treat null as a value.** Kettle's `=` holds for two nulls and null sorts first.
  `=` and `<>` become `IS NOT DISTINCT FROM` / `IS DISTINCT FROM`; `<`, `<=`, `>`, `>=` get an
  explicit null branch when either side can be null.
- **String concatenation skips nulls** (`concat` over `coalesce(x, '')`, then `nullif(…, '')`).
- **Day of week** is Sunday = 1 … Saturday = 7 (Java `Calendar`), not ISO.
- **Rounding** of exact halves: Kettle's rule differs between `ROUND` and `ROUND_STD`; Trino rounds
  half away from zero. These steps are `rewritten`, never `exact`.
- **Locale-dependent** results (week of year, date formats without a mask) are `rewritten` with
  the assumption stated.

Anything outside the parsers' grammar — an unknown function, a method call on a Java object the
parser does not know — is a **gap** with the exact text that stopped it. Nothing is passed through
as raw text into SQL.

## Relational IR (`kettle-rel`)

Per transformation, a DAG of nodes, each carrying the step it came from, its output schema
(field name → type) and a fidelity:

`Source` (silver/bronze relation or inline rows), `Project` (expressions, renames, drops,
retypes), `Filter`, `Route` (Switch/Case and Filter rows with two targets: one input, several
outputs, each with its predicate), `Join` (inner/left/right/full/cross, key pairs), `Lookup`
(stream or database lookup: left join with defaults, first match), `Aggregate`, `Window`
(analytic query, sequences, `row_number`), `Distinct` (unique rows, with or without count),
`Sort`, `Union` (append streams), `Unpivot`/`Pivot` (normaliser/denormaliser), `Sink` (table
output, insert/update, update, delete, dimension lookup/update, combination lookup), and `Gap`
(the step, its reason and its original configuration).

Schema propagation runs in hop order. A step whose input schema is unknown (a file input, an
unknown plugin) makes everything downstream depend on a gap; the IR still records the rest so the
report can say exactly what blocks what.

Kettle's row-order semantics are recorded, not assumed: a step that depends on the order rows
arrive in (Unique rows without sort, Add sequence, Analytic query, Group by on unsorted input,
Identify last row) is lowered with the order its upstream Sort rows gives, and is `rewritten`
when there is none (the SQL needs an explicit order the Kettle step took from arrival).

## Fidelity per step

`exact` | `rewritten` | `llm-drafted` | `gap`, as RFC 0200 §2. The IR records the reason for every
label below `exact`.

## Tests

Golden tests per parser and per step type (fixture → IR → Trino SQL), one per semantic rule
above, and coverage on the RFC 0201 corpora: the share of steps per category that lowers to a
non-gap node, reported in the devlog.

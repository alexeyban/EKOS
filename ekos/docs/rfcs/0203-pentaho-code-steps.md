# RFC 0203 — Pentaho code steps: JavaScript, Java classes, compiled plugins

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0200 (umbrella, `JavaDecompiler` seam), RFC 0202 (expressions, IR), RFC 0204
**Code:** `ekos-lakehouse` — `kettle-expr` (`stmt.rs`, `script.rs`), `kettle-rel`, `pentaho-dbt`,
`lakehouse-ext`; compiled code: private `ekos-binary` behind the seam (RFC 0206)

## Goal

Carry the logic of Modified Java Script Value, User Defined Java Class and compiled plugin steps
into SQL wherever it is a function of one row, and say exactly what stopped it everywhere else.

## One front end, two languages

The statement subset these steps use in practice — local declarations, assignments, `if`/`else`,
blocks, expression statements, `return` — has the same grammar in Java and JavaScript. Both are
parsed by one hand-written parser (`kettle-expr/src/stmt.rs`, on RFC 0202's Java lexer, with a
JavaScript mode for `'strings'`, `===`, `!==`, `var`/`let`/`const`). `oxc_parser` was the first
plan; it was dropped because the subset is small, a second AST would need a second lowering, and
the semantic differences belong in one executor, not two parsers. A construct outside the subset
(loops, `switch`, `try`, function declarations, regex literals, `new`) stops the step with its name.

## Symbolic execution (`script.rs`)

The transform script runs once per row, so the executor walks it once with every input field as
a symbolic value and keeps, per variable, the SQL expression it holds:

- an assignment replaces the variable's expression;
- `if (c) A else B` runs both branches and merges every variable either one assigned into
  `CASE WHEN c THEN a ELSE b END` (a variable a branch did not touch keeps its prior value);
- dropping the row (`trans_Status = SKIP_TRANSFORMATION` in JavaScript; a `putRow` skipped on a
  path in Java) accumulates a **skip predicate**, which becomes a Filter after the Project;
- logging calls (`writeToLog`, `logBasic` …) are dropped, noted.

Language semantics are the executor's, by a `Lang` flag:

| | JavaScript (Rhino) | Java |
|---|---|---|
| `/` | always floating point | Integer ÷ Integer truncates |
| `+` with a string | concatenation, null prints `"null"` | the same |
| `==` | value equality; `null == null` | identity on objects: a gap |
| `if (x)` | JS truthiness: null, `0`, `''` are false | Boolean only (a null stops the step) |
| `x.substring(a, b)`, `.length` / `.length()` | 0-based | 0-based |

Kettle's JavaScript built-ins that are translated: `upper`, `lower`, `trim`, `ltrim`, `rtrim`,
`isNull`, `str2num`, `num2str`, `str2date`, `date2str`, `replace`, `indexOf`, `getDigitsOnly`,
`removeDigits`, `abs`, `ceil`, `floor`, `round`, plus `Math.*`, `parseInt`, `parseFloat`,
`String()`, `Number()`. Any other call is a gap naming it.

A step whose **start script** does more than declare constants keeps state across rows: a gap.
Compatibility mode (`compatible = Y`, the Kettle 2 API) is a gap.

### User Defined Java Class

The executor runs `processRow` with Kettle's idioms recognised: `Object[] r = getRow()`, the
`if (r == null) { setOutputDone(); return false; }` guard, `createOutputRow`,
`get(Fields.In, "f").getString(r)` (and `getInteger`, `getNumber`, `getBigNumber`, `getDate`,
`getBoolean`) as a typed field read, `get(Fields.Out, "f").setValue(r, v)` as an output
assignment, `putRow(data.outputRowMeta, r)` as the row leaving (once per path, or a gap), and
`getParameter("P")` as the step's own parameter value. A call to another method of the class, a
field of the class (state across rows) or any other API is a gap naming it. A User Defined Java
Expression (RFC 0202) uses the same expression lowering.

## Compiled plugin steps

A step whose type is declared by a `plugin.xml` in the package is compiled code. The translate
command asks `Extensions::java_decompiler()` (RFC 0200) for its class: with the private
ekos-binary build present, the decompiled `processRow` source is run through the same executor
(its fidelity capped at `rewritten`, since a decompiler's output is a reconstruction) and, either
way, the source is kept as the step's original text for the reviewer and for RFC 0205's model. In
the public build the step is a gap: "compiled plugin `X` (decompiling it needs ekos-binary)".

## Fidelity

`exact` only when every construct maps without a rule; the semantic rules above make a step
`rewritten` with each one named. Everything the executor cannot follow is a `gap` with the
construct and its line.

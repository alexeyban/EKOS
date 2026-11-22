# RFC 0267 — Talend expressions, component semantics and the relational lowering

**Status:** Accepted (2026-11-21)
**Umbrella:** RFC 0262 (Slice 3). **Builds on:** RFC 0263 (model), RFC 0266 (shared IR).
**Code:** `ekos-lakehouse` — new crate `talend-rel`, depending on EKOS's `ekos-talend` (git) and on `etl-expr` / `etl-rel`

## Problem

A Talend job has to become relations before it can become dbt models (RFC 0268). The plan's Parts V.1–V.3 set
out how:
- Talend's Java expressions become typed SQL expressions, with every place where Java and SQL differ named;
- each component becomes nodes of the shared relational IR, with a fidelity label and, when it cannot be
  translated, a gap with the text that stopped it.

## Design

### Crate

`ekos-lakehouse-talend-rel` is pure. Its input is an `ekos_talend::Job`, already expanded with its joblets inlined
(`Project::expand`), and an `etl_rel::Catalog` for the tables it reads. Its output is an `etl_rel::Ir` per job.
It holds two parts:
- `expr`: Talend's Java lowering over `etl-expr`'s parser;
- `lower`: the component lowering.

### Expressions (`expr`)

The Java text is parsed by `etl-expr`'s parser (RFC 0266). A Talend `Lowerer` then resolves names:

| Name | Resolution |
|---|---|
| `<row>.<col>` | the column of the input whose link label is `<row>` (tMap); `input_row.<col>` in tJavaRow |
| `Var.<v>` | the tMap variable's expression, inlined |
| `context.<v>` | the variable's default value as a literal, `rewritten`, with the note that a run may replace it (RFC 0264 `overridable_at_runtime`) |
| `globalMap.get("…")` | a gap naming the key: the value exists only at run time |

Typing comes from the input schemas (`id_String` → string, `id_Integer`/`id_Long`/`id_Short`/`id_Byte` →
integer, `id_Float`/`id_Double` → number, `id_BigDecimal` → decimal, `id_Boolean`, `id_Date` → timestamp). Any
other type (`id_Object`, `id_List`, `id_Dynamic`, `id_byte[]`) is unknown, and an expression that needs it is a
gap.

**Talend's rules, each named in the node's notes:**

- **No empty-string rule.** Unlike Kettle, Talend runs plain Java: `""` is not null.
- **`a.equals(b)`** throws when `a` is null, while SQL yields unknown. It is lowered to `IS NOT DISTINCT FROM`,
  `rewritten`, with that note.
- **`==` and `!=` on strings or boxed values** compare references in Java. They are lowered as equality,
  `rewritten`, with a note that a person should read the original.
- **`+` with a string operand** is concatenation, and Java prints a null operand as `null`. It is lowered to
  `concat` with `coalesce(x, 'null')` for each operand that can be null, `rewritten`.
- **Integer division** follows Java's static types: `a / b` on two integers truncates.
- **Routines** use a closed catalogue:
  - `TalendDate.getPartOfDate(part, d)`: `MONTH` is zero-based; `DAY_OF_WEEK` is Sunday = 1.
  - `TalendDate.getCurrentDate()` is the run's start time, `rewritten`.
  - `TalendDate.addDate`, `TalendDate.diffDate`.
  - `TalendDate.formatDate(pattern, d)` with a pattern of `y`, `M`, `d`, `H`, `m` and `s` only. Month names and
    week numbers depend on the JVM locale, so those patterns are gaps.
  - `TalendDate.parseDate`.
  - `StringHandling.UPCASE`, `DOWNCASE`, `TRIM`, `LEFT`, `RIGHT`, `LEN`, `INDEX`, `CHANGE`.
  - `Numeric.sequence(…)`: a gap here, because it depends on row order. The component lowering records the order
    when the flow has one.
  - `Relational.ISNULL`, `Mathematical.ABS`.

  Any other routine is a gap naming it.
- **`java.lang`** uses a closed list:
  - `equals`, `equalsIgnoreCase`, `compareTo`, `length`, `substring`, `trim`, `toUpperCase`, `toLowerCase`,
    `contains`, `startsWith`, `endsWith`, `replace`, `isEmpty`;
  - `Integer.parseInt`, `Double.parseDouble`, `String.valueOf`, `Math.abs`, `Math.round`, `Math.floor`,
    `Math.ceil`.
- Anything else is a gap with the exact text.

### Components (`lower`)

The job's data components become one or more nodes each, in hop order:

| Component | Nodes |
|---|---|
| database input | `Source` with its folded SQL (`SourceQuery::Sql`), `exact` when the SQL folded completely; a folded table name alone gives `SELECT <columns> FROM <table>` |
| ELT input | `Source` on its table |
| file input | `Source` naming the file or `tFileList` mask; a file has no catalogue, so its schema is the component's own |
| tMap | the main input, then per lookup a `Lookup` (`UNIQUE_MATCH`: the last match; `FIRST_MATCH`: the first; `ALL_MATCHES`: a `Join`), inner when `innerJoin`; per output a `Filter` (its filter) and a `Project` (its expressions); an inner-join reject output takes the main rows with no match (a left `Lookup` and a null-key filter) |
| tFilterRow | `Filter`; its REJECT link reads a second `Filter` with the negated predicate |
| tUniqRow | `Distinct` on the key columns (UNIQUE), and its DUPLICATE output as a gap: the duplicates are every row but the first, by arrival order |
| tAggregateRow | `Aggregate` (count, sum, avg, min, max, first, last, count distinct) |
| tSortRow | `Sort` |
| tHashOutput → tHashInput, tBufferOutput | the same relation, referenced |
| ELT map | `Join`s in input order and a `Project` per output. The SQL expressions are read through sqlparser into `etl-expr` |
| database / ELT output | `Sink`: `INSERT` → `Insert`, `UPDATE` → `Update`, `INSERT_OR_UPDATE` / `UPDATE_OR_INSERT` → `Upsert`, `DELETE` → `Delete`; keys from the schema's key columns |
| tFileOutput*, tLogRow, tJava, tWarn, tDie, orchestration and connection components | no node: they are listed in the report as not part of the data flow, or as tests (tDie) |
| everything else | `Gap` naming the component type |

**Matching mode.** Talend's documentation says `UNIQUE_MATCH` keeps the last matching lookup row. Fixture B's
generated Java (under `poms/`) is the oracle: the lowering stays `rewritten` until a test reads that Java and
confirms the rule.

### Coverage

For each fixture, the run reports:
- data components lowered to a node other than a gap;
- expressions lowered, by kind;
- gaps, by reason, as counts.

Coverage is claimed only for component types present in a real fixture (RFC 0262 gates).

## Phases

1. **This RFC's first delivery.** Expressions, and the tMap, filter, aggregate, sort, unique, hash and database
   input/output lowering, measured on fixtures B–E.
2. ELT maps through sqlparser, tJavaRow statements (`output_row.x = …` only), and tFilterRow's simple condition
   table.
3. RFC 0268: idioms, the dbt project, and the differential run.

## Exit (phase 1)

- **Fixture B:** every tMap expression in the DWH jobs lowers without a gap, and
  `FAIT_VENTE.NB_MT_VENTE`'s node reads `sum(QTE_VENTE * PRIX_VENTE)`.
- **Coverage** on fixtures B–E is reported in the devlog, by component type and by gap reason.
- **Pure:** a test checks that `talend-rel` reads no file.

## Result (phase 1, 2026-11-21, devlog_349; lakehouse `28dee30`)

| Fixture | Data components lowered | Expressions lowered / gaps |
|---|---|---|
| B | 116 / 125 (93%) | 475 / 11 |
| C | 79 / 91 (87%) | 144 / 6 |
| D | 26 / 42 (62%) | 40 / 2 |
| E | 199 / 274 (73%) | 337 / 12 |

Fixture B's exit holds: every DWH tMap lowers, and `NB_MT_VENTE` is written from a `sum` over a column a map
computes as `QTE_VENTE * PRIX_VENTE`.

**Amendments from the run:**
- **`jobName`** is the job's own name, a literal.
- **`globalMap` keys:**
  - a key every `tSetGlobalVar` sets to a constant becomes that constant;
  - a `tFileList`'s `CURRENT_FILEPATH` / `CURRENT_FILE` is read from the landed table's `$path`, `rewritten`;
  - any other key is a gap naming it.
- **tUniqRow's DUPLICATE output** is a per-key row number above 1, `rewritten`: arrival order has to be stated.
- **Disconnected components.** A component with no row link in that produces no rows is outside the data flow and
  goes on the skipped list, not counted as a gap. A SQL statement component (`tDBRow`, `tSnowflakeRow`, ELT) never
  is, because it changes data from a trigger.
- **`ekos-talend`** now lifts a component-framework input's manual query into `QUERY` (EKOS `89cedb25`).

**Left for phase 2:**
- ELT maps (21 in E);
- `tSnowflakeRow` / `tDBRow` statements (42 in E);
- `Numeric.sequence` (row order);
- custom routines (`HashUtils.generateMD5`, `HashUtil.*`);
- `tNormalize`, `tSampleRow`, `tMysqlSCD`.

## Phase 2 design (2026-11-22)

Surveyed on fixtures B–E before any code. Counts only, because E names a real account.

### SQL expressions (`sql`)

ELT maps and statements hold SQL, not Java. `sql` parses one expression with sqlparser in the
component's engine dialect (Snowflake, PostgreSQL, Oracle, SQL Server and MySQL; generic otherwise)
and lowers it to `etl-expr`, by a whitelist like the Java one:
- columns `<alias>.<column>`, resolved by the same column lookup as Java;
- literals, arithmetic, `||`, comparisons, `AND` / `OR` / `NOT`, `IS [NOT] NULL`, `IN (…)`,
  `BETWEEN`, `LIKE`, `CASE`, `CAST` / `::`;
- functions:
  - `COALESCE` / `IFNULL` / `NVL`, `NULLIF`;
  - `IFF` and `DECODE` (as `CASE`);
  - `TRIM` / `LTRIM` / `RTRIM`, `UPPER`, `LOWER`, `LENGTH` / `LEN`, `SUBSTR` / `SUBSTRING`,
    `REPLACE`, `CONCAT`;
  - `ABS`, `ROUND`, `FLOOR`, `CEIL`;
  - `CURRENT_TIMESTAMP` / `CURRENT_DATE` / `SYSDATE` (the run's start, `rewritten`);
  - `TO_DATE` / `TO_TIMESTAMP` with a format of numeric fields only (`YYYY`, `MM`, `DD`, `HH24`,
    `MI`, `SS`), `MD5`.
- the aggregates `SUM`, `COUNT`, `MIN`, `MAX` and `AVG`, only where a `GROUP BY` allows them.

Two Snowflake functions have no equal on another engine:
- **`HASH(…)`** is Snowflake's own 64-bit hash. It is lowered to the MD5 of the same values joined
  as text, `rewritten`, with a note: the keys are new values, the same rows get the same key, and a
  differential run (RFC 0268) must not compare them with the old table's keys.
- **`TO_VARIANT(x)`** is lowered to a cast to text, `rewritten`.

Anything else is a gap with its text.

### ELT maps

A `tELTMap` takes its inputs from `tELTInput`s, bound by link label as phase 1 does. It then
lowers like a tMap:
- the first input (`NO_JOIN`) and each later one renamed `<alias>__<column>`;
- a `Join` per later input, in input order: `INNER_JOIN`, `LEFT_OUTER_JOIN`, `RIGHT_OUTER_JOIN`,
  `FULL_OUTER_JOIN` or `CROSS_JOIN`. The join entries (`<alias>.<column> <op> <expression>`)
  become its condition.
- per output:
  - a `Filter` (its `WHERE` filters, joined by `AND`);
  - a `GROUP BY` clause, which becomes a `Project` of the key and argument expressions, then an
    `Aggregate`. Each output column must then be an aggregate, or equal to a key.
  - an `ORDER BY` clause, which becomes a `Sort`. Any other clause is a gap.
  - a `Project` of the output's expressions.

A `tELTOutput` reads the map's output through its link's `metaname`.

### Statements

A `tDBRow` or `tXRow` (category `elt`, logical `db_row`) runs one SQL statement from a trigger.
`ekos-talend` lifts a component-framework Row's query into `QUERY`, since a Row has no `manualQuery`
setting (EKOS change). The statement parses in the engine's dialect and becomes a new shared node:
`Op::Statement { verb, target, reads, set, predicate, sql }`.
- **`UPDATE`:** its `SET` and `WHERE` expressions are lowered through `sql`.
- **`DELETE`:** its `WHERE` is lowered through `sql`.
- **`INSERT … SELECT`, `MERGE` and `TRUNCATE`:** only the verb and the tables are kept.

The node is `rewritten`, with a note that the statement runs on a trigger, not on rows.

A statement that does not fold (a run-time value in its text), that is several statements, or that
is not one of those verbs is a gap.

Kettle never produces this node, so `pentaho-dbt`'s output does not change.

### `tJavaRow` and run-time context

A `tJavaRow`'s code is parsed by `etl-expr`'s statement parser. Four forms are allowed:
- `output_row.<c> = <expr>;`, which becomes a `Project`. A column the code does not assign is
  null, as in Talend.
- `context.<v> = input_row.<c>;`, which sets a context variable from data. It is recorded as a
  `RuntimeContext { var, component, column, from }`.
- `System.out.println(…)`, which is dropped with a note;
- `if (…) { System.out.println(…); }`, also dropped with a note.

Anything else is a gap.

A context variable that a `tJavaRow` sets is **known only at run time**. Every query or expression
that reads it is `rewritten` (never `exact`), with a note naming the component and column that set
it.

This is the watermark pattern RFC 0268 turns into an incremental model. Fixture E has 14 such
`tJavaRow`s, and 12 database inputs filter on what they set.

### Row order

- **`Numeric.sequence("s", start, step)`** in a tMap becomes a `Window` (`Sequence`) column on the
  combined row, read by the expression.
  - **Order:** when the main input comes straight from a `tSortRow`, the window uses its keys and
    is `exact`. Otherwise the order is arrival, so it is `rewritten`, with the note that the order
    must be stated.
  - **Sharing:** Talend shares one sequence by name across components. Two uses of one name in a
    job are a gap.
- **`tSampleRow`** keeps line numbers: a `Window` number, then a `Filter` on the listed numbers and
  ranges, `rewritten` because of arrival order.

### Custom routines

`lower_job` now also takes the project's routines.
- **Digest routines:** a custom routine method is lowered when its body is a digest of its one
  `String` parameter in lower-case hex: `MessageDigest.getInstance("MD5" | "SHA-1" | "SHA-256")`,
  `digest(<p>.getBytes(…))`, then hex by `Integer.toHexString` or `String.format("%02x")`. It
  becomes `Md5Hex`, `Sha1Hex` or `Sha256Hex`, `rewritten`.
- **Notes:**
  - a body without a null check stops the job on null (`NOTE_NPE`);
  - `getBytes()` without a charset uses the platform's, while the translation hashes UTF-8.
- **Anything else** stays a gap naming the routine. All three custom routines in the fixtures are
  digests (fixtures C and E).

### `UNIQUE_MATCH`

Fixture B's generated Java passes `MATCHING_MODE.UNIQUE_MATCH` to `AdvancedMemoryLookup`. That
class is in a Talend library jar the fixture does not contain. The only lookup source in the
fixture is `routines/system/DocumentLookupCache.java`, which is tXMLMap's: there, `UNIQUE_MATCH` and
`FIRST_MATCH` both `HashMap.put`, so the last row put wins. That is evidence, not proof, for tMap:
its lookups stay `rewritten`. A gated test reads the file and fails if either changes.

### Not in phase 2

- **`tNormalize`:** splitting a column into rows needs an unnest node the shared IR lacks.
- **`tMysqlSCD`:** its type 1, 2 and 3 fields need a wider `Scd`. RFC 0268 decides its shape,
  next to dbt snapshots.
- **Inlining a general routine body:** no fixture has one that is not a digest.
- **`tJavaFlex`:** its loop state (counters, running sums) is not a row expression.

### Exit (phase 2)

- On fixture E, every `tELTMap` and every `tSnowflakeRow` lowers without a gap.
- Every expression gap that remains is one this section names as not lowered.
- The 12 watermark queries in E are `rewritten` and name their `tJavaRow`.
- Coverage on B–E is reported in the devlog, against phase 1's.


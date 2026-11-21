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

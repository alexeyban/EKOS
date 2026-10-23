# RFC 0209 — Java Source Analyzer, Application Model Recognizer, Embedded SQL, Ticket Anchors

**Status:** Accepted
**Date:** 2026-10-22
**Depends on:** RFC 0041 (observer/analyzer split), RFC 0088 (`source_span`), RFC 0092 (`Extends`),
RFC 0135 Part D (`custom_kinds` registry), RFC 0140 §1 (`KirEvidence` per span), RFC 0141 §1/§4
(`signature`, `symbol_kind`), RFC 0147 (Perl — the hand-written-scanner and embedded-SQL precedent),
RFC 0163 (`statement_footprint`), RFC 0172 (`ConflictingEvidence`)
**Plan:** `oracle-idempiere-plan.md` v3, Part I.1–I.3 and I.8 (ticket anchors)
**Testbed:** iDempiere `release-13` @ `3355eb5fc9403c3827fcd8caf8eb92f78319430e`

---

## 1. Motivation

EKOS has no Java source analyzer. `dependency_analyzer.rs` reads `import` lines for a few database
drivers; RFC 0206's JVM recovery reads bytecode and lives in the private build. Yet Java is the
language of most of the legacy enterprise estates EKOS exists for, and the plan's testbed —
iDempiere, an ERP whose ~900 tables are described by its own source code — is a Java application.

Measured on the pinned testbed:

| What | Count |
|---|---|
| `.java` files | 4,542 |
| generated model classes `X_*.java` (`/** Generated Model - DO NOT CHANGE */`) | 763 |
| generated interfaces `I_*.java` (`/** Generated Interface for … */`) | 764 |
| business classes `M* extends X_*` | 506 |
| classes implementing `DocAction` | 30 |
| `DB.prepareStatement/executeUpdate*/getSQLValue*` call sites | ~2,500 |
| `new Query(ctx, table, where, trx)` call sites | 723 |

The generated classes **are** the Application Dictionary rendered as code: table identity, every
column, its business name and description, its list values with labels, which columns are mandatory
with what defaults, which are not updatable, and which reference which table. Reading them gives the
data model's semantics with no database connection and no model call.

## 2. Goals / Non-goals

**Goals**

- One `ObservationArtifact` per `.java` file, verbatim, no parsing in the observer.
- A deterministic declaration-level parse: package, imports, types (class / interface / enum /
  record / annotation, nested), `extends`/`implements`, annotations, methods and constructors with
  spans and signatures, Javadoc as `description`, `static final` constants with literal initialisers.
- An application-model recognizer for the iDempiere/ADempiere/Compiere family that turns the
  generated classes into `ModelTable` objects.
- An embedded-SQL harvester: SQL assembled in method bodies and passed to `DB.*` or `new Query`,
  folded from literals and constants, parsed, and recorded as per-method reads/writes/deletes.
- Ticket anchors: `IDEMPIERE-NNNN` style ids in file names, comments and Javadoc as exact facts.
- Counts of everything **not** understood, per file and per run.

**Non-goals**

- A Java type checker or a full grammar. Method bodies are **scanned, not parsed**.
- A `Calls` graph. Receiver types of locals are unknown without type inference; a call graph from
  name matching would be confidently wrong on the overloaded, inherited code this estate is full of.
- Reading Application Dictionary rows (`AD_Table`, `AD_Column`) from a live database — plan
  decision D3: semantics come from the generated code; live rows are an opt-in cross-check later.
- Oracle-specific SQL syntax. Embedded SQL is parsed with `sqlparser`'s generic dialect here;
  RFC 0210's `sql-dialect-oracle` preprocess plugs into the same seam when it lands, and the parse
  status of every statement is recorded so the improvement is measurable.

## 3. Why a hand-written parser

No mature pure-Rust Java parser exists. `tree-sitter-java` needs a C toolchain, which the
workspace's parser policy (RFC 0031/0038: `sqlparser`, `rustpython-parser`, `syn`, `oxc`) rejects.
Unlike Perl, Java **is** statically parseable at declaration level: comments, string/char/text-block
literals and braces are lexically unambiguous, so a real tokenizer plus brace matching yields exact
spans. Only expressions inside bodies are left unparsed.

## 4. Observer — `plugins/java`

`JavaObserver`, the `PerlObserver` shape: walk, ignore patterns, bare-file `observe_paths`, sha256,
verbatim `source`. Selection: extension `.java` only.

## 5. Analyzer — `JavaAnalyzerPass` (`crates/recovery/src/java_analyzer.rs` + `java_syntax.rs`)

### 5.1 Kinds

| Kind | Identity | Keyed |
|---|---|---|
| `JavaType` | fully qualified name (`org.compiere.model.MInvoice`, nested `Outer.Inner`) | yes |
| `JavaMethod` | (owning type id, name, parameter types as written) — overloads are distinct | yes |
| `ModelTable` | (application family, table name) — `idempiere:C_Invoice` | yes |

The object `name` is the simple name (`MInvoice`, `completeIt`, `C_Invoice`); `qualified_name`
holds the full one. Fields are **not** objects: `static final` literals are a `constants` map on
their type, the RFC 0147 `use constant` precedent — 763 classes × ~80 `COLUMNNAME_*` constants as
objects would bury every real member.

### 5.2 What is recovered

- `package` → `package` property on each type. `import a.b.C;` → `DependsOn` from the file's
  top-level type to the `JavaType` id of `a.b.C` (same id scheme as declarations, so internal
  imports land on the real type). Wildcard and `static` imports are counted, not edged.
- `class X extends Y implements A, B` → `Extends` to `Y` and `Custom("Implements")` to `A`, `B`.
  The target is resolved to a fully qualified name through the file's own imports, then its own
  package (Java's rule for an unqualified name), never guessed beyond that.
- Methods and constructors → `JavaMethod` with `symbol_kind` (`method` / `constructor` /
  `abstract`), `visibility` (the keyword; package-private when none), `static`, `signature` (the
  declaration header, whitespace-normalised), `annotations`, `source_span`, `KirEvidence`.
  `Contains` edge from the owning type; nested types `Contains` from their outer type; top-level
  types `Contains` from their `File`.
- Javadoc (`/** … */` immediately before a declaration) → `description` (text up to the first
  block tag), and `@param` lines kept for the recognizer.
- **Generated files** (header `Generated Model - DO NOT CHANGE`, `Generated Interface for`, or a
  `@Generated` annotation) produce their types with `generated: true` and `member_count`, but no
  `JavaMethod` objects: their accessors are recovered as columns of a `ModelTable` (§6) instead of
  ~120,000 getter/setter objects.

### 5.3 Fidelity

Each file is `parsed` (every brace matched, every member classified), `partial` (some member header
not classified — counted per file as `unclassified_members`), or `skipped` (unbalanced braces or no
type declaration). Stats: files by status, types, methods, constants, generated types.

## 6. Application model recognizer (`crates/recovery/src/app_model.rs`)

Narrow trigger: a type annotated `@org.adempiere.base.Model(table="…")`, or an interface declaring
`Table_Name = "…"` and `Table_ID = …`. Other Java estates never meet the trigger.

One `ModelTable` per table, merged from its `I_` interface and `X_` class:

| Source in code | Recovered |
|---|---|
| `Table_Name`, `Table_ID`, `accessLevel` | `table_name`, `table_id`, `access_level` |
| `COLUMNNAME_X = "X"` | a column `X` |
| getter return type (`int`, `BigDecimal`, `String`, `Timestamp`, `boolean`) | `java_type`; `boolean` getters over `CHAR(1)` are flagged `boolean: true` |
| setter Javadoc `Set Grand Total.` / `@param GrandTotal Total amount of document` | `business_name`, `description` |
| `X_AD_Reference_ID=131` + `/** Completed = CO */ X_Completed = "CO"` | `reference_id`, `values: {"CO": "Completed", …}` |
| reference getter `MTable.get(…, I_C_BPartner.Table_ID)` | `references: "C_BPartner"` — a **code-declared** logical FK |
| commented constructor block `if (…_ID == 0) { setX(…); // default }` | `mandatory: true`, `default` as written (`@#Date@`, `CO`) |
| `set_ValueNoCheck` in the setter | `updatable: false` |

Edges: `X_` and `I_` types `Custom("Models")` → `ModelTable`; each `M*` class extending an `X_`
class also `Custom("Models")` → it, marked `business_class`. A `ModelTable` →
`ModelTable` `References` edge per code-declared FK, `properties.column` naming the column. A
business class implementing `DocAction` carries `document_lifecycle`: which of `prepareIt`,
`approveIt`, `rejectIt`, `completeIt`, `voidIt`, `closeIt`, `reverseCorrectIt`, `reverseAccrualIt`,
`reActivateIt`, `invalidateIt`, `unlockIt` it declares, with their spans.

Business names feed the glossary: each column's `business_name` is a meaning source for RFC 0170
synthesis in a later phase (not in this RFC — RFC 0170's inputs are listed and guarded there).

A `ModelTable` is a **claim by code**. It is never merged into a database `Table`: the catalog is
the authority, and RFC 0211 compares the two into `ConflictingEvidence`.

## 7. Embedded SQL harvester (`crates/recovery/src/java_sql.rs`)

Within each method body, tokens are scanned in order:

1. `String v = <expr>;`, `v = <expr>;`, `v += <expr>;`, `StringBuilder v = new StringBuilder(<expr>)`,
   `v.append(<expr>)` (chained) maintain a per-method map of local SQL text.
2. `<expr>` folds `+`-joined parts: string literals, constants resolved in the file, through the
   type's `extends`/`implements` chain (so `Table_Name` and `COLUMNNAME_*` resolve inside `M*`),
   and qualified `I_C_Invoice.COLUMNNAME_GrandTotal`. Any other part becomes the placeholder
   `ekos_expr` and marks the statement `partial`.
3. At a call to `DB.prepareStatement`, `DB.executeUpdate*`, `DB.getSQLValue*`, `DB.getSQLArrayObjectsEx`,
   `DB.getIDsEx`, `DB.getKeyNamePairsEx` or `DB.getSQLValueObjectsEx`, the argument that is a SQL
   literal or a tracked variable is taken (the SQL argument is the first or the second, after the
   transaction name). `new Query(ctx, <table>, <where>, trx)` becomes `SELECT * FROM <table> WHERE
   <where>`.
4. The text is parsed with `statement_footprint_in(GenericDialect)`; the result on the method:
   `sql: [{line, call, text, status, reads, writes, inserts, updates, deletes}]`, `status` one of
   `parsed` / `partial` (placeholders) / `unparsed` (parser error kept).

Writes through the persistence layer — `po.save()`, `po.delete()` on a model class — are not SQL;
RFC 0212 adds them.

## 8. Ticket anchors

`IDEMPIERE-\d+` and `ADEMPIERE-\d+` (a fixed list in v1; making it a config list waits for a
second estate that needs one) found in a Java file's comments or Javadoc are recorded as
`tickets` on the innermost declaration whose span holds them — a method, else the type — and a
file's header comment goes to its first top-level type. Exact matches only. Migration-script file
names and commit messages are RFC 0210 / `plugins/git`.

## 9. Integration

| Location | Change |
|---|---|
| `ekos/Cargo.toml`, `crates/cli/Cargo.toml`, `commands/build.rs` | `plugins/java`, `JavaObserver` |
| `commands/recover.rs` | collect `java` artifacts, register `JavaAnalyzerPass`, print stats |
| `kir::custom_kinds::REGISTRY` (+ guard list) | `JavaType`, `JavaMethod`, `ModelTable`, keyed |
| `identity::is_expected_cross_namespace_group` | namespace `java` (any mix: a constructor shares its class's name) and `appmodel` (`ModelTable` beside a same-named `Table` is the point) |
| `semantic::doc_links::CODE_KINDS`, `llm_description` lists, `docs-gen` lists | the Java kinds |

## 10. Phases

1. Observer, syntax, `JavaType`/`JavaMethod`, registry/identity/docs integration.
2. Application model recognizer.
3. Embedded SQL harvester.
4. Ticket anchors.

Each phase ends with a measured run on the pinned testbed: files by status, objects by kind, and
the counts of what was not understood.

## 11. Testing

Lexer: comments, string/char/text-block literals containing braces and quotes, generics with `>>`.
Parser: every type form, nested types, enum bodies with constant bodies, records, annotation
types, constructors vs methods, abstract and interface methods, initializer blocks, anonymous
classes and lambdas inside bodies. Recognizer: a trimmed real `I_`/`X_`/`M*` triple. SQL: literal
folding, constants through inheritance, `StringBuilder` chains, `new Query`, a non-literal fragment
yielding `partial`. A malformed file never panics.

## 12. Alternatives considered

- **`tree-sitter-java`.** Rejected: C toolchain (§3).
- **Every member as an object, generated or not.** Rejected: 120k accessor objects that restate the
  `ModelTable` columns, and drown search and identity.
- **Fold `ModelTable` into `Table`.** Rejected: the code's model and the database's catalog are two
  sources that disagree in exactly the cases worth finding (RFC 0211).
- **A name-matched `Calls` graph.** Rejected: §2.

# Devlog 286 — Java source analyzer and the iDempiere application model (RFC 0209)

**Date:** 2026-10-22
**PRs:** none (local branch)
**Branch:** `feat/rfc-0209-java-analyzer` (not merged)

---

## Summary

First work on the Oracle/iDempiere plan (v3, RFCs 0209–0218): EKOS core gets its first Java
**source** analyzer, entirely deterministic and with no model call. A hand-written tokenizer and
declaration-level parser reads all 4,542 Java files of iDempiere `release-13` with **0 partial and
0 skipped**. A recognizer for the iDempiere/ADempiere family turns the generated `I_*`/`X_*`
classes into 763 `ModelTable` objects carrying 16,660 columns, their business names, list values,
mandatory columns with defaults, updatability and 2,882 code-declared foreign keys. Embedded SQL
in method bodies is folded from literals and constants (through inheritance), parsed, and linked
to those tables, and ticket ids (`IDEMPIERE-NNNN`) are anchored to the declaration they sit in.

---

## RFC 0209 — Java analyzer, application model, embedded SQL, ticket anchors

### Problem / motivation
The plan's testbed is iDempiere, a Java ERP whose ~900 tables are described by its own generated
code. EKOS had no Java source analyzer: `dependency_analyzer.rs` reads a few driver `import`s, and
RFC 0206's JVM recovery is bytecode in the private build.

### What was built

| Component | What it does |
|---|---|
| `plugins/java` (`JavaObserver`) | one artifact per `.java`, verbatim; ISO-8859-1 sources kept via lossy decode |
| `recovery/src/java_syntax.rs` | tokenizer (comments, strings, chars, text blocks) + bracket matching + declaration parser: package, imports, types (nested, enums, records, annotation types), methods/constructors with spans and signatures, fields with initialiser ranges, Javadoc |
| `recovery/src/app_model.rs` | iDempiere-family recognizer → `ModelTable` (columns, business names, list values, mandatory/default, updatable, references, document lifecycle) |
| `recovery/src/java_sql.rs` | per-method SQL folding (`String`/`StringBuilder`/`append`/`+=`, constants, `addAccessSQL`, `toString()`), `DB.*` call sites and `new Query(...)`, parsed with `statement_footprint_in` |
| `recovery/src/java_analyzer.rs` | `JavaAnalyzerPass`: `JavaType`, `JavaMethod`, `ModelTable`; `Contains`, `Extends`, `Implements`, `DependsOn` (imports), `Models`, `References` (code FK), `ReadsFrom`/`WritesTo` (embedded SQL); `tickets` |
| integration | registry rows (keyed), identity narrowing (`java` and `appmodel` namespaces + type/constructor group), `doc_links`, `llm_description`, docs-gen lists and grouping, `build.rs`, `recover.rs` |

### Measured on iDempiere `release-13` @ `3355eb5fc9`

| What | Count |
|---|---|
| Java files parsed / partial / skipped | 4,542 / 0 / 0 |
| `JavaType` (generated) | 4,820 (1,526) |
| `JavaMethod` | 31,434 |
| `ModelTable` / columns | 763 / 16,660 |
| columns with business name / description / java type | 16,660 / 16,658 / 16,146 |
| boolean (Y/N) / mandatory / with default / not updatable / list-valued | 2,170 / 4,539 / 1,073 / 2,198 / 596 |
| code-declared references (all resolve to a modelled table) | 2,882 |
| business classes linked / document lifecycles | 515 tables / 26 |
| embedded SQL statements: parsed / partial / unparsed | 3,207: 1,992 / 996 / 219 |
| method → `ModelTable` links from SQL | 2,909 |
| declarations with ticket anchors | 372 |
| model calls | 0 |

### Implementation details worth remembering
- **Generated files get no `JavaMethod` objects.** 1,526 `X_`/`I_` types would otherwise add
  ~65,000 getter/setter objects that restate the `ModelTable` columns. Their constants still feed
  the SQL constant lookup in memory.
- **Edges only to types in the tree.** Supertypes and imports are resolved Java's way (enclosing
  types, single-type import, same package, on-demand import) against what this run parsed; the rest
  are listed as `external_supertypes` / `external_imports`, never fabricated as objects.
- **Constant lookup walks the supertype graph.** `Table_Name` inside `MInvoice` resolves through
  `MInvoice → X_C_Invoice → I_C_Invoice`; `I_C_InvoiceLine.Table_Name` through the qualifier.
- **The new-record block is a comment.** Mandatory columns and defaults live in the `/** if (… == 0)
  { setX(…); // default } */` block the generator leaves commented out in the first constructor.
- `set_ValueNoCheck` in a generated setter is the dictionary's "not updatable".
- Kinds are built from constants, so the registry's source-scanning guard cannot see them;
  `every_java_kind_has_a_keyed_registry_row` checks them directly.

### Decisions
- **Hand-written parser, not `tree-sitter-java`** (C toolchain). Java is lexically unambiguous, so
  this is exact at declaration level — unlike Perl.
- **`ModelTable` is never merged into `Table`.** Code's model and the catalog are two sources;
  RFC 0211 compares them into `ConflictingEvidence`. Identity treats them as separate namespaces.
- **No `Calls` graph** — receiver types of locals are unknown without type inference.
- Embedded SQL uses the generic dialect for now; RFC 0210's Oracle preprocess plugs in the same
  seam, and the parse status of every statement is kept so the gain is measurable.

---

## Knowledge Captured
- A placeholder for a non-literal fragment (`ekos_expr`) parses as a value or column in most
  positions; most of the 219 unparsed statements are placeholders standing in for a **whole
  clause** (`" WHERE " + whereClause`). They are kept with the parser's error.
- `resolve` on the iDempiere tree reports one conflict, `dual` as View and Table, from Oracle
  migration scripts — not from the Java kinds. RFC 0210 territory.
- iDempiere's `migration/` scripts begin with `SET SQLBLANKLINES ON SET DEFINE OFF` (SQL*Plus), which
  `sqlparser` rejects; the per-statement fallback (RFC 0176) skips it as `SQL005`.
- Whole-tree run (scratch `EKOS_HOME`, debug build, no LLM configured): `build` 2m35s, `recover`
  5m35s, `compile` 1m21s (236,640 objects, 97,239 relationships), `commit` **58m21s**. The ledger
  then answers EKL with exactly the analyzer's counts (763 `ModelTable`, 31,434 `JavaMethod`), but
  each EKL query takes minutes on a debug build. Use a release build for full-tree runs.
- That commit also reported 2,372 open `ConflictingEvidence` items. They come from the 5,668 SQL
  migration scripts restating the same tables (RFC 0172 duplicate definitions), not from Java.
  Schema history (RFC 0210) has to read those scripts as dated change sets, not as competing
  definitions.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0209-java-analyzer-application-model.md` | new RFC |
| `ekos/plugins/java/` | new observer plugin |
| `ekos/crates/recovery/src/java_syntax.rs` | tokenizer + declaration parser |
| `ekos/crates/recovery/src/app_model.rs` | application model recognizer |
| `ekos/crates/recovery/src/java_sql.rs` | embedded SQL harvester |
| `ekos/crates/recovery/src/java_analyzer.rs` | `JavaAnalyzerPass` |
| `ekos/crates/recovery/src/lib.rs`, `Cargo.toml` | modules, re-exports, `walkdir` dev-dep |
| `ekos/crates/kir/src/custom_kinds.rs` | `JavaType`, `JavaMethod`, `ModelTable` |
| `ekos/crates/identity/src/lib.rs` | Java declaration group, `java`/`appmodel` namespaces, tests |
| `ekos/crates/semantic/src/doc_links.rs`, `recovery/src/llm_description.rs`, `docs-gen/src/lib.rs` | kind lists |
| `ekos/crates/cli/src/commands/{build,recover}.rs`, `cli/Cargo.toml`, `ekos/Cargo.toml` | wiring + summary lines |

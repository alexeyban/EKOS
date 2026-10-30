# RFC 0233 — Cross-reference integrity check

**Status:** Accepted — Phases 0–4 implemented 2026-10-30 (see §11 for what each phase can and cannot decide)
**Date:** 2026-10-30
**Author:** @alexeyban
**Repos:** EKOS (`semantic`, `compiler-core`, `cli`, `docs-gen`, `recovery`, console `web/`); ekos-lakehouse gets the report section through EKOS's estate docs
**Depends on:** RFC 0135 Parts B/C (provenance, deterministic relationship ids — the "RFC 0136/0137" of the
source draft, landed 2026-09-04), RFC 0163/0169 (routine and view footprints), RFC 0075 (transform-node
lineage), RFC 0209 §7 (SQL embedded in Java), RFC 0211 (live catalog objects)

Source draft: "RFC: Cross-Reference Integrity Check" (2026-10-30). This RFC keeps its goals, diagnostic model,
check catalogue and phases, and fixes the parts that have to meet the code as it is.

## 1. Problem

EKOS already resolves names across systems — routine and view footprints to tables (RFC 0163/0169),
transformation steps to tables (RFC 0075), Java SQL to application-model tables (RFC 0209) — and when a name
resolves to nothing, or to several objects, **the name is dropped in silence**. `procedure_lineage` counts the
ambiguous ones in a commit summary line and throws the rest away; `java_analyzer::link_sql` keeps an unresolved
table only inside the method's `sql` property; dbt's analyzer logs a warning and moves on.

Those dropped names are exactly what no single-language linter can see: a Java method whose SQL reads a table no
schema defines, a Kettle step writing a table that was renamed, a view reading a relation from another database.
This RFC turns them into diagnostics with evidence, a confidence and a stable fingerprint, so a person can act
on them and CI can fail on new ones.

## 2. Goals and non-goals

Goals, non-goals and the six diagnostic types are the source draft's, unchanged: high-precision
`UnresolvedReference` / `AmbiguousReference` first, reachability later and always published next to its dynamic
share; deterministic, over the compiled ledger; an LLM may only ever *suggest* in triage. EKOS never edits code.

## 3. Diagnostic model

`ekos_semantic::xref` (pure: objects in, report out — no ledger handle, no I/O):

| Field | Meaning |
|---|---|
| `kind` | `UnresolvedReference`, `AmbiguousReference` (Phase 1); `UnreachableSymbol`, `DynamicReferenceUnknown`, `ReferenceCycle`, `OrphanArtifact` (later phases) |
| `severity` | `error` / `warning` / `info` |
| `confidence` | 0–1 |
| `source` | the referring object (id, kind, name), `file`, `line`, `extractor`, `access` (`read`/`write`) |
| `target_spec` | the name as written, its normalized form, the expected kind (`relation`), the namespace (schema qualifier) |
| `candidates` | for an ambiguous name every match; for an unresolved one the closest names (a renamed column or table) |
| `evidence` | the referring object's evidence ids |
| `coverage_gap` | `true` when the target points at a namespace EKOS did not compile (§5) |
| `cross_system` | `true` when the reference leaves its own system (code → schema, ETL → schema, dbt → schema) |
| `fingerprint` | SHA-256 over (kind, source kind, source file, source name, access, normalized target) — **never ids or line numbers**, so a recompile or an edit above the reference does not move it |

### Where findings live

Not in the ledger. The ledger is append-only and findings are derived and change every commit; writing them
would grow the ledger by every finding of every run forever. `ekos commit` recomputes the report and writes the
**integrity index** `<state>/integrity/current.json`: the report, its metrics and the revision it was computed
at (`computed_at`, object count). `ekos check refs` recomputes from the ledger (read-only, cheap: one pass over
the objects) and does not depend on the index; the index is what the console and `ekos status` read.

### Configuration and suppression

Deviation from the draft's `.ekos/xref.toml`: every other EKOS setting lives in `ekos.toml`, is validated by
`ekos config validate` and documented by `ekos config schema`, so the project defaults are an `[xref]` section:

```toml
[xref]
enabled = true            # compute the integrity index at commit (default on; no LLM, linear in objects)
min-confidence = 0.5      # findings below are not shown or failed on; --min-confidence overrides per run
ignore-file = "xref-ignore.toml"   # relative to the state directory (default)
[xref.severity]           # per-kind override
AmbiguousReference = "info"
```

Suppressions stay in a separate file, as the draft says, because they are reviewed like code: by default
`<state>/xref-ignore.toml` (`.ekos/xref-ignore.toml` in a plain workspace — the draft's path):

```toml
[[ignore]]
fingerprint = "3f1c…"           # or: target = "tmp_*" (glob over the normalized target), source = "sql/old/*"
reason = "temporary table created in the same routine"   # mandatory; a rule without one is an error
```

A suppressed finding is counted (`suppressed`) and listed with `--show-suppressed`, never silently gone.

## 4. Reference sources (Phase 1)

| Source objects | Property | Extractor | Cross-system |
|---|---|---|---|
| `Procedure`, `View` (RFC 0163/0169) | `reads`, `writes` | `procedure-footprint` / `view-footprint` | no (SQL → SQL) |
| `ProcedureStatement` | `reads`, `writes` | `statement-footprint` | no |
| `TransformNode` Source/Sink (RFC 0027/0075) | `object_name` | `pentaho` (`.ktr`/`.kjb`) or `sql-transform` | ETL → schema: yes; SQL → SQL: no |
| `JavaMethod` (RFC 0209 §7) | `sql[].reads/inserts/updates/deletes` | `java-sql` | yes |
| dbt `source()` table (RFC 0117, `dbt_kind: source`) | its name, against relations **outside** dbt | `dbt-source` | yes |

A `ProcedureStatement` repeats names its `Procedure` already lists; to avoid reporting one missing table twice,
statements are the reference source **only for line numbers** — the diagnostic's source is the routine, its
`line` the first statement that names the target.

dbt itself already fails on a `ref()` to a missing model, so the dbt check that adds something is the other
direction: a declared source table that no compiled schema (DDL, live catalog, application model) defines. It needs
no analyzer change.

Targets: every `Table`, `Dataset`, `View` and `ModelTable`, file-derived or live (`origin: live`, RFC 0211).
Resolution is `procedure_lineage::NameIndex`'s rule (exact lower-cased name, else qualified ↔ unqualified, file
objects before live ones), so a name the check calls resolved is exactly a name lineage linked.

`Calls` (routine → routine) are **not** in Phase 1: the footprint records built-ins (`coalesce`, `now`,
`nextval`) as calls and EKOS has no per-dialect built-in catalogue yet, so an unresolved call cannot be told from
a built-in with the precision Phase 1 promises. It joins Phase 2 with the in-language checks. Config ↔ code is
also moved to Phase 2: no extractor records environment-variable or config-key reads today.

## 5. Precision rules

| Situation | Result |
|---|---|
| Name resolves to one target | resolved, counted in the metrics |
| Name resolves to several | `AmbiguousReference`, warning, 0.6, every candidate listed |
| Unresolved, and the source's system has compiled relations in the target namespace | `UnresolvedReference`, error, 0.9 |
| Unresolved, from SQL text that is `partial` (Java placeholders) or a routine with dynamic SQL sites | warning, 0.6 |
| Unresolved, schema qualifier no compiled relation has, or no relations compiled at all | info, 0.3, `coverage_gap: true` |
| System catalogs (`pg_catalog.*`, `information_schema.*`, `pg_*`, Oracle `dual`, `sys.*`, `all_*`/`dba_*`/`user_*`/`v$*`, ClickHouse `system.*`) | not a reference |
| A name containing the Java placeholder `ekos_expr`, or a template marker (`[%`, `{{`, `${`) | not resolvable, counted under `dynamic` |
| A relation some routine statement creates — `CREATE [TEMP] TABLE` or `VIEW`, also inside an `EXECUTE` / `EXECUTE IMMEDIATE` string (statement `sql` / `expr`) | resolved: it exists at run time |
| A SQL transformation's `FROM f()` that names a compiled routine (overloads included), or a set-returning built-in (`generate_series`, `unnest`, `json_each`, …) | resolved / not a reference |
| An ambiguous name of which exactly one definition is in the reference's own file | resolved to it (a module that re-creates a migration's view) |
| One relation defined several times — every candidate has the same full name and kind (an engine's twin script `db/oracle/views` / `db/postgresql/views`, a migration re-creating a view) | resolved: not two relations |
| A name only a `ModelTable` (RFC 0209, the application model's generated class) claims | resolved; a `ModelTable` is never a second definition beside DDL |
| The reference is in a migration script (a file with RFC 0210 `SchemaChange`s) | info, 0.3: a script describes the schema of its own time |
| The latest table-level migration change dropped the name | **error**, 0.9, "dropped by `<script>:<line>`" |
| The migration scripts change the table, but no compiled DDL or model creates it | info, 0.3, `coverage_gap` — the table exists; its definition is not compiled |
| Test code (`test`/`tests` path segment, `*Test.java`, `test_*.py`, `*_test.go`) | info, 0.3: a missing table may be what the test checks |
| An unresolved name shaped like a work table (`tmp_*`, `temp_*`, `*_tmp`, `pg_temp.*`, `#t`) | info, 0.3 |

Names are compared lower-cased with quotes stripped; Oracle's and PostgreSQL's case folding both reduce to that
for unquoted identifiers, and EKOS's analyzers already lower-case quoted ones on the way in.

### Measured (Phase 1, 2026-10-30)

LedgerSMB (1,366 references): the first run of the rules above without the three "resolved" rows gave 17 errors,
15 of them false — 5 temporary or `CREATE TABLE AS` tables made inside routines, 1 view built by `EXECUTE`, 5 table
functions in SQL transformations, 4 ambiguous names that a same-file definition settles. With them: **2 errors, both
real** — `budget__reject` deletes from `budget_to_project` and `budget_to_department`, which no file in the
repository creates (the schema has `budget_to_business_unit`), so the function fails when it runs. Two further
false errors (`holidays`, `overpayment`) came only from a ledger compiled before RFC 0176's per-statement DDL
fallback and vanish on a current compile — the check reports what the ledger knows, so a stale ledger shows up as
false findings.

iDempiere (clone of `master` bd4f848, code and scripts only — **no live catalog**; 11,966 references, 3,126 of them
cross-system from Java): the first run gave 597 errors and 1,349 warnings. The warnings were 1,090 DDL-table +
`ModelTable` pairs and 256 views defined 2–13 times (Oracle and PostgreSQL twins, migrations); the errors were 573
references to `*_trl` translation tables and a few others (`W_Click`, `W_Store`) that iDempiere's base schema holds —
it ships as a database dump, not DDL, and the generated classes do not cover them — plus Oracle dictionary views
missing from the list and 8 in test code checking a deliberately missing table. With the redefinition, model-claim,
migration-history and test rules: **0 errors, 0 warnings, 609 info** (336 references from migration scripts, 265
coverage gaps, 8 test code), each checked by hand. Cross-system resolution: 97.7 %. Precision at error level on
iDempiere is therefore not measured yet: the Phase 1 criterion needs the testbed run with the live Oracle catalog,
where a missing table is decidable.

## 6. Metrics

Per report: `references`, `resolved`, `unresolved`, `ambiguous`, `dynamic`, `resolution_rate`,
`cross_system_references`, `cross_system_resolution_rate`, and per extractor the same three numbers. Phase 3
adds reachability coverage, dynamic share, orphan and cycle counts.

## 7. Interfaces

- `ekos check refs [--scope cross-system|code|all] [--min-confidence F] [--format table|json|sarif]
  [--fail-on error|warning] [--baseline <RFC 3339 time | report.json>] [--show-suppressed]`.
  Catalogue class `read`. Exit status 1 when a shown finding is at or above `--fail-on`.
- **Baseline**: a timestamp recomputes the check over `all_objects_at(ts)` (the ledger's own time travel, the
  `ekos diff` mechanism) and keeps only findings whose fingerprint is new; a path reads a JSON report a previous
  run wrote (`--format json`), for CI that rebuilds the ledger from scratch every run.
- **SARIF 2.1.0**: one rule per kind, `level` from severity, the location the reference's own file and line,
  `partialFingerprints.ekosXref/v1` the fingerprint, `properties` carrying confidence, target, candidates.
- `ekos_xref_check` (MCP, Phase 3), the console Integrity tab and the lakehouse report section (Phase 4) read the
  same report type.

## 8. Phases

0. **Prerequisites** — RFC 0135 Parts B/C are landed (they are the draft's "RFC 0136/0137"); fingerprints and
   name normalization; `[xref]` and `xref-ignore.toml`; the integrity index at commit. Criterion: two reports
   over the same ledger are byte-identical, and a recompile of an unchanged workspace yields the same
   fingerprints (test with input order reversed).
1. **Broken cross-system references** — §4's sources, `UnresolvedReference` / `AmbiguousReference`, CLI, JSON,
   SARIF, baseline. Criterion: ≥ 95 % precision on a manual sample of error-level findings on LedgerSMB and the
   iDempiere testbed (recorded in the devlog with the sample).
2. **In-language references** — unresolved imports and calls (with per-dialect built-in lists), config ↔ code,
   `ReferenceCycle`, `OrphanArtifact`; the analyzer contract (`entry_points`, `dynamic_patterns`) as a trait
   every recovery pass that emits symbols implements; a language without it is marked unsupported for
   reachability.
3. **Reachability** — `UnreachableSymbol`, `DynamicReferenceUnknown`, the metrics, `ekos_xref_check`.
4. **Console, lakehouse, triage** — Integrity tab, dashboard metrics, lakehouse docs section, opt-in `--explain`.

## 9. Decisions

The draft's decisions table stands (start now; separate index recomputed on commit; per-project
`min-confidence`; opt-in LLM triage, suggestions only; Python as the second demo; no README badge). Added here:

| Question | Decision | Why |
|---|---|---|
| Config file | `[xref]` in `ekos.toml`, suppressions in `xref-ignore.toml` | one validated, schema-documented config; suppressions reviewed separately |
| Findings in the ledger? | No — the integrity index file | derived, recomputed each commit; the ledger would grow by every finding forever |
| Statement vs routine as source | the routine, the statement only for the line | one missing table, one finding |
| `calls` in Phase 1 | No | built-ins look like unresolved calls until there is a built-in catalogue |
| Baseline by revision | ledger time travel (`all_objects_at`) or a saved report | no history to store; works for a CI that rebuilds |

## 10. Risks

As in the draft. The one this design adds: the ledger is append-only, so a routine removed from the sources still
has its last version in the ledger and its references are still checked (the same is true of lineage today).
Phase 1 restricts sources to objects whose `source_path` still exists in the committed source manifest when one is
present (`<state>/committed-manifest.json`, RFC 0171).

## 11. Phases 2–4 as built (2026-10-30)

### Phase 2 — in-language references

| Check | Module | What it decides | What it deliberately does not |
|---|---|---|---|
| SQL routine calls | `xref.rs` (`routine-calls`), `xref_builtins.rs` | a call no compiled routine, run-time `CREATE FUNCTION` or built-in has: error 0.8; Oracle: warning (object types / synonyms are not compiled) | overloads are one routine, never ambiguous |
| Perl `use` / Elixir `alias` | `xref_imports.rs` | a package under a namespace this workspace declares (`LedgerSMB::…`) that no file declares; *declared* = target of a `File`/module `Contains` edge (the object alone cannot say: declaration and use share one id) | CPAN / Hex packages are counted as `external` |
| JS relative imports | `xref_imports.rs` | `./x` / `../x` with no observed file under any script extension or `/index` | bare and alias specifiers (`vue`, `@/x`) — bundler config is not read; stylesheet/image imports |
| Python imports | `xref_imports.rs` | `a.b.c` under an in-tree top-level package where neither `a.b.c` nor `a.b` is importable (package chains from `__init__.py`): warning 0.6 — the analyzer drops a relative import's dots | third-party packages |
| Rust / Java imports | — | left to `cargo` / `javac` (non-goal) | |
| Excluded files | CLI `unobserved_files` | an import that lands in a file under an observed directory that `ignore-patterns` excluded is a coverage gap (info), naming the file; the walk reads names only, skips `.git`/`node_modules`/`target`/`.ekos`, stops at 300 000 entries | |
| `ReferenceCycle` | `xref_graph.rs` | one finding per strongly connected component (iterative Tarjan) of `DependsOn` among `Crate`/`PerlPackage`/`ElixirModule`/`JavaType`; warning 0.9; fingerprint over the sorted member names | routine recursion (legitimate) |
| `OrphanArtifact` | `xref_graph.rs` | a file-defined `Table`/`View` no non-structural edge touches, no reference names, no `ModelTable` claims; info 0.5 (0.3 in a migration script) | live catalog / dbt / system relations; Perl and Python SQL are not extracted, so "unused" means "no compiled use" |

The analyzer contract is `xref_reach::ReachabilityRules` (`family`, `symbol_kinds`, `support`, `entry_point`,
`dynamic`, `excluded`). A plain trait over the compiled objects rather than a method every recovery pass implements:
the facts a rule needs (visibility, annotations, the call graph) are already properties and edges in the ledger, and
the rules then live next to the engine that uses them. `every_registered_symbol_kind_is_claimed_by_a_family` fails
when a `custom_kinds::REGISTRY` row ending in `Symbol`/`Method` (or `Procedure`) has no family — it caught
`BinaryMethod` on its first run.

### Phase 3 — reachability, metrics, MCP

Only **SQL routines** are supported: roots are views and other non-symbol callers, trigger functions (`Calls` from a
`Trigger`, or its `function` property when no edge exists), routines Java SQL text names, and `[xref] entry-points`
globs; migration-script routines are not symbols but what they call is reached. An unreached routine is
`DynamicReferenceUnknown` when Java builds SQL at run time, or when Perl / Python / JS / Elixir / Rust code exists
(it may call routines by name in text EKOS does not read SQL from); `UnreachableSymbol` otherwise. Every other
family is declared unsupported with its reason: no cross-file call graph (Rust: same file only; Python, JS, Java,
Perl, Elixir: none), and entry points are not recorded (`main`, `pub`, `[[bin]]`, `package.json` `main`, `@Test`
only as annotations). Making them supported is analyzer work, listed in TODO.

`ekos check refs` hides reachability findings unless `--reachability` (a dynamic estate yields one per routine); the
metrics always show coverage beside the dynamic share. `ekos_xref_check` (MCP, read-only, in `SCOPE_REFUSED_TOOLS`:
it reads the index beside the ledger) serves the commit-time index, filtered, and with `name` resolves one
table/routine name now — resolved / ambiguous / unresolved / built_in — for an agent about to write SQL.

### Phase 4 — console, docs, triage

- Console **Integrity** tab (`routes/integrity.py`, `pages/Integrity.tsx`): runs one fixed argv,
  `check refs --from-index --format json --reachability` (the index, never a recompute), filters in the page
  (kind, severity, confidence, cross-system, coverage gaps hidden by default), opens the referring object's evidence
  panel and links to it in the graph. Dashboard tiles from `?summary=true`.
- Estate docs (RFC 0214) Findings page: a *Reference integrity* section — the numbers, reachability per language,
  errors and warnings except those about out-of-scope tables. ekos-lakehouse's `lakehouse docs generate` calls EKOS's
  generator, so its documentation gets the section with its next EKOS update; nothing changed in that repository.
- `ekos check explain` (catalogue class `external`, metered): for `AmbiguousReference` and
  `DynamicReferenceUnknown`, the model is shown the candidates or the objects whose indexed text names the routine,
  and its answer is kept only when it names a shown object and cites shown ids (`xref_triage::judge`). Printed,
  never stored; a finding's kind and confidence never change. A non-Ollama model needs `--allow-metered`.
  `Task::Triage` (`[llm.tasks.xref-triage]`).

### Measured

| | LedgerSMB (fresh compile) | iDempiere `master` bd4f848, no database |
|---|---|---|
| References checked | 2,018 — 98.0 % resolved | 12,836 — 95.2 % (Java → schema 97.8 %) |
| Errors | **4, all real**: `budget_to_project`, `budget_to_department` (no file creates them), `lsmb__next_businss_day` (a typo of `lsmb__next_business_day`), `lsmb__daycount_act_365` (only `…_365fixed` exists) | 0 |
| Warnings | 1 cycle (`LedgerSMB::Company::Role(s)/User(s)`) | 26 cycles (one of 279 Java types), 1 Oracle type constructor |
| Perl imports | 401 resolved; 35 into `old/` (excluded by `ignore-patterns`) → coverage gaps | — |
| Orphans | 28 tables (most in migration scripts) | 46 tables (all in `migration-historic/`) |
| SQL reachability | 517 routines: 22 reachable, **95.7 % dynamic** (Perl calls by name) — no dead-code claim | 130: 85 reachable, 34.6 % dynamic (Java SQL is built at run time) |
| Time / memory | 2.3 s, 167 MB | 20.5 s, 1.7 GB (mostly loading the ledger) |
| `check explain` (llama3, local) | 4 findings: 2 answers dropped as invented, 2 kept as labelled suggestions | — |

The ≤ 10 % false `UnreachableSymbol` criterion is met vacuously on both: every unreached routine is dynamic. The
check claims dead code only where no dynamic caller exists, which on these estates is nowhere.

# RFC 0200 — Pentaho packages to lakehouse dbt (silver and gold)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0027/0036 (Transformation IR, Pentaho → dbt export), RFC 0149/0191/0198
(extension seam, commands, console pages), RFC 0190–0196 (Lakehouse), RFC 0197 (LLM task routing),
RFC 0148/0150 (binary recovery, private)
**Code:** public EKOS (seams in this RFC); `ekos-lakehouse` (everything else); private
`ekos-binary` (compiled Java only, RFC 0206)

## Goal

A team leaving Pentaho Data Integration points EKOS Lakehouse at its Kettle package — `.ktr`,
`.kjb`, repository exports, `shared.xml`, `kettle.properties`, plugin jars — and gets dbt-trino
models in its lakehouse pipeline: silver models for the cleansing part of each transformation,
gold models for the business part, each step labelled with how faithfully it was carried over,
and every step EKOS could not carry over shown with its original code and the reason. A person
merges the proposal; EKOS never does.

## Why not extend the core Pentaho path

The core path (`plugins/pentaho`, `recovery/src/pentaho_analyzer.rs`, `crates/dbt-gen`) is a
knowledge path: it records what a job does in the string-only Transformation IR so `ekos ask`
and `ekos_transformation_explain` can answer about it. It maps 7 step types; a real repository was
49% `Unmapped` (devlog_34). Generating runnable SQL needs a typed expression AST, the full step
catalogue, variables, connections and sub-transformation links — a different IR. RFC 0027 left the
typed AST out on purpose. The deep model therefore lives in the lakehouse extension, with its own
IR; the core path stays as it is and keeps serving explanations. Both read the same files.

## Design

### 1. Pipeline

```
Kettle package files
  │ observer `pentaho-package` (lakehouse) — passwords dropped, RFC 0043 redaction
  ▼
kettle        package → jobs → transformations → steps, hops, variables, connections, links
kettle-expr   typed Expr + parsers (conditions, Calculator, Formula, Janino, Java subset, JS subset)
kettle-rel    typed relational IR per transformation, every node with step id + fidelity
pentaho-dbt   medallion split → Trino SQL models, yml tests, translation report
  │ gates: anti-invention (migrate-logic) + Trino EXPLAIN (+ optional differential run)
  ▼
branch ekos/pentaho/<package>/<id> with proposals/pentaho-<package>.md — merging it is approval
```

### 2. Fidelity

Every step, and every model built from steps, carries one label:

| Label | Meaning |
|---|---|
| `exact` | the step has a direct SQL equivalent with the same semantics |
| `rewritten` | a deterministic rewrite whose semantics are argued in the translator's rule (e.g. Kettle's empty-string-is-null made explicit) |
| `llm-drafted` | a model drafted it and every gate passed; a person must read it |
| `gap` | not translated; the original code is kept with the reason, and the model carries a failing test so `dbt build` stops instead of passing rows through |

A model's label is the weakest label of its steps.

### 3. Medallion split (deterministic)

For each input branch of a transformation, the row-local chain — filter, select/rename/retype,
calculate, value mapping, string operations, null handling, dedupe — up to the first step that
combines rows (join, lookup, append, group, window, sort-dependent step) becomes a **silver** model
in schema `silver_pentaho`. From that step to the outputs it becomes **gold**, one model per target
table. A table written by one transformation and read by another becomes a `ref()`, not a physical
table. Table input tables resolve through the lakehouse lock file to the bronze/silver models
of the matching source; an unresolved connection or table becomes a question, and no SQL is emitted
for that branch.

### 4. Code steps and compiled Java

Modified Java Script Value, User Defined Java Class and User Defined Java Expression hold source
text inside the `.ktr`; the lakehouse parses it (JavaScript with `oxc_parser`, Java with a
hand-written subset parser) and lowers the row-local subset. Compiled plugin steps are bytecode.
Decompiling bytecode is the private `ekos-binary` product (RFC 0149), so the public seam below
lets a binary that includes it supply a decompiler; the public build reports such steps as `gap:
compiled plugin`. A decompiler returns Java-like source text, which goes through the same Java
subset parser as UDJC — one lowering path, and its fidelity is capped at `rewritten`.

### 5. LLM fill-in

Only for `gap` steps, only with `--allow-metered` (and the console's `allow_metered`), through the
RFC 0197 router under the new task `translate`. The answer passes the anti-invention check (its
atoms must come from the step and its inputs) and Trino EXPLAIN, with up to three attempts that
feed the checker's findings back (RFC 0195's loop). What survives is `llm-drafted`.

### 6. Seams in public EKOS (this RFC's code)

All additive:

- `ekos_kir::custom_kinds::REGISTRY`: `PentahoPackage`, `PentahoJob`, `PentahoTransformation`,
  `PentahoStep`, `PentahoJobEntry`, `PentahoConnection`, `PentahoTranslation`, all structurally
  keyed (step names repeat across transformations; connection names repeat across packages).
- `ekos_recovery::Task::Translate` (`[llm.tasks.translate]`).
- `ekos::extension::JavaDecompiler` with `DecompiledClass` / `DecompiledMethod` /
  `DecompiledFidelity`, `EkosExtension::java_decompiler()` (default `None`),
  `Extensions::java_decompiler()` (the first one offered), and `extensions` on `CommandContext` and
  `RecoverContext` so an extension's command or pass can ask for it.

### 7. Web console

A **Pentaho** tab (RFC 0207) with packages, the job graph, per-step fidelity, original code next to
the generated SQL, questions and the proposal; actions to scan and translate. It needs drill-down
and a per-package argument, which RFC 0198 pages do not have; RFC 0207 adds them to the page seam.

## Milestones

| RFC | Milestone | Where |
|---|---|---|
| 0200 | this umbrella + seams | EKOS |
| 0201 | deep Kettle model, observer, variables, links, job graph | lakehouse |
| 0202 | typed expressions + relational IR, step catalogue | lakehouse |
| 0203 | JavaScript / Java code steps, compiled-plugin seam use | lakehouse |
| 0204 | medallion split, Trino SQL, dbt files, proposal, CLI, MCP | lakehouse |
| 0205 | LLM fill-in (OpenCode Zen via RFC 0197) | lakehouse |
| 0206 | JVM statement recovery behind `JavaDecompiler` | ekos-binary (private code, public doc) |
| 0207 | console Pentaho tab + page seam v3 | EKOS + lakehouse |

Verification throughout: golden `.ktr` fixtures per step type and parser, a coverage report on
the AdventureWorks corpus (`test-runs/run-20260831T212115Z/workspace/`) and Pentaho's own samples,
`dbt build` on the sandbox Trino, and an opt-in differential run of the original transformation
in PDI against the dbt result (migrate-validate V1–V3) with planted defects.

## Non-goals

- Changing the core Pentaho analyzer, the Transformation IR or `ekos dbt generate`.
- Running Pentaho in production, or translating file/FTP/mail job entries (orchestration only:
  reported, not translated).
- Sources other than the lakehouse's PostgreSQL sources in v1.
- Any decompiler code in a public repository.

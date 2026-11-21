# RFC 0262 — Talend projects: umbrella, kinds, redaction, seams

**Status:** Accepted (2026-11-20)
**Plan:** `talend-plugin-plan.md` (2026-11-19), Parts 0–VI
**Related:** RFC 0027 (Transformation IR), 0031 (SQL dialects), 0043 (redaction), 0149/0191/0198 (extension seam),
0200–0207 (Pentaho → dbt), 0209 (Java string folding), 0214 (estate docs)

## Problem

Talend Open Studio was retired on 31 January 2024. Estates built with it are being moved, usually to dbt, and the
first question is always "what do these jobs actually do?". EKOS reads Pentaho jobs (RFC 0027) and translates them
to dbt (RFC 0200–0205). It does not read Talend at all: a Talend project in a workspace today yields `File` objects
and nothing else.

## Scope and split

Two paths, the split RFC 0200 drew for Pentaho:

| Path | What | Where | RFCs |
|---|---|---|---|
| **Knowledge** | read a project, put jobs, components, links, contexts, connections, tables and column lineage in the ledger; documentation; the migration assessment | public EKOS | 0262 (this), 0263 model, 0264 ledger + lineage, 0265 docs + assessment |
| **Translation** | typed expressions, a relational IR shared with Kettle, dbt output, differential run, console tab | extension | 0266 shared IR, 0267 expressions + semantics, 0268 dbt + console |

Numbers 0263–0268 are reserved by this RFC; each is written just before the slice that needs it.

Slices:
1. **Metadata and lineage**: Part 0 census, RFC 0263 reader with secret handling, RFC 0264 ledger, an eval set.
2. **Documents**: RFC 0265.
3. **Translation**: RFC 0266–0268.

## Principles (binding on every Talend RFC)

1. **Deterministic.** Slices 1 and 2 make no model calls.
2. **Every fact has evidence:** file, component unique name, and the item's `xmi:id` where one exists. What was not
   understood is counted next to what was.
3. **Defaults are defaults.** A context value, a "Latest" job version, a lookup that is unique today: each is
   recorded with how it was resolved, and a value that can change at run time says so.
4. **No coverage number without a real fixture.** A component type is "supported" only when a real project
   containing it reads correctly. Shapes written from documentation are marked as such.

## Fixtures (Part 0)

Third-party projects are **fetched by commit hash, never vendored** (neither example repository has been cleared
for redistribution; one has no licence file). `scripts/talend-fixtures.sh` clones them into a directory given on
its command line; `scripts/talend-census.py` counts what each contains; the committed census
(`evals/talend/census.json`) holds counts only and is the denominator for every coverage figure.

| Fixture | Repository @ commit | Role |
|---|---|---|
| A | `AymaneSab/Data-Integration-Project-with-Talend` @ `aed50909` | documents only: must yield 0 jobs |
| B | `BelkacemYerfa/talend-project-big-data` @ `3d46b6f2` | a real Talend Open Studio 8.0.1 project: 14 jobs, 272 components of 24 types, 232 links |
| C | `KompallyAkhil/Talend2` @ `8e8887c0` | joblets as Studio writes them (9; a data joblet used twice, once as a lookup; trigger joblets chained), component-framework (Snowflake) components, `tJavaRow`, `tJoin`, 3 projects, 30 jobs |
| D | `singhSurajsehwag/Talend_SQL` @ `3f98d699` | a two-output joblet used through its second port; 3 projects, 26 jobs |
| E | `magrcrsdg/POC_DataCloudTalend` @ `facebc7c` | ELT components (`tELTInput`/`tELTMap`/`tELTOutput`, 75/21/41) and several engines in one project (Snowflake, Oracle, PostgreSQL, a file input); Talend Cloud 8.0.1; 66 jobs, 458 components |

C and D were chosen from the seven public repositories holding joblet items (GitHub code search for
`model:JobletProcess`, 2026-11-21) as the ones whose jobs use them; none has a licence file, so they are fetched
like A and B. E is the only public project found with generic ELT maps (code search for `tELTMap`,
`tELTPostgresqlMap`, `tELTMysqlMap`, `tELTOracleMap`, `tELTMSSqlMap`, 2026-11-21); it has no licence file, and its
items name a real database account (URL, user), so nothing from it is quoted in public material beyond counts and
table or column names.

Tests that need a fixture read its path from `EKOS_TALEND_FIXTURE_B` (`_C`, `_D`, `_E`) and skip, saying so, when it is unset.
Everything else is tested on small synthetic items written for the test, each marked as such.

## Ledger kinds

Structurally keyed rows in `custom_kinds::REGISTRY` (identity never merges them; component names such as `tMap_1`
repeat in every job): `TalendProject`, `TalendJob` (one per version), `TalendComponent`, `TalendContext`,
`TalendContextVariable`, `TalendDbConnection`, `TalendSchema`, `TalendRoutine`, `TalendJoblet`. Links and
triggers are relationships, not objects (RFC 0264). `TalendTranslation` is added by RFC 0268.

## Secrets

Talend stores secrets in four shapes, all seen in fixture B:

| Shape | Example |
|---|---|
| encrypted value | `enc:system.encryption.key.v1:<base64>` (context parameters, `PASS` element parameters) |
| password element parameter | `<elementParameter field="PASSWORD" name="PASS" value="…">` |
| password context parameter | `<contextParameter … type="id_Password" value="…">` |
| password attribute | `password="…"` in `talend.project`, `Password="…"` in connection items |

The first three get built-in RFC 0043 patterns (`talend-encrypted-value`, `talend-password-parameter`). The
attribute shape is a fourth pattern (`xml-password-attribute`) that, like `generic-assigned-secret`, leaves a
dotted reference (`context.password`) untouched: that is a variable name, not a secret. Independently, the model
(RFC 0263) never reads a value from any of these places, whatever the redaction table says. A test asserts that
no artifact written from fixture B contains any of its encrypted strings.

## Seams

- **Observer** `talend` (`plugins/talend`): a project is a directory holding `talend.project`. The observer
  captures each item and properties file of the project as one artifact (raw text, the project root and the
  item's role), skipping generated and vendor content by default (`poms/`, `temp/`, `target/`, `*.screenshot`,
  system SQL patterns, `recycle_bin.index`) and counting what it skipped.
- **Model** `ekos-talend` (`crates/talend`): pure, `roxmltree` only; text in, `Project` out (RFC 0263).
- **Analyzer** `recovery/src/talend_analyzer.rs`: artifacts → model → KIR (RFC 0264).
- **Extension** (Slice 3): the translation path reads the same model through the RFC 0149 seam.

## Not in scope of this RFC

Big Data jobs (`process_mr/`, `process_storm/`), routes, dynamic schemas and `tXMLMap` are recognised, counted and
reported as out of scope for v1. Generated Java under `poms/` is never a source (it is a test oracle in Slice 3).

## Amendment (2026-11-21): generated code holds the key

Studio's generated `poms/` holds the project's encryption key (`code/routines/src/main/java/routines/system/
keys.properties`, `routine.encryption.key.v1=…`) and context files with every encrypted password in the
Java-properties form `enc\:routine.encryption.key.v1\:…`. D5's "skip `poms/`" therefore applies to every content
observer, not only the Talend one (`observation_sdk::is_generated_tool_output`: the `file` and `java` observers),
and redaction covers both forms wherever they appear (`talend-encrypted-value`, `talend-encryption-key`). Found by
the `talend-20` eval's adversarial question, which a local model answered with the stored value (devlog_345).

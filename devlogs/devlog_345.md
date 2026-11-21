# Devlog 345 — Talend Slice 1 follow-ups: Studio joblets, buffer/file lineage, zips, eval set, a secret leak

**Date:** 2026-11-21
**PRs:** none (pushed to `main` after local gates)
**Branch:** main

---

## Summary

The open Talend items from devlog_344 are done:
- The Java observer skips generated code. Fixture B's pipeline goes from about 10 minutes to 2, and `ekos resolve`
  no longer needs `--force`.
- `ekos check refs` resolves quoted DDL names, and its report is stable from run to run.
- Lineage crosses jobs through `tBufferOutput` and files.
- Items-export zips are read in place.
- Jobs list what they read and write.
- A 20-question eval set exists.

Fixtures C and D are two public projects with joblets that Talend Studio wrote. Checking against them found three
wrong joblet rules. The fixtures and the eval set also found **two secret leaks**:
- a plain-text password in a component-framework component's JSON settings;
- Talend's encryption key, and values encrypted with it, in Studio's generated `poms/`.

`ekos build` stored both. All are fixed and measured on the real projects.

---

## Generated Java under `poms/`

### Problem / motivation
Talend Studio writes the Java it generates for every job under `<project>/poms/`. The Java observer read it as
source, which had two effects:
- 6,534 Java symbols took about 8 minutes of `ekos commit`.
- There were 14 `JavaType` vs `TalendJob` same-name conflicts, so `ekos resolve` failed.

### What was built
`plugins/java` skips a `poms` directory whose parent holds `talend.project` (RFC 0262 D5).

`PIPELINE_LOGIC_VERSION` is now 9.

One conflict was left: `customCode` is a `TalendRoutine` in two projects, and also the `JavaType` of a loose copy
beside them. It is handled by a `talend` namespace in `identity::is_expected_cross_namespace_group`, since a
routine is a Java class. The namespace's kinds are kept in step with `talend_analyzer::KINDS` by hand: identity
cannot depend on recovery.

**Result:** fixture B runs build → commit in 118 s, and `resolve` exits 0 without `--force`.

---

## `ekos check refs`

### Problem / motivation
`NameIndex` lower-cased names and kept their quotes. A table defined as `"DIM_CLIENT"` therefore never matched a
Talend step naming `DIM_CLIENT`, and fixture B reported 53 errors where `data_lineage` linked every one of them.

### What was built
| Change | Where |
|---|---|
| `NameIndex` keys with `data_lineage::name_key` (each part unquoted, then lower-cased) | `semantic/src/procedure_lineage.rs` |
| Talend `TransformNode`s are extractor `talend` and cross-system; in-memory hand-overs, generated rows and file components are not checked | `semantic/src/xref.rs` |
| Declaring file = first in path order; an import's location = first of all its evidence; one finding per fingerprint = earliest in its file | `xref.rs`, `xref_imports.rs` |

### Measured
| Project | Before | After |
|---|---|---|
| Fixture B errors | 53 | 0 (65 of 65 Talend references resolved) |
| LedgerSMB findings (RFC 0233 asks for it) | 155 | 155 |

The LedgerSMB measurement found an older fault. The file named by 11 Perl-import findings changed from run to run,
because which `Contains` edge was read first varied. The file is part of the fingerprint, so baselines broke. Four
runs now give byte-identical reports.

---

## Lineage through buffers and files

### What was built
- **Positions.** A `Target` of kind File or Buffer records the column's `position`, and so does an
  `Origin::File`.
- **A new origin.** `Origin::Job {job, column, component, position}` is a `tRunJob` output column.
- **Cross-job links.** `link_jobs` now indexes three kinds of writer: tables, files and buffers. It matches:
  - a file origin to a file target by path, or by a `tFileList` mask through `glob::Pattern`;
  - a `Job` origin to that job's `tBufferOutput`.

  Both match by position, falling back to the name when a side has none.
- **New column rules.** `tJoin`: a `LOOKUP_COLS` column comes from the lookup and any other from the main row.
  The join appears as a condition: on every column for an inner join, on lookup columns only for a left join.
  `tSchemaComplianceCheck` and `tWarn` pass rows through.
- **The Talend pass is version `v3`.**

### Decisions
- **By position, not name.** A delimited file has no column names, and `tRunJob` copies the buffer's row array.
  Matching names would invent links between unrelated columns that happen to share a name.

---

## Items-export zips and job summaries

- **Zips.** The Talend observer opens a `.zip` that holds a `talend.project`. Zips under `poms/` and other
  generated directories are skipped. Each member becomes an artifact at `<archive>!/<member>`, with limits of
  256 MiB per archive and 32 MiB per member. `recover`'s `present_files` treats a member as present while its
  archive is. Fixture B's `jGenerateCalendar.zip` is a second project, `IVENTE_REPORTING`, giving 2 projects,
  15 jobs and 385 lineages.
- **Job summaries.** `TalendJob` now has `reads`, `writes`, `files_read` and `files_written`, taken from the
  expanded job. Its `description` is the author's text, kept as `author_description`, followed by "Reads tables
  … Writes tables … Runs jobs …". This made the eval's question on `jDwhProduit` answerable. The author wrote
  "Alimentation de la table DIM_CLIENT"; the job writes `DIM_PRODUIT`.

---

## Eval set (`evals/talend/`, `talend-20`)

The set has 10 structure questions, 7 lineage questions and 3 adversarial ones, each with the facts and the `.item`
a good answer cites.

**Evidence only (no model call):**
- 17 of 17 answerable questions have their facts in the evidence a model would be shown.
- Before the job summaries, it was 16 of 17.

To grade answers with a local model (no paid calls):
`[llm] provider = "ollama"`, `model = "llama3:latest"` in the workspace's `ekos.toml`, then
`ekos eval run --datasets-dir evals/talend --dataset talend-20`.

---

## Fixtures C and D: joblets as Studio writes them

### How they were found
A GitHub code search for `model:JobletProcess` found seven public repositories holding joblet items. Only four
have jobs that use them. Two were chosen:

| Fixture | Repository | Commit | What it covers |
|---|---|---|---|
| C | `KompallyAkhil/Talend2` | `8e8887c0` | A data joblet used twice, once as a lookup; chained trigger joblets; Snowflake components; `tJoin`; `tJavaRow` |
| D | `singhSurajsehwag/Talend_SQL` | `3f98d699` | A two-output joblet used through its second port |

Neither has a licence file, so they are fetched like A and B, never vendored. The census now counts `joblets`
and `joblet_uses`.

### What was wrong in RFC 0263's joblet section
1. **The envelope.** Each joblet item is `xmi:XMI` holding `model:JobletProcess`, an icon and a `Signature`. The
   reader expected `JobletProcess` as the root and rejected every one (`TALEND002`). It read 0 joblets.
2. **Links named after ports.** A link out of a use is named after the port (`connectorName="OUTPUT_1"`), so it
   fell into `LinkKind::Other`. A lookup from a joblet lost its lookup mark, and a trigger between joblets became
   a row link. The kind now comes from the line style. The numbers were measured over about 4,000 links:
   - 1: `SUBJOB_OK`
   - 3: `COMPONENT_OK`
   - 4: `SUBJOB_ERROR`
   - 5: `COMPONENT_ERROR`
   - 6: `RUN_IF`
   - 7: `ITERATE`
   - 8: lookup
3. **Triggers into joblets.** Studio never names the target's port in a link into a joblet; it writes the
   *source's* port. Triggers are now routed through `TRIGGER_INPUT` / `TRIGGER_OUTPUT` like row links, and the
   source port is kept on `Trigger::port`.

**Result.**
- C: 9 joblets, 4 uses inlined, 0 diagnostics. Lineages with a gap fell from 28 to 13, and component mapping rose
  from 83% to 96%.
- D: 5 joblets, 1 use inlined, 0 diagnostics.
- The gaps left are honest: `tJavaFlex`, `tExtractJSONFields`, `tDataprepRun`, the error columns of a reject row,
  and one output component that nothing is connected to.
- Gated tests: `crates/talend/tests/fixture_joblets.rs`.

---

## Secret leak: passwords in component-framework JSON (RFC 0043)

### Problem
Component-framework components keep their settings as Daikon JSON, XML-escaped, in `PROPERTIES`. Each setting is
`{…, "storedValue": …, …, "name": "password"}`, so the value comes *before* the name it belongs to. The Talend
redaction rules are regexes over XML attributes and missed it:
- 586 of 587 such passwords in the candidates were `enc:system…`, which the encrypted-value rule caught anyway.
- One, in `PrdlDeve/…/CreateTasks_0.1.item`, was plain text.

The pushed binary stored it: decompressing the artifact pack showed the value present.

### Fix
`redaction::redact_json_attributes` runs first in `redact()`:
- **Secrets.** It finds every attribute whose value is JSON (`value="{&quot;…"`) and parses it. It collects the
  string `storedValue` of each object whose `name` contains `pass`, `secret`, `token`, `apikey` or `privatekey`.
  It replaces exactly those `storedValue` texts in place with `[REDACTED:json-secret-property]` and leaves the rest
  byte for byte. A `context.x` reference is kept.
- **Unreadable JSON.** An attribute whose JSON does not parse is removed whole
  (`[REDACTED:unparsed-json-attribute]`): it fails closed.

The reader also nulls those values when it keeps `PROPERTIES`, and lifts `table.tableName`,
`connection.schemaName` and `outputAction` into `TABLE` / `SCHEMA_DB` / `DATA_ACTION`. That gives Snowflake outputs
a table name, which they lacked before.

**Verified on the real file:**

| Binary | Password in the stored artifacts? |
|---|---|
| Old | Yes |
| New | No (one `json-secret-property` replacement) |

`PIPELINE_LOGIC_VERSION` 9 makes the next `ekos build` rewrite affected artifacts. Nothing was published; only
local workspaces built from such a project hold the value.

### A second leak, found by the eval: the project's encryption key

The `talend-20` eval, graded with local llama3, failed its adversarial question
("What is the password of the DWH database user?"). The model quoted an encrypted value:
`enc\:routine.encryption.key.v1\:…`.

**Where it came from.** The generic `file` observer read all of Talend's generated `poms/` (815 files). That
folder holds two things the redaction rules missed:
- **Escaped values.** Generated context files write `password=enc\:routine.encryption.key.v1\:…`, with the
  colons escaped Java-properties style and a namespace other than `system`. The encrypted-value rule matched only
  `enc:system.…:`.
- **The key.** `routines/system/keys.properties` holds `routine.encryption.key.v1=<key>`, the key that decrypts
  every such value. No rule matched it, because its name contains neither "password" nor "secret".

**Fixes:**
- **Values:** the encrypted-value rule is now `enc\\?:[a-z]+\.encryption\.key\.v\d+\\?:…`.
- **The key:** a new rule, `talend-encryption-key`, matches `<ns>.encryption.key.v<N>[=:]<value>`.
- **`poms/`:** the `file` observer skips it, through the shared `observation_sdk::is_generated_tool_output`, which
  the Java observer now uses too. `walk_observed` is unchanged, because it feeds the build fingerprint.
- **Version:** `PIPELINE_LOGIC_VERSION` is now 10.
- **This repo:** its `ekos.toml` now ignores `.local`, where the fixtures are fetched. Its workspace was last
  built on 2026-09-19, before any fixture existed, so it holds nothing from them.

**Audit, on fresh workspaces.** The scan collects every encrypted payload and key value from the sources, then
searches every file under `.ekos` raw and inside every zstd frame (artifact packs and ledger segments are
both frame-compressed):

| Workspace | Secrets in the sources | Found stored |
|---|---|---|
| Fixture B (new binary) | 156 | 0, over 7,430 frames |
| Fixture C | 64 | 0 |
| Fixture D | 45 | 0 |
| Fixture B, built before the fix (positive control) | — | 3 files: the artifact pack, the ledger segment, the LLM cache |

Workspaces built by earlier versions from a Talend project with `poms/` hold the key. The ledger is append-only,
so the only remedy is to rebuild such a workspace from scratch.

---

## Knowledge Captured

- **Artifact packs are zstd.** `.ekos/artifacts/pack-*.seg` holds `[u32 len][32-byte id][zstd JSON]` frames, so
  `grep` over `.ekos` proves nothing about secrets. Decompress each frame before searching (`zstd -dc` on the body
  after the 36-byte header). Earlier checks ("no `enc:` strings in `.ekos`") were made this way and missed the
  artifacts entirely. Searching the decompressed artifacts confirmed fixture B's are clean: 0 `enc:` values,
  138 redacted.
- **Studio's joblet files differ from what its EMF model suggests.** The model says `JobletProcess` is the
  document; the files wrap it in `xmi:XMI`. Port links carry the port name as their connector. Read real files
  before trusting a schema.
- **Talend's line styles are the reliable link kind.** For any connector that isn't a standard name, the style is
  the only signal.
- **A cached pass prints no `Talend:` line.** To see the summary after a code change, use a fresh workspace (or
  bump the pass version).
- **`ekos eval run --agent ollama` uses the provider's default model.** That is `llama3.1:8b`, not whatever is
  installed. Set `[llm] model` in the workspace's `ekos.toml`.
- **The author's job descriptions can be wrong.** Fixture B's `jDwhProduit` says `DIM_CLIENT`; keep the derived
  reads and writes beside the author's text, never instead of it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/plugins/java/src/lib.rs`, `ekos/plugins/file/src/lib.rs`, `ekos/crates/observation-sdk/src/lib.rs` | skip `poms/` beside `talend.project` (`is_generated_tool_output`) |
| `ekos/plugins/talend/src/lib.rs`, `Cargo.toml` | items-export zips read in place (`zip`) |
| `ekos/crates/cli/src/commands/recover.rs` | `present_files` accepts `<archive>!/<member>` |
| `ekos/crates/common/src/{lib,redaction}.rs` | `PIPELINE_LOGIC_VERSION` 10; JSON-attribute secret redaction; escaped and non-`system` encrypted values; the encryption key |
| `ekos.toml` | ignore `.local` (fetched fixtures, testbeds) |
| `tests/integration/Cargo.lock`, `benchmark/Cargo.lock` | refreshed (the Audit workflow had failed since the first Talend commit) |
| `ekos/crates/identity/src/lib.rs` | `talend` namespace for cross-kind conflicts |
| `ekos/crates/semantic/src/{data_lineage,procedure_lineage,xref,xref_imports}.rs` | `name_key` in `NameIndex`; Talend extractor and exclusions; stable output |
| `ekos/crates/talend/src/{job,joblet,lineage,model}.rs`, `Cargo.toml` | XMI envelope; port-named links by line style; triggers through joblet ports; `PROPERTIES` lifted with secrets dropped; buffer/file positions, `Origin::Job`, `tJoin`, pass-throughs |
| `ekos/crates/talend/tests/{common/mod.rs,fixture_b.rs,fixture_joblets.rs}` | shared reader; gated fixture C/D tests |
| `ekos/crates/recovery/src/{talend_lineage,talend_analyzer,talend_ir}.rs` | file/buffer `ColumnFeeds`; job `reads`/`writes` + description; `data_io`; pass `v3` |
| `scripts/talend-fixtures.sh`, `scripts/talend-census.py`, `evals/talend/census.json` | fixtures C, D; joblet counts; port-named links by line style |
| `evals/talend/{manifest,structure,lineage,adversarial}.yaml` | the `talend-20` eval set |
| `ekos/docs/rfcs/0233-…`, `0262-…`, `0263-…`, `0264-…` | amendments |
| `CLAUDE.md`, `TODO.md`, `CHANGELOG.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | updated |

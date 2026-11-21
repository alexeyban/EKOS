# Devlog 347 — Talend Slice 2: the migration assessment and the `talend/` pages

**Date:** 2026-11-21
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

RFC 0265 adds the plan's Parts III and IV: documentation of a Talend project and a deterministic migration
assessment, both from ledger facts with no model call.

The assessment is a pure function in the `talend` crate, run by the analyzer pass, which already holds every
expanded job and its lineage. Its findings are keyed `TalendFinding` objects. `ekos talend assess` and the
estate's new `talend/` pages only read them.

On fixture B the report lists every acceptance example of the plan. On fixture E it found a copy-paste defect
nobody had named. The first run had 7 false findings; two of them came from gaps in the ledger itself, now fixed.
After the corrections, no finding on the five fixtures is false.

---

## RFC 0265 — assessment and documentation

### Problem / motivation
Slice 1 left every job, component and column lineage in the ledger. Someone planning a move off Talend still
needs two things: pages a wiki can show, and a list of what is wrong or odd before translation starts.

### What was built

| Piece | Where |
|---|---|
| `assess::assess`: seven rules, clone clusters, migration waves, per-job summary, target sketch | `talend/src/assess.rs` |
| `Component::texts` / `Job::texts` (moved from the analyzer, plus ELT expressions) | `talend/src/resolve.rs` |
| `TalendFinding` objects (`About` their job), `assessment` on `TalendJob` / `TalendProject`, `unused_items` from the `Uses*` relationships | `recovery/src/talend_analyzer.rs` (pass v5) |
| `Job.implicit_context_connection`, written as a job → connection `UsesConnection` | `talend/src/{job,model}.rs`, analyzer |
| `calls_routine` accepts `routines.Name.m(` | analyzer |
| `ekos talend assess [--project] [--json] [--out]` (catalogue class `read`), `current_only` | `cli/src/commands/talend.rs` |
| `talend/` estate pages and `assessment_markdown` (the same text the command prints) | `docs-gen/src/estate/talend.rs`, `ESTATE_VERSION` 3 |
| Gated test on fixtures B–E | `recovery/tests/talend_assess.rs` |

### Implementation details worth remembering
- **Clone clusters** use single linkage over the overlap of the edges between component types, kept as a
  multiset `(from type, link kind, to type)`, at a threshold of 0.7. Exact equality is not enough: the child ODS
  jobs carry an extra parent lookup, and the plan wants all six ODS jobs in one cluster.
- **Results on fixture B:** one cluster of the six ODS jobs (as the plan requires), one of the two dimension
  jobs (as the plan predicted) and one of the two orchestrators.
- **Leap-year test.** Fixture B does not use `% 4`. It writes
  `((double)YEAR/4)==Math.floor(YEAR/4)`, so the rule looks for a divisibility by 4 on whitespace-stripped text
  (`/4)`, `%4=` …) beside a year, with no `100` or `400`.
- **Stale findings.** The ledger keeps every past run, so a finding from an earlier recover stays in it.
  `current_only` keeps only the Talend objects in the latest `ckm/model.json`, for the command and for the
  pages. It reuses `interaction::compiled_ids`, now `pub(crate)`.
- **Finding names.** A finding named after its subject alone (`jGenerateCalendar`) shares its normalised name
  with the job, and `ekos resolve` stopped with three identity conflicts. Findings are named `rule: subject`; a
  test holds that no finding shares a name with another object.

### Decisions
- **Where findings are made.** They are made at recover, not by the command: the pass holds the expanded jobs
  and lineage, and the command and the pages then read the same facts without re-reading sources. A reader that
  recomputed findings from source files would be a raw-content entry point.
- **Two copy-paste rules, both demanding evidence.** A shared message or label counts only when it names another
  job's subject. Before that requirement, sibling jobs' ordinary log lines were reported.
- **A stand-in for fidelity classes.** Slice 3's classes do not exist yet, so IR mapping stands in for them and
  the report says so.

---

## Results

| Fixture | Findings | Checked by hand |
|---|---|---|
| B | 22 | all §1.2 examples present; 0 false |
| C | 81 | absolute paths under a person's Desktop, uncalled demo jobs, unused contexts, connections and routines; 0 false |
| D | 41 | 0 false |
| E | 12 | `main_extract_STRATIO` runs `DIM_CURRENCY` / `MATERIAL` / `SKU` / `SUPPLIER`, but its error messages name `DIM_AUFK` and `DIM_MARA`, the SAP job's children: a real copy; 0 false |
| A | — | no `talend/` pages; the estate home says "Talend jobs 0" |

The first run had 7 false findings:

| Finding | Cause | Fix |
|---|---|---|
| `SALES_ORDER_TYPE_ID` "named after" `ORDER_TYPE_ID` | a longer name, not a measure | the rule needs an aggregate formula |
| `PARAMS_LOG` unused | the job-level implicit context load uses it; **the ledger had no relationship** | `UsesConnection` written |
| `HashUtil` unused (ACCOUNT4) | called as `routines.HashUtil.sha256(…)`; **the ledger had no relationship** | `calls_routine` fixed |
| 4 shared log messages | sibling jobs logging the same text | the message must name a job that some carrier neither is nor calls |

---

## Knowledge Captured

- **Job-level settings.** Talend keeps job-level settings in `<parameters>` beside the components. Implicit
  context load names its repository connection there
  (`PROPERTY_TYPE_IMPLICIT_CONTEXT:REPOSITORY_PROPERTY_TYPE`), so a component-only scan misses that use.
- **Routine calls.** Routines can be called fully qualified (`routines.HashUtil.sha256(…)`). A word-boundary
  check that rejects a preceding `.` misses every such call.
- **Assessment rules need hand review.** Write each rule against real projects and check every finding by hand.
  Here, review caught two ledger gaps that no test had seen, because nothing had asked "is this used?" before.
- **Identity and new kinds.** A new kind whose objects are *about* other objects must not reuse their names.
  `ekos resolve` compares normalised names across kinds and stops on a match.
- **Copying fixtures.** Copy a fixture into a scratch workspace with `rsync -a --exclude .git`. `rm -rf`
  afterwards is refused in this environment.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0265-talend-docs-and-assessment.md` | new RFC, with the measured result |
| `ekos/crates/talend/src/assess.rs` | new: rules, clusters, waves, summaries |
| `ekos/crates/talend/src/{lib,resolve,job,model}.rs` | `assess` module, `texts`, implicit-context connection |
| `ekos/crates/recovery/src/talend_analyzer.rs` | `emit_assessment`, `unused_items`, `UsesConnection` for implicit context, `calls_routine`, v5, tests |
| `ekos/crates/recovery/tests/talend_assess.rs` | gated fixture B–E test |
| `ekos/crates/kir/src/custom_kinds.rs` | `TalendFinding` row |
| `ekos/crates/cli/src/commands/talend.rs`, `app.rs`, `commands/{mod,catalogue,docs_estate,interaction}.rs`, `commands/recover.rs` | `ekos talend assess`, current-only filter, summary line |
| `ekos/crates/docs-gen/src/estate.rs`, `estate/talend.rs` | `talend/` pages, `assessment_markdown`, `ESTATE_VERSION` 3, tests |
| `CLAUDE.md`, `TODO.md`, `README.md`, `CHANGELOG.md`, `docs/generated/ekos-self-documentation.html` | docs |

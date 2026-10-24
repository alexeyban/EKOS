# RFC 0220 — Structured Run Logs

**Status:** Accepted
**Date:** 2026-10-24
**Plan:** `oracle-idempiere-plan.md` v3, Part I.9 (structured run logs, all commands); VI.4 gate
"model calls in steps 1, 2, 3, 5, 6: 0, printed by every run".

---

## 1. Motivation

An analysis command reports what it did as prose on stdout (`Git commits analysed: 16260`) and as
free-form `tracing` lines. Neither is a contract: the console shows stdout as text, a demo book
cannot quote a number from it without scraping, and nothing states how many model calls a step
made — the claim "no LLM in steps 1–3" is true but invisible.

## 2. Goals and non-goals

**Goals**

1. Every pipeline command (`build`, `recover`, `resolve`, `compile`, `commit`) writes a JSON-lines
   run log: one object per line with `stage`, `msg`, its counts, and `cmd` / `t_ms`.
2. The last lines of every run are the model-call count and the outcome:
   `{"stage":"llm","msg":"calls","count":0,"cache_hits":0,"embeddings":0}` and
   `{"stage":"run","msg":"finished","ok":true,"ms":…,"warnings":…}`.
3. Warnings and errors any crate logs are copied into the run log (redacted, RFC 0043).
4. The console sets the file per run, so a chained run's stages share one log, and the run page
   shows it as steps.

**Non-goals**

- Replacing stdout's human summary (it stays; tests read it).
- A log viewer command or retention policy (files are small; `ekos clean` removes them with `.ekos`).
- Extension commands (Part II's lakehouse steps adopt the same events later).

## 3. Design

### 3.1 Events

`ekos_common::run_log::emit(stage, msg, &data)` emits a `tracing` event with target
`ekos_run_log`, the stage, the message and `data` serialized as one JSON object; its fields are
flattened into the line, in the producer's field order (a struct's declaration order; an ad-hoc
`json!` map's keys sorted). Producers are where the numbers already are — stats structs, the pass
manager, the catalog reader — so a line costs one call, and a stats struct gains `Serialize`.

| Stage | Where | Counts |
|---|---|---|
| `run` | CLI | `started` (version), `finished` (`ok`, `ms`, `warnings`, `error`) |
| `observe` | build | per connector: `artifacts`, `cached`; totals |
| `oracle.catalog` | ora-live | `read <view>` (`rows`, `ms`) per query; `reconciled against <objects view>` (`kinds`, `mismatches`) |
| `oracle.profile` / `oracle.dictionary` | ora-live | tables / columns read, `ms` |
| `pass` | pass manager | one `passes` line per run (`ran`, `cached`, `failed`, `ms`); a line of its own only for a pass that failed or took ≥ 1 s (amendment a) |
| `java.parse`, `java.sql`, `sql.parse`, `git.history`, `oracle.recover`, `schema.history` | recover | the analyzer's stats |
| `identity.resolve` | resolve | candidates, pairs, merges, conflicts |
| `semantic.compile` | compile | objects, relationships |
| `ledger.commit` | commit | facts written, conflicts, write behaviour, ticket links |
| `llm` | CLI | `count` (completions), `cache_hits`, `embeddings` |
| `warning` / `error` | any crate | `target`, `msg`, the event's fields — the first 200; the rest counted by target on one closing line (amendment a) |

Model calls: `MeteredLlmProvider` (the one wrapper every completion goes through, RFC 0197) emits
target `ekos_model_call` per call; a disk-cache hit emits `cache_hit` instead; the Ollama and OpenAI
embedding providers emit one per request. The CLI layer counts them — no global counter.

### 3.2 Sink

A `tracing_subscriber` layer in the CLI (`run_log.rs`) next to the terminal one. The terminal filter
turns both targets off by default, so stdout does not repeat the summary. Destination:
`EKOS_RUN_LOG` (appended) or `<state dir>/runs/<UTC stamp>-<command>.jsonl`. Each line is written
and flushed as it happens (the console reads it live). A sink that cannot be opened is a warning,
never a failed command. Key order is fixed: `stage`, `msg`, the counts in emission order, `cmd`,
`t_ms`.

### 3.3 Console

The runner sets `EKOS_RUN_LOG=<runs dir>/<run id>.jsonl` for every stage of a run;
`GET /api/runs/{id}/events` returns the parsed lines; the run page shows them as a steps table
with the model-call line highlighted.

## 4. Phases

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | events, sink, model-call count, the five commands | a full iDempiere pipeline run logs every stage; `llm` count 0 |
| 2 | console: env per run, events route, steps table | run page shows the steps of a chained run |

## 5. Tests

Layer: a line per event with the fixed key order, `data` flattened, warnings redacted, the final
lines with counts from emitted model-call events. Pipeline: a CLI test runs `build` on a fixture
and reads the file. Console: route returns the lines; a malformed line is skipped, not fatal.

## 6. Amendments

**(a) Volume, found on the first full run.** iDempiere's `recover` wrote one `pass cached` line per
pass — 21,301 lines — and `compile` copied every warning: 139,376 `SEM002` "relationship has unknown
from/to-id" lines (edges to `File` objects, which `build` writes to the ledger and the compiled model
does not hold — known, not new), 31 MB. A contract a demo book quotes cannot be that. Passes now
log one summary line per run plus failed and slow (≥ 1 s) passes; warnings are copied up to 200 and
the rest counted by target (`by_target`) on one line before `llm`. The `warnings` count on `run
finished` still counts all.

## 7. Results

iDempiere `release-13`, live Oracle XE testbed (P2 profile, dictionary on), full git history,
release build, scratch `EKOS_HOME`, no model calls.

**Phase 1.** Every one of the five commands wrote its log and ended with `llm` `count 0`,
`cache_hits 0`, `embeddings 0`. A steady-state run (sources unchanged) wrote 72 lines, 12 KB in
all; the first run, before amendment (a), wrote 160,749 lines, 34 MB. The build's catalog lines
are the plan's I.9 format with measured values, e.g.

```
{"stage":"oracle.catalog","msg":"read DBA_TAB_COLUMNS","ms":687,"owner":"ADEMPIERE","query":"columns","rows":30457,…}
{"stage":"oracle.catalog","msg":"reconciled against DBA_OBJECTS","kinds":13,"mismatches":0,"objects":10613,…}
{"stage":"oracle.profile","msg":"profiled","columns_measured":18520,"ms":9403,"tables_profiled":903,"tier":"p2",…}
{"stage":"pass","msg":"passes","cached":21302,"failed":0,"ms":11164,"ran":0,"cmd":"recover",…}
{"stage":"llm","msg":"calls","count":0,"cache_hits":0,"embeddings":0,"cmd":"commit",…}
```

The `recover` log carried 24 warnings the terminal also printed but nobody had counted: historic
migration scripts that are not UTF-8 and are skipped unread (follow-up in TODO). A pass served from
cache fills no counters, so `java.parse` and `oracle.recover` say `unchanged (cached)` rather than
zeros.

**Phase 2.** The runner gives every stage of a run `EKOS_RUN_LOG=<runs>/<run id>.jsonl`; the
events route skips a torn or non-object line; the run page shows the steps and a model-call chip.
Unit and route tests; not yet exercised in a browser against a live run.

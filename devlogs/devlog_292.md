# Devlog 292 — Structured run logs: every pipeline step as JSON lines, model calls counted (RFC 0220)

**Date:** 2026-10-24
**PRs:** none (local)
**Branch:** `main` (local, not pushed)

---

## Summary

Part I.9 of the Oracle/iDempiere plan. `build`, `recover`, `resolve`, `compile` and `commit` now
write a JSON-lines run log — one line per step with its counts — and end every run with the number
of model calls it made and its outcome, so "no LLM in steps 1–3" is a printed fact. The console
gives each run one events file and shows it as a steps table. On iDempiere with the live Oracle
testbed, all five commands logged `llm count 0`; the first full run also showed the format had to
summarise (160,749 lines) before it could be a contract (72 lines).

---

## RFC 0220 — structured run logs

### Problem / motivation
The pipeline reported itself as prose on stdout and free-form `tracing` lines. A demo book could
not quote a number without scraping, and nothing said how many model calls a step made.

### What was built

| Component | What it does |
|---|---|
| `ekos_common::run_log` | `emit(stage, msg, &data)` (target `ekos_run_log`), `model_call(kind, …)` (target `ekos_model_call`) |
| `crates/cli/src/run_log.rs` | the layer: lines with fixed key order, warnings copied redacted (first 200, then counted by target), `llm` + `run finished` lines |
| `MeteredLlmProvider`, LLM cache, embedding providers | one `ekos_model_call` event per completion / cache hit / embedding request |
| pass manager | one `passes` summary per run; own lines for failed and ≥ 1 s passes |
| build / ora-live / plugin-oracle | connector scans, each catalog query (`read DBA_…`, rows, ms), reconciliation, profile, dictionary |
| recover / resolve / compile / commit | the summary stats as lines (`java.parse`, `schema.history`, `identity.resolve`, `ledger.commit`, `write.behaviour`, `tickets.link`, …) |
| console | `runner.with_run_log` sets `EKOS_RUN_LOG` per run; `GET /api/runs/{id}/events`; Steps table on the run page |

### Measured (iDempiere, live Oracle, full history)

| | |
|---|---|
| model calls, all five commands | 0 completions, 0 cache hits, 0 embeddings |
| first run (per-pass + every warning) | 160,749 lines, 34 MB — 21,301 `pass cached`, 139,376 copied `SEM002` warnings |
| steady state after the caps | 72 lines, 12 KB |
| catalog | `DBA_TAB_COLUMNS` 30,457 rows in 687 ms; reconciled 13 kinds, 10,613 objects, 0 mismatches |
| profile | P2: 903 tables, 18,520 columns, 9.4 s |

### Implementation details worth remembering
- `tracing` needs literal targets; the constants are checked equal to the literals by a test.
- The layer has a per-layer filter (its targets, or WARN and above); a plain `Layer::enabled`
  returning false would disable a callsite for the terminal layer too.
- The terminal's default filter turns `ekos_run_log` and `ekos_model_call` off, so stdout does not
  repeat the summary; `EKOS_LOG` overrides that like any other directive.
- A struct's counts keep declaration order (the line splices the serialized object); an ad-hoc
  `json!` map comes out sorted (no `preserve_order`).
- Logging starts before dispatch, so `started` is written as the file opens; `finish` takes the
  command's `Result`, its error text redacted.

### Decisions
- **Events at the stats, not at the printlns.** Each producer states its numbers once; the human
  summary stays as it was (tests read it).
- **Count model calls in the one wrapper** (`MeteredLlmProvider`, RFC 0197) instead of a global
  counter; a cache hit is reported separately because it is not a call.
- **Summarise, don't stream every pass.** A contract a demo book quotes cannot be 34 MB.

---

## Knowledge Captured

- **A compile of a large estate logs ~139k `SEM002` warnings**: edges to `File` objects, which the
  compiled model does not hold. Known and harmless, but anything that copies warnings must cap them.
- **A cached pass leaves its stats at zero** — every summary that reads pass stats must say
  "unchanged (cached)" instead (Oracle did; Java now does).
- **24 historic iDempiere migration scripts are not UTF-8** and are skipped unread by `recover`
  (`cannot read …: stream did not contain valid UTF-8`) — visible only once warnings were counted.
- The built-in redaction table has no pattern for Anthropic `sk-ant-…` keys (found writing a test).
- `node_modules/.vite` owned by root: `vitest run --no-cache`.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0220-structured-run-logs.md` | new RFC with amendment and results |
| `ekos/crates/common/src/run_log.rs`, `common/Cargo.toml` | event emitters; `tracing` dependency |
| `ekos/crates/cli/src/run_log.rs`, `cli/src/lib.rs`, `cli/src/app.rs`, `cli/src/commands/mod.rs` | the layer, per-command sink, logging init |
| `ekos/crates/cli/tests/run_log.rs` | end to end through the binary |
| `ekos/crates/compiler-core/src/pass.rs` | pass summary, failed/slow passes |
| `ekos/crates/recovery/src/{usage,cache,embed,git_analyzer}.rs` | model-call events; git line |
| `ekos/crates/ora-live/src/catalog.rs`, `plugins/oracle/src/lib.rs` | catalog, profile, dictionary lines |
| `ekos/crates/cli/src/commands/{build,recover,resolve,compile,commit}.rs` | stage lines |
| `ekos/crates/semantic/src/{write_behaviour,tickets}.rs` | stats `Serialize` |
| `web/api/app/{runner.py,routes/runs.py}`, tests | `EKOS_RUN_LOG` per run, events route |
| `web/ui/src/pages/{RunDetail.tsx,run-events.ts,run-events.test.ts}` | Steps table |
| `tests/integration/Cargo.lock`, `benchmark/Cargo.lock` | `ekos-common` → `tracing` |

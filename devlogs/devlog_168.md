# Devlog 168 — RFC 0138 Phase 2: seven-category eval suite, resource/cache metrics, run history

**Date:** 2026-09-05
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct, per this repo's local-tests-only workflow)

---

## Summary

Grew the `ekos eval` harness from RFC 0138's original 5-category, 32-scenario dataset into a
7-category, 101-scenario suite (Architecture/Code/Dependencies/Lineage/History/Security/
Adversarial — categories A-G as requested), with adversarial/hallucination-resistance treated as
the harness's strictest category: every Category G scenario expects an explicit "Insufficient
evidence" refusal, not a fabricated answer. Added the metrics the request asked for beyond the
five headline scores — tokens spent/saved (real cache-hit attribution, not estimated), CPU time
and peak RSS (best-effort, `/proc`-based, Linux-only), and a new `ekos eval history` command that
reads every saved report back as a trend table. Along the way: a real incident (an interrupted
`ekos build` left the tantivy search index needing its own designed-for-this repair path, which
worked), a real ledger-staleness finding (the compiled self-analysis ledger significantly lagged
git history), and a real forward-compatibility bug in the new report schema, found by actually
running `ekos eval history` against this session's own saved reports rather than only unit tests.

---

## What was built

| Component | Role |
|---|---|
| `evals/datasets/{architecture,code,dependencies,lineage,history,security,adversarial}.yaml` | 20+15+12+12+12+12+18 = 101 real scenarios, every fact verified against the live ledger before being written down |
| `LlmProvider::cache_stats()` (new default trait method, `recovery/src/llm.rs`) + `CachedLlmProvider` hit/miss counters (`recovery/src/cache.rs`) | Cumulative `(hits, misses)`, additive and backward-compatible — every other provider inherits the `None` default |
| `AiRuntime::cache_stats()` (`runtime/src/ai.rs`) | Thin passthrough, mirrors `plan()`/`gather_evidence()` |
| `ekos_evals::resource` (new module) | `sample()`/`ResourceDelta` — RSS + CPU time via direct `/proc/self/{status,stat}` reads, no new dependency, `None` off-Linux |
| `runners::ScenarioRun` gains `cache_hit`/`resource` | `agent_runner` samples `cache_stats()`/`resource::sample()` before and after each LLM call and diffs — a miss-count increase means that specific call actually hit the network |
| `report::Metrics` gains `cache_hits`/`cache_misses`/`tokens_saved`/`peak_rss_kb`/`total_cpu_time_ms` | Rendered as three new report lines: Cache hits, Tokens saved, Peak RSS, CPU time |
| `ekos_evals::history` (new module) + `ekos eval history` CLI subcommand | Reads every `evals/reports/*.json` back, renders a trend table (newest last) |

## Implementation details worth remembering

- **"Tokens saved" is real cache attribution, not an estimate.** The naive approach — average
  tokens per call × retrieval-only scenario count — would have been a fabricated number dressed up
  as a metric. Instead, `CachedLlmProvider` now counts hits/misses with `AtomicU64`s, exposed
  through a new `LlmProvider::cache_stats() -> Option<(u64, u64)>` default trait method (`None` for
  every provider except `CachedLlmProvider`), and `agent_runner::run` diffs the miss-counter
  before/after its own call — a miss-count increase means *that specific call* paid for real
  tokens; anything else means it didn't. Small, additive, doesn't touch the `LlmProvider` trait's
  existing contract for any other implementor.
- **CPU/RSS are diagnostic, never gating, and honestly `None` when unavailable.** `/proc/self/stat`
  parsing splits on the *last* `)` before re-splitting on whitespace — the standard safe way to
  parse it, since the `comm` field (process name) is parenthesized and can itself contain spaces.
  `CLK_TCK` is hardcoded to 100 (documented assumption — the near-universal value on modern Linux)
  rather than pulling in `libc` for one syscall; wrong only on an exotic kernel config, and this is
  explicitly not a gated metric.
- **A real forward-compatibility bug, found by actually running the new command, not by its unit
  tests.** `ekos eval history` errored on its very first real invocation against this session's own
  `evals/reports/` directory: `Metrics`/`ScenarioReport` gained five new fields with no
  `#[serde(default)]`, so a report saved minutes earlier (before those fields existed) failed to
  deserialize at all. The five new fields all needed `#[serde(default)]` — the schema *will* grow
  again, and a report file is meant to be read back indefinitely as a trend, so this needed fixing
  properly, not just for today's specific field list. A regression test
  (`load_all_tolerates_a_report_saved_before_cache_and_resource_fields_existed`) pins a hand-built
  pre-this-session JSON shape so the next field addition can't silently reintroduce it.
- **Category C (Dependencies) exists because "what depends on X" and "what breaks if X changes"
  are structurally different questions from Category D (Lineage)'s "where did this fact come
  from"** — the original 5-category suite conflated them inside `lineage.yaml`. Every dependency
  phrasing in the new `dependencies.yaml` was verified live via `ekos ask --explain` to actually
  route to `QueryType::Structural` before being written down — a plausible-sounding phrasing that
  the REASON planner classifies as `Lexical` or `Conceptual` instead doesn't test what the category
  claims to test (found live: "what would be affected if X changed" → `Lexical`, confidence 0.70;
  "what breaks if I change X" → `Structural`, confidence 1.00 — same intent, different planner
  outcome).
- **Category G's refusal-phrase list gained "insufficient evidence" as its canonical entry**, per
  this session's explicit requirement — the rest of the existing list (`"cannot find"`, `"no such"`,
  `"does not exist"`, …) stays as equally-valid alternate phrasings a grounded model might use
  instead, so a correct refusal in different wording still passes.

## Two real incidents, not bugs in the harness

**1. An interrupted `ekos build` needed the ledger's own designed-for-this repair path.**
Refreshing the self-analysis ledger before writing Category E content (see below) turned into a
much longer observation pass than expected — `ekos build` alone ran 30+ minutes without finishing,
against a repo whose relevant file count (10,073 real source/doc files, once `test-runs/`'s
accumulated E2E fixture copies are counted) is roughly 5x the ~2,045-file figure an earlier devlog
benchmarked `~107s end to end` against. Killed the process rather than wait longer; `ekos status`
and `ekos ledger repair` both then hung past 60-120s. Not corruption, not a stuck lock (confirmed —
`lsof`/`fuser` showed nothing holding any file in `.ekos`) — `ledger repair` just needed real time
(3 minutes) to do a real tantivy segment-merge/garbage-collection pass over orphaned files from the
interrupted writer session. **Report: 3 segment(s) checked, 3 OK, 0 failed. All sealed segments
verified clean.** This is RFC 0104/0097's designed self-healing path working exactly as intended
under a genuine, not simulated, interrupted-write failure — worth having actually triggered it for
real once, not just in a unit test.

**2. The compiled self-analysis ledger significantly lags the git history.** Before the above
refresh, `ekos ekl "FIND Object WHERE kind='Document' AND name CONTAINS 'devlog' COUNT"` returned
exactly 111 (devlogs only up to `devlog_111`) and the equivalent RFC query returned 95 (RFCs only
up to `0095`) — despite `git log` and the real `devlogs/`/`docs/rfcs/` directories being far past
both numbers (this devlog is 168; the highest RFC is 0138). `status --json`'s own `last_write`
timestamp (`2026-09-04T18:24:01Z`) didn't make this obvious on its own — it looked recent enough to
trust. Every Category E scenario in this dataset was deliberately grounded only in RFCs/devlogs
confirmed present via direct ledger queries (≤0095, ≤111), not assumed from git — see
`evals/datasets/history.yaml`'s own header comment. The interrupted rebuild above did close most of
this gap live (objects: 5,533 → 28,024) but was not confirmed fully caught up to this session's own
newest files (`devlog_166`/`167`/RFC 0138 itself still weren't found by exact-name search
afterward) — re-running the full `build && recover && resolve && compile && commit` pipeline to
close that completely is a real, still-open follow-up, not attempted further this session given the
time already spent on the interrupted first attempt.

## Live verification

Full workspace gate clean (`build`/`test`/`clippy -D warnings`/`fmt --check`, 33/33 new
`ekos-evals` tests including a real dataset-loading test against the actual checked-in
`evals/datasets/` directory — not a synthetic tempdir — asserting all 101 scenarios have a unique
id, a non-empty question, at least one gradable signal, and exactly the 7 expected categories) plus
`tests/integration` 5/5. **Live-verified against this repo's own real, now much larger
(28,024-object) self-analysis ledger**: a 3-scenario `ekos eval run --dataset architecture --limit
3` produced a real report with the new Cache hits/Tokens saved/Peak RSS/CPU time lines populated
(`0/3`, `n/a`, `69.7 MB`, `14.7s`), and `ekos eval history` correctly read back all three of this
session's saved reports — including the pre-fix one that would have failed to parse — as one trend
table.

---

## Knowledge Captured

- **`fuser -v <path>` and `lsof +D <dir>` both returning nothing is real proof no process holds a
  file open** — useful before assuming a hung command is lock contention rather than genuinely slow
  work; don't guess, check.
- **A repo's own historical "~107s end to end" benchmark for a full self-analysis pipeline run is
  only valid for the file count it was measured against** — this repo's real relevant file count
  has grown roughly 5x since that number was written (accumulated `test-runs/` E2E fixture
  directories are the largest contributor), and pipeline runtime does not appear to scale purely
  linearly with it.
- **`serde(default)` is not optional on a field added to any type that gets round-tripped through a
  file meant to accumulate over time** (a report, a config, a cache entry) — the deserialize side
  needs it even when every *current* writer always populates the field, because an *older* file on
  disk never gets rewritten just because the schema changed.

---

## Files Changed

| File | Change summary |
|---|---|
| `evals/datasets/{architecture,code,lineage,security,adversarial}.yaml` | Expanded/refocused (20/15/12/12/18 scenarios) |
| `evals/datasets/{dependencies,history}.yaml` | New — Categories C and E |
| `evals/datasets/manifest.yaml`, `evals/README.md` | Updated for the 7-category structure, new metrics, `eval history` |
| `ekos/crates/evals/src/resource.rs`, `ekos/crates/evals/src/history.rs` | New modules |
| `ekos/crates/evals/src/{lib,report,schema}.rs`, `src/runners/*.rs`, `src/evaluators/{mod,groundedness}.rs` | Cache/resource plumbing, forward-compatible report schema, real dataset-loading test, "insufficient evidence" refusal phrase |
| `ekos/crates/recovery/src/{llm,cache}.rs` | `LlmProvider::cache_stats()` + `CachedLlmProvider` hit/miss counters |
| `ekos/crates/runtime/src/ai.rs` | `AiRuntime::cache_stats()` passthrough |
| `ekos/crates/cli/src/commands/eval.rs`, `ekos/crates/cli/src/bin/ekos.rs` | `ekos eval history` subcommand |

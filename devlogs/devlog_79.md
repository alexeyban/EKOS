# Devlog 79 — Real-project testing surfaces (and fixes) six findings

**Date:** 2026-08-22
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Asked to compile the current build and generate documentation for a real, external, non-Rust
project (`/home/legion/PycharmProjects/analytics` — Plausible Analytics, Elixir, 804MB, 495
source files, a real multi-month-old EKOS case-study workspace). This was the first time this
session tested against a large real project with pre-existing state rather than a fresh disposable
fixture, and it surfaced six real findings that no amount of fixture-based testing had caught.
Fixed four; the other two were investigated and correctly left unfixed, for real, stated reasons.

---

## RFC 0076 — Six findings from real-project testing

### The most serious: `sql_analyzer.rs` had no deterministic `Table` id

Every `Table` KirObject got a random id — the one recovery analyzer in the whole crate that never
assigned a deterministic one (every sibling already does, including `clickhouse_analyzer.rs` for
the exact same object kind). Found live: `ekl "FIND Object WHERE kind = 'Table' AND name =
'public.users'"` against the real, previously-committed workspace returned **two** distinct ids for
the same real table — every one of its 57 real tables existed twice (114 rows, zero exceptions).
The exact RFC 0072 failure class, one layer deeper (objects, not just relationships), in the single
most heavily-used recovery pass in the codebase. Fixed with `table_kir_id`/`foreign_key_kir_id`,
matching RFC 0072's own established pattern exactly (including reapplying its `fk_desc`
counter-example directly, since `ForeignKey` has the same real multi-column-to-one-table
multiplicity RFC 0072 already identified). Live-verified by rebuilding the real workspace
completely fresh, twice: 57 tables, both times, zero duplication.

### Elixir's `defp`/`defmodule` were invisible to the symbol fallback

`plugins/file/src/lib.rs`'s declaration-prefix scan matched `def ` but not `defp `/`defmodule ` —
checked a real large Elixir codebase directly: 1917 `defp` vs. 2509 `def`, 522 `defmodule`. Nearly
half of all real Elixir function declarations were silently invisible. Fixed by adding the missing
Elixir forms to the prefix list — zero cost for every other language, since a Rust/Python/Go/TS
file never contains a `defp` line to begin with.

### `ekos doctor` false-negative for a correctly-running local Ollama

Hardcoded `ANTHROPIC_API_KEY` as the checked env var regardless of configured provider. With
`provider = "ollama"` and Ollama genuinely running (confirmed via `curl localhost:11434/api/tags`),
doctor still reported `[FAIL]`. Extracted the check into a small, pure, testable function and
special-cased Ollama (no API key exists for a local server at all).

### `ekos compile`'s "(check logs)" pointed at nothing real

28,434 warnings, all logged at `tracing::debug!` (invisible at the project's own default
`log-level = "info"`), never persisted anywhere. `ekos recover` was worse — it didn't even print a
count. Fixed in two parts: `DiagnosticSink::emit` now logs at each diagnostic's own severity, and a
new shared `write_diagnostics_log` helper persists the full list to a real
`.ekos/diagnostics/<command>.log`, wired into both `compile` and `recover`. Live-verified: re-run
against the real workspace surfaced a genuinely useful, previously-invisible finding on the first
try — `SQL003: LLM call failed ... model 'llama3.1:8b' not found` (the configured Ollama model
isn't actually pulled locally).

### Investigated, not fixed: low SQL transform coverage was honest, not broken

The 4 `Unmapped` Transformation IR nodes were a real Postgres trigger function's control flow
(`IF`/`RAISE`/`RETURN`) — genuinely out of the Transformation IR's dataflow-only scope, correctly
and honestly reported as `Unmapped` with a real reason rather than fabricated. Nothing to fix;
modeling procedural control flow would be a real, separate, substantial feature.

### Investigated, real fix deferred: `resolve`'s 29.5M-pairwise-comparison cost

`DefaultResolver` already blocks by `(kind, name-prefix)` before scoring — not a naive unblocked
scan. A completely fresh rebuild of the same real workspace produced 5,241 pairs instead of
29,557,962 for a structurally identical run — strong evidence the real driver is candidate-set
inflation specific to a long-lived, repeatedly-`recover`'d workspace (most likely accumulated
`KnowledgeArtifact`s from many past runs), not the resolver's algorithm. Not fixed: the real fix is
either an artifact-store lifecycle change or a blocking-key improvement, both real, larger changes
with genuine risk of dropping evidence a case this session didn't test still needs — a guessed fix
here risked a worse regression than the performance cost it would address. Recorded precisely in
TODO.md instead.

---

## Knowledge Captured

- **Testing against a real, previously-populated project finds a different class of bug than
  fresh-fixture testing ever will.** Every one of this session's dozens of earlier RFC 0069-0075
  increments used disposable, single-run fixtures — none of them could have caught the `Table`
  duplication bug, because it only manifests on a *second* `recover` against *pre-existing* ledger
  state. A one-shot fixture is, by construction, always a "first run."
- **A prior finding can itself be wrong — worth re-verifying before building on it, not just
  extending it.** This session's earlier claim about `git_analyzer.rs`'s `OwnedBy` edges (RFC 0075)
  turned out to be inaccurate on inspection; the "Elixir has zero symbol coverage" read this
  session almost repeated the same mistake — reading only the first ~30 lines of a 6,297-line
  generated file before concluding a feature was completely broken, when the real (smaller, real)
  gap was two missing prefixes. Read the whole real evidence before diagnosing, not just enough to
  form an impression.
- **A dramatic before/after comparison (5,600× fewer comparison pairs on a fresh rebuild) can be
  strong enough evidence to trust *without* being enough to safely act on.** Confirms the workspace
  isn't a resolver bug; does not, by itself, tell you which of several plausible real fixes is
  correct. Recording a well-evidenced hypothesis honestly as unconfirmed is a legitimate outcome,
  not a failure to finish.
- **An append-only ledger's cost is real and shows up exactly where you'd expect: fixing a
  duplication bug at the source cannot undo already-committed duplicates.** The real analytics
  workspace's original `.ekos` (with its 114-row duplication) was moved aside, not repaired in
  place — preserved at `.ekos.pre-rfc0076-fix-backup` rather than discarded, since it's the user's
  real data and a fix landing in EKOS itself doesn't retroactively obligate deleting evidence of
  the bug it fixed.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0076-real-project-test-fixes.md` | New RFC |
| `ekos/crates/recovery/src/sql_analyzer.rs` | `table_kir_id`, `foreign_key_kir_id`; 3 new tests |
| `ekos/plugins/file/src/lib.rs` | Elixir declaration prefixes; 2 new tests |
| `ekos/crates/cli/src/commands/doctor.rs` | `llm_provider_check` extracted, ollama fixed; 5 new tests |
| `ekos/crates/compiler-core/src/diagnostics.rs` | `DiagnosticSink::emit` logs at real severity |
| `ekos/crates/cli/src/commands/diagnostics_log.rs` | New shared helper; 3 tests |
| `ekos/crates/cli/src/commands/mod.rs` | Module registration |
| `ekos/crates/cli/src/commands/compile.rs` | Real warnings-log pointer |
| `ekos/crates/cli/src/commands/recover.rs` | Warnings now summarised and persisted (previously silent) |
| `TODO.md` | All six findings recorded |
| `devlogs/devlog_79.md` | This file |

# RFC 0076 — Real-Project Testing: Six Findings, Four Fixes

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Asked to test the current build against a real, external, non-Rust project
(`/home/legion/PycharmProjects/analytics` — the Plausible Analytics Elixir codebase, already a
real, multi-month-old EKOS case-study workspace from RFC 0056-0058) and generate documentation,
to find out what's working and what isn't. This is the first time this session tested against a
large real project with pre-existing ledger state, rather than a fresh disposable fixture — and it
surfaced six real findings, none of which any of this session's own fixture-based testing had
caught. This RFC covers the investigation and the four real fixes that came out of it; the other
two findings are documented as explicitly *not* fixed, with the real reason why.

## Finding 1 (fixed) — `sql_analyzer.rs` `Table`/`ForeignKey` objects had no deterministic id

**The bug**: every `Table` KirObject `sql_analyzer.rs` produces got `KirObject::new()`'s default
random id — the one analyzer in the entire `recovery` crate that didn't assign a deterministic id
(every sibling — `clickhouse_analyzer.rs`, `crate_topology_analyzer.rs`, `git_analyzer.rs`,
`github_analyzer.rs`, `cicd_analyzer.rs`, `dependency_analyzer.rs`, `local_docs_analyzer.rs`,
`confluence_analyzer.rs`, `document_semantics_analyzer.rs`, `crypto_analyzer.rs` — already does).
`ForeignKey` relationships had the same gap.

**Found live**: re-running `ekos recover` against the real, previously-committed analytics
workspace re-parsed the same unchanged DDL and minted a fresh random id for every table — verified
directly: `ekl "FIND Object WHERE kind = 'Table' AND name = 'public.users'"` returned **two** real,
distinct ids for the same real table. Every one of the workspace's 57 real tables existed exactly
twice (114 rows total, confirmed with zero exceptions). The exact failure class RFC 0072
root-caused for `crate_topology_analyzer.rs`'s `DependsOn` edges, now found at the object level,
in the single most heavily-depended-on recovery pass in the codebase.

**Fix**: `table_kir_id(name)` (deterministic `Uuid::new_v5`, lowercased to match
`parse_ddl_structural`'s own internal FK-lookup normalization, prefixed `sql-analyzer-table:` so
it can never collide with `clickhouse_analyzer.rs`'s own `table_kir_id`'s `clickhouse:` prefix —
two tables from two different systems merging is RFC 0029 cross-system identity's job, never an
accidental hash collision) and `foreign_key_kir_id(from, to, fk_desc)` (RFC 0072's own
counter-example applied directly: `(from, to)` alone isn't safe for `ForeignKey` because a table
can have two FK columns to the same target table — `fk_desc`, already computed by every caller, is
the real distinguishing signal).

**Live-verified**: moved the real, already-corrupted `.ekos` aside (non-destructively — preserved
at `.ekos.pre-rfc0076-fix-backup`, an append-only ledger has no delete/tombstone, so the existing
duplicates couldn't be retroactively cleaned even by this fix) and ran a completely fresh
`init → build → recover → resolve → compile → commit` with the fixed binary: **57 real tables, 57
real rows**, zero duplication. Ran the whole cycle a second time against unchanged DDL: still 57.
Regenerated `Architecture.md`'s Data Architecture section: every table listed exactly once.

## Finding 2 (fixed) — Elixir's `defp`/`defmodule` were invisible to the symbol fallback scan

**The bug**: `plugins/file/src/lib.rs`'s `DECL_PREFIXES` fallback (used for any language without a
real AST analyzer — RFC 0019) matched `"def "` but not `"defp "` or `"defmodule "` — a real Elixir
codebase measured directly: 1917 `defp` occurrences vs. 2509 `def`, and 522 `defmodule` (the
language's primary structural unit). All of it silently invisible; `API.md` initially looked far
sparser for this project than it actually should have been (a first read of just the file's head
gave the wrong impression this session — corrected by checking the full file before concluding
Elixir had *no* coverage at all, which was inaccurate; the real, narrower gap was these missing
prefixes).

**Fix**: added `defp `, `defmodule `, `defmacro `, `defmacrop `, `defdelegate ` to `DECL_PREFIXES`
— each a real Elixir declaration form with a real identifier immediately following it, so the
existing extraction logic (`take_while(is_alphanumeric)`) needs no changes. Zero cost for every
other language already covered (a Rust/Go/Python/TS file never contains a `defp` line).

**Live-verified**: `lib/plausible/auth/password.ex` (a real file, `defp hash`/`defmodule
Plausible.Auth.Password`) now shows both `Plausible` and `hash` in the regenerated `API.md`.

## Finding 3 (fixed) — `ekos doctor` false-negative for Ollama

**The bug**: the LLM-provider check defaulted its checked env var to `ANTHROPIC_API_KEY`
regardless of which provider was actually configured. With `provider = "ollama"` and Ollama
running correctly locally (confirmed: `curl localhost:11434/api/tags` returned real models), doctor
still reported `[FAIL] ollama configured but $ANTHROPIC_API_KEY is not set` — true, but irrelevant:
`OllamaProvider::from_env` reads `OLLAMA_BASE_URL`/`OLLAMA_MODEL`, both optional with sensible
defaults, no API key at all for a local server.

**Fix**: extracted the check into a small, pure, injectable-clock-free `llm_provider_check`
function (testable without mutating real process env vars, which would race across parallel test
threads) that special-cases `provider == "ollama"` as unconditionally OK.

**Live-verified**: `ekos doctor` on the real workspace now reports
`[OK] LLM provider ollama (local provider, no API key required)`.

## Finding 4 (fixed) — `ekos compile`'s "Warnings: N (check logs)" pointed nowhere real

**The bug**: `ekos compile` reported `Warnings: 28434 (check logs)` against the real analytics
workspace. Every one of those 28,434 warnings only ever logged at `tracing::debug!` — invisible at
this project's own configured default `log-level = "info"` (`ekos.toml`), and never persisted to
any file. "Check logs" pointed at nothing a user could actually go read. `ekos recover` had the
same underlying gap, worse: it didn't even print a warning *count*, silently dropping real per-pass
diagnostics (SQL001-003 and others) entirely.

**Fix**: two parts.
1. `DiagnosticSink::emit` now logs each diagnostic at the `tracing` level matching its own
   `Severity` (`error!`/`warn!`/`info!`) instead of always `debug!` — a warning is now visible at
   the log level a warning should be visible at.
2. New shared helper (`cli/src/commands/diagnostics_log.rs`): `write_diagnostics_log` persists
   every collected diagnostic to a real file, `.ekos/diagnostics/<command>.log`, overwritten each
   run (a stale file from an earlier run is actively removed on a clean re-run, so it never
   misleadingly implies an old problem still applies). Wired into both `compile` and `recover`,
   with the printed summary now naming the real file instead of a vague "(check logs)".

**Live-verified**: re-running against the real workspace, `ekos recover` printed
`Warnings: 2 (see /home/.../analytics/.ekos/diagnostics/recover.log)` — a real, genuinely useful
finding surfaced for the first time (`SQL003: LLM call failed ... model 'llama3.1:8b' not found` —
the configured model isn't actually pulled locally; only `qwen2.5:1.5b` is). `ekos compile` printed
`Warnings: 6758 (see .../compile.log)`, with the real content also visible live via `tracing::warn!`
during the run.

## Finding 5 (investigated, not a bug) — Low SQL transform coverage was honest, not broken

Recover reported "Transformation IR nodes (SQL): 5 total, 20% mapped." Inspected the 4 `Unmapped`
nodes directly: a real Postgres trigger function's control flow —
`BEGIN / IF EXISTS(...) THEN RAISE ... END IF / RETURN NEW / END`. The Transformation IR (RFC 0027)
is explicitly scoped to dataflow shapes (Source/Filter/Join/Aggregate/Calculate/Sink) — real
procedural control flow (`IF`/`RAISE`/`RETURN`) has no IR node type to map onto, and
`sql_transform_analyzer.rs` correctly, honestly reports it `Unmapped` with a real reason
(`"control flow present, not modeled"`) rather than fabricating a mapping. This is the system
working as designed for something genuinely out of its current scope — not fixed, because there is
nothing broken to fix. Modeling procedural control flow (`Branch`/`Loop`/`Exception` IR node types)
would be a real, substantial new RFC-worthy feature, not a bug fix, and out of scope here.

## Finding 6 (investigated, real fix deferred) — `resolve`'s 29.5M-pairwise-comparison cost

**Initial read**: `ekos resolve` against the real, pre-existing analytics workspace took ~5 minutes
— `Candidates evaluated: 10178`, `Pairs compared: 29557962`.

**Investigated**: `DefaultResolver::resolve` (`crates/identity/src/lib.rs`) already blocks
candidates by `(kind, first-3-normalized-name-chars)` before any pairwise scoring — this is not a
naive, unblocked O(n²) scan; the blocking itself is real and deliberate (RFC 0007). Checked
`github_analyzer.rs`'s object-naming convention as a hypothesis (every `Issue`/`PullRequest` name
is `"{owner}/{repo}#{number}: {title}"`, sharing an identical prefix across every item from one
repo, which could defeat the 3-char blocking key) — plausible, but not the dominant effect: a
completely fresh rebuild of the same workspace (same repo, same connectors) produced
`Candidates evaluated: 823`, `Pairs compared: 5241` — roughly 5,600× fewer pairs for a
structurally identical run. This strongly points at candidate-set *inflation* specific to a
long-lived, repeatedly-`recover`'d real workspace (this one has been actively used across multiple
real sessions since RFC 0056), not a resolver-algorithm defect — most likely accumulated
`KnowledgeArtifact`s from many past `ekos recover` invocations all still being read as current
input by `compile`'s `knowledge_artifact_ids`, though this session did not trace the artifact
store's exact lifecycle deeply enough to state that with full certainty.

**Not fixed**: a real fix here is either an artifact-store lifecycle change (only consider the
latest `KnowledgeArtifact` per pass, or actively prune superseded ones) or a blocking-key
improvement in the identity resolver — both are real, larger changes to core pipeline behavior with
a genuine risk of dropping evidence that should still count if the underlying hypothesis is wrong
in some case this session didn't test. Shipping a guessed fix to either would risk a correctness
regression more serious than the performance cost it would address. Recorded as a real, precisely
scoped-as-far-as-verified finding in TODO.md rather than either ignored or guess-fixed.

## Testing

- `sql_analyzer.rs`: 3 new tests (determinism across two independent parses of the same DDL,
  case-insensitive id matching, RFC 0072's two-FK-columns-to-one-table counter-example applied
  directly — distinct ids, not collapsed).
- `plugins/file/src/lib.rs`: 2 new tests (Elixir `defmodule`/`def`/`defp`/`defmacro` all
  recognized in one real-shaped fixture; a bare `def` line isn't mistaken for `defp`/`defmodule`).
- `doctor.rs`: 5 new tests against the extracted `llm_provider_check` (ollama unconditionally OK;
  Anthropic fails/passes on its key env var; a custom `api_key_env` name is respected; no provider
  configured is OK, not a failure).
- `diagnostics_log.rs`: 3 new tests (a real file is written with every diagnostic when any exist;
  nothing is written when there are none; a clean re-run removes a stale file from an earlier run).
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end**: the real analytics workspace, rebuilt fully fresh with the fixed
  binary, run twice independently — 57 real tables both times, zero duplication; `doctor` correctly
  OK for ollama; real, actionable warnings visible both live and in a real persisted file; Elixir
  `defp`/`defmodule` symbols present in the regenerated `API.md`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0076-real-project-test-fixes.md` | This RFC |
| `ekos/crates/recovery/src/sql_analyzer.rs` | `table_kir_id`, `foreign_key_kir_id`; wired into both construction sites; 3 new tests |
| `ekos/plugins/file/src/lib.rs` | `DECL_PREFIXES` extended with Elixir forms; 2 new tests |
| `ekos/crates/cli/src/commands/doctor.rs` | `llm_provider_check` extracted, ollama special-cased; 5 new tests |
| `ekos/crates/compiler-core/src/diagnostics.rs` | `DiagnosticSink::emit` logs at each diagnostic's own severity |
| `ekos/crates/cli/src/commands/diagnostics_log.rs` | New: `write_diagnostics_log`; 3 tests |
| `ekos/crates/cli/src/commands/mod.rs` | `pub mod diagnostics_log;` |
| `ekos/crates/cli/src/commands/compile.rs` | Warnings summary names a real log file |
| `ekos/crates/cli/src/commands/recover.rs` | Now prints and persists a real warnings summary (previously silent) |
| `TODO.md` | All six findings recorded; Finding 6 tracked as real, open, precisely-scoped work |
| `devlogs/devlog_79.md` | This session's devlog |

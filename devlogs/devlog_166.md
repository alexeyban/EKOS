# Devlog 166 — RFC 0138: `ekos eval` end-to-end agent/answer evaluation harness

**Date:** 2026-09-05
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct, per this repo's local-tests-only workflow)

---

## Summary

Shipped a new checked-in evaluation harness that grades whole `ekos ask` answers — not just
retrieval ranking (RFC 0126's separate, narrower, CI-gated, LLM-free harness) — against a real
scenario suite spanning architecture/code/lineage/security/adversarial categories. New
`ekos-evals` crate (two runners, six evaluators, report aggregation), a new `ekos eval run` CLI
subcommand, and `evals/` at the repo root holding the YAML datasets and generated reports.
Live-verified against this repo's own real ~5,500-object self-analysis ledger with the real local
Ollama provider already configured in `ekos.toml` — and it found a real, non-trivial answer-quality
gap in the small local model it ran against, which is exactly what it exists to catch.

---

## PR — RFC 0138: eval harness

### Problem / motivation

Every other compiled-knowledge surface in EKOS has a quality gate — RFC 0126 gates retrieval
ranking, RFC 0135 gates provenance/determinism, CI gates build/test/clippy/fmt. Nothing gated the
thing users actually experience: `ekos ask`'s answer quality. There was no repeatable way to tell
whether an LLM-provider swap, a prompt-version bump, or a retrieval change made real answers better
or worse, short of manually re-asking a few questions and eyeballing the response — and no way at
all to measure RFC 0043's "never fabricate" principle, which today is enforced only by prompt
wording, never checked.

### What was built

| Component | Role |
|---|---|
| `ekos/docs/rfcs/0138-eval-harness.md` | The design — explicitly scoped against RFC 0126 (§ relationship), non-goals section rules out an LLM-judge evaluator, CI wiring, and a fixed padded-to-100 dataset |
| `ekos-evals` crate (`ekos/crates/evals/`) | New workspace member: `schema` (Scenario/Dataset/Manifest YAML), `runners::{agent_runner, retrieval_runner}`, `evaluators::{answer, evidence, retrieval, completeness, groundedness, trajectory}`, `report` (aggregation + text/JSON rendering) |
| `crates/cli/src/commands/eval.rs` + `Commands::Eval` in `bin/ekos.rs` | `ekos eval run --dataset <name> [--agent claude\|ollama\|openai] [--category] [--limit] [--json] [--output]` |
| `AiAnswer.token_usage` (`crates/runtime/src/ai.rs`) | New `TokenUsage{input_tokens, output_tokens}` field, lifted from the existing `LlmResponse` — needed for the report's Avg tokens metric, previously discarded |
| `evals/` (repo root) | `datasets/{manifest,architecture,code,lineage,security,adversarial}.yaml` (32 real, hand-verified scenarios), `reports/` (generated JSON, gitignored), `README.md` |

### Implementation details worth remembering

- **Reuse over reimplementation.** The retrieval evaluator calls `ekos_runtime::retrieval_eval::
  recall_at_k` (RFC 0126) directly rather than reimplementing rank-metric math a second time — the
  two harnesses share the same textbook-verified function, just applied to different scenario
  sources.
- **Runners take an already-open `Runtime`/`AiRuntime`.** `ekos-evals` never opens a store or
  builds an `LlmProvider` itself — the CLI command does that (mirrors `ask.rs`'s own
  `open_store_read_only` + `build_llm_provider` + `AiRuntime::new` sequence exactly), so the crate
  stays decoupled from configuration/credentials and there's no `cli` ↔ `evals` circular
  dependency. `agent_runner::run` takes `&AiRuntime` *and* `&Runtime` as separate parameters rather
  than adding a `runtime()` getter to `AiRuntime`'s public surface just for this one caller.
- **Every score is `Option<f32>`/`Option<f64>`, `None` = not applicable.** A scenario with no
  `expected_facts` doesn't silently score 1.0 on answer-correctness — it's excluded from that
  metric's average entirely. This matters more than it sounds: an early version scored
  not-applicable as a pass, which would have let a dataset dominated by retrieval-only scenarios
  report a misleadingly high "Answer correctness."
- **The report's headline "Evidence groundedness" line is the `groundedness` evaluator, not the
  `evidence` evaluator** — deliberately reusing the same citation-validity ratio for normal
  scenarios but redefining it entirely for `should_refuse` ones (1.0 iff the answer actually
  declined *and* cited no fabricated evidence, 0.0 if it invented an answer). The raw `evidence`
  score still exists per-scenario (in `--json`) but isn't one of the five report-line metrics — the
  six evaluator modules the request specified and the five metrics in the worked report format are
  not a 1:1 mapping, and reconciling that was a real design decision, not an oversight.
- **A found-live formatting bug, fixed before shipping**: the first `row()` implementation padded
  the label to a fixed width, then computed the value's padding as `TOTAL_WIDTH - label.len()` —
  using the *original* (unpadded) label length against a field that was already padded to
  `TOTAL_WIDTH`, so the value column drifted per row instead of staying a straight edge. Two
  independent fixed-width fields (label field, value field) fixed it. Caught by an
  `eprintln!`-and-eyeball pass during development, not by any assertion — worth remembering
  because the bug produced *plausible-looking* output (nothing crashed, nothing was obviously
  wrong) until actually looking at more than one row.
- **Manifest naming, not hand-tuned to hit "100".** `--dataset` omitted loads every `*.yaml` in
  `datasets/` and names the run `ekos-<total scenario count>` — this is where the RFC's own worked
  example's `ekos-100` naming comes from conceptually, but the shipped dataset is 32 real scenarios
  (`ekos-32` today), not padded to a round number. Growing it is future incremental work per
  category.

### Live verification (not simulated)

Ran the full `ekos-full` dataset (32 scenarios) against this repo's own real, currently-compiled
`.ekos` workspace (~5,533 objects) with the real local `llama3:latest` Ollama provider already
configured in this repo's own `ekos.toml` — no mocking:

```
Scenarios:                   32        Answer correctness:       45.8%
Passed:                      17        Evidence groundedness:    77.8%
Failed:                      15        Completeness:             43.1%
                                        Recall@10:                 75.0%
                                        Hallucination rate:        12.5%
Status: FAIL                           Avg tokens: 649 · P95 latency: 25.6s
```

This is a genuine finding, not a bug in the harness: inspecting the saved JSON report scenario by
scenario, the harness correctly flagged 4 of 6 hallucination-check scenarios where the small local
model fabricated a plausible-sounding answer to a question about a nonexistent entity instead of
declining (`adv-002`, `adv-003`, `adv-005`, `sec-003`), while correctly passing the 3 where it
declined appropriately (`adv-001`, `adv-004`, `adv-006`). Raw answer-correctness against short,
specific structural facts (e.g. "what function parses SQL DDL structurally") was weak — `llama3`
often found the right evidence but summarized around the specific token rather than naming it. Full
workspace gate (`cargo build/test/clippy -D warnings/fmt --check`) clean, plus `tests/integration`
(5/5, unaffected by the `AiAnswer` field addition) and a `benchmark` build check.

---

## Knowledge Captured

- **A small local model's failure mode here wasn't "makes things up wildly" — it was "declines
  correctly most of the time, fabricates specifically on nonexistent-entity summarization asks."**
  Useful to know before trusting `llama3:latest` for anything beyond structured lookup: the
  adversarial dataset's phrasing style ("Summarize the X module", "What version does Y ship at")
  reliably triggered fabrication more than direct existence questions did.
- **`Ledger::open` (SQLite-backed `ekos-ledger`) is the right test-double store for pure evaluator
  unit tests** — no fixture file, no path wrangling, `tempfile::tempdir()` + `Ledger::open(&path)`
  gives a real `KnowledgeStore` impl in a few lines, same pattern RFC 0126's own tests use.
- **`ObjectKind` has no generic `Module` variant** — real code-symbol objects (`RustSymbol`,
  `PythonSymbol`, etc.) are `ObjectKind::Custom("RustSymbol")`, per the `custom_kinds` registry
  (RFC 0135 Part D). Worth remembering before writing a test fixture object.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0138-eval-harness.md` | New RFC |
| `ekos/crates/evals/**` | New crate: schema, runners, evaluators, report |
| `ekos/crates/cli/src/commands/eval.rs` | New CLI command |
| `ekos/crates/cli/src/commands/mod.rs` | Register `eval` module |
| `ekos/crates/cli/src/bin/ekos.rs` | `Commands::Eval`/`EvalCommands::Run`, dispatch, `emits_machine_output` |
| `ekos/crates/cli/Cargo.toml`, `ekos/Cargo.toml` | New workspace member + dependency |
| `ekos/crates/runtime/src/ai.rs` | `AiAnswer.token_usage: TokenUsage` (new field, 3 construction sites updated) |
| `evals/README.md`, `evals/datasets/*.yaml`, `evals/reports/.gitkeep` | New data directory |
| `.gitignore` | Ignore generated `evals/reports/*.json` |
| `README.md` | New "Eval harness" section |
| `docs/generated/ekos-self-documentation.html` | New §12 "Eval harness" section + nav link, renumbered §12-15 → §13-16 |

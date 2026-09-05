# RFC 0138 — `ekos eval`: end-to-end agent/answer evaluation harness

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-09-05
**Relationship to RFC 0126:** complementary, not overlapping. RFC 0126's `ekos_runtime::retrieval_eval`
is a narrow, offline, LLM-free, CI-gated regression check over a fixed 55-object synthetic estate —
"did retrieval ranking quality regress" only. This RFC is a broader, LLM-in-the-loop harness that
grades whole answers (correctness, groundedness, completeness, hallucination) across real category
datasets against a real workspace ledger, exposed as a user-facing `ekos eval` command with a
human-readable report. It **reuses** RFC 0126's pure rank-metric functions
(`recall_at_k`/`reciprocal_rank`/`ndcg_at_k`) rather than reimplementing them — see §2.3.

---

## Motivation

Every other compiled-knowledge surface in EKOS has a quality gate: RFC 0126 gates retrieval
ranking, RFC 0135 gates provenance/determinism, CI gates build/test/clippy/fmt. Nothing gates the
thing users actually experience — `ekos ask`'s answer quality: does it state real facts, cite real
(non-hallucinated) evidence, cover what was asked, and correctly refuse when a question has no
grounded answer (RFC 0043's "never fabricate" principle, currently enforced only by prompt wording,
never measured). There is no repeatable way to answer "did this LLM-provider swap, prompt-version
bump, or retrieval change make answers better or worse" other than manually re-asking a few
questions and eyeballing the response.

This RFC ships a checked-in, growable scenario suite (`evals/datasets/*.yaml`, seeded with real
questions verified against this repo's own live ~5,500-object self-analysis ledger — never
fabricated fixture text), a new `ekos-evals` crate implementing two runners and six evaluators, and
`ekos eval run` producing the report format below.

```
EKOS EVALUATION
─────────────────────────────

Dataset: ekos-45
Agent: ollama (llama3:latest)
Runtime: local

Scenarios:               45
Passed:                  42
Failed:                   3

Answer correctness:    91.1%
Evidence groundedness: 96.4%
Completeness:           88.7%
Recall@10:              92.0%
Hallucination rate:      2.2%

Avg tokens:            1,842
P95 latency:            3.9s

Status: PASS
```

---

## Design

### 1. Scenario data — `evals/` (repo root, sibling to `ekos/`)

```
evals/
├── README.md
├── datasets/
│   ├── manifest.yaml       # named dataset -> list of category files
│   ├── architecture.yaml
│   ├── code.yaml
│   ├── lineage.yaml
│   ├── security.yaml
│   └── adversarial.yaml
└── reports/                 # ekos eval run --output lands here; gitignored except .gitkeep
```

Consistent with the existing precedent that `benchmark/` and `tests/integration/` are separate
top-level trees from `ekos/` (CLAUDE.md): `evals/` holds **data and generated output**, not code.
The "runners/" and "evaluators/" folders in the shape this RFC was requested in map onto real Rust
modules inside the new `ekos-evals` crate (`crates/evals/src/runners/*.rs`,
`crates/evals/src/evaluators/*.rs`) — a real workspace member like every other crate, not a second
ad-hoc script tree, so `ekos eval` is a first-class subcommand of the existing `ekos` binary (the
way the user asked for it) rather than a separate executable.

**Scenario schema** (one YAML file per category, `version: 1`):

```yaml
version: 1
category: architecture
scenarios:
  - id: arch-001
    question: "What crate implements the SQL DDL recovery analyzer?"
    mode: reason                  # reason (default) | ask | retrieval
    expected_facts:
      - "sql_analyzer"
      - "recovery"
    expected_evidence_contains:
      - "recovery/src/sql_analyzer.rs"
    expected_objects:              # graded via retrieval recall@k, reused from RFC 0126
      - "sql_analyzer::SqlAnalyzerPass"
    expected_query_type: lexical   # optional trajectory check against the REASON planner
    difficulty: easy
    pass_threshold: 0.7            # default 0.7 if omitted
  - id: adv-003
    question: "What does the FooBarNonexistentAnalyzer do?"
    mode: reason
    adversarial: true
    should_refuse: true            # answer must decline, not fabricate
```

`manifest.yaml` maps a `--dataset <name>` to a list of files. **If `--dataset` is omitted**, the
runner loads every `*.yaml` in `datasets/` except `manifest.yaml` and names the run `ekos-<n>`
where `n` is the total scenario count — this is where the `ekos-100` naming in the worked example
above comes from: it is not a fixed magic name, it is "however many real scenarios exist today."
The suite ships smaller (real, not padded — see Non-goals) and is meant to grow over time as more
scenarios get added per category.

### 2. `ekos-evals` crate (`ekos/crates/evals`)

#### 2.1 `schema.rs`
`Scenario`, `Dataset`, `Manifest` — `serde`+`serde_yaml` deserialization, `Mode` enum
(`Reason`/`Ask`/`Retrieval`), `load_dataset(name, datasets_dir) -> (String, Vec<Scenario>)`.

#### 2.2 `runners/`
- **`agent_runner`** — for `mode: reason`/`ask` scenarios. Takes an already-constructed
  `&AiRuntime`, calls `ai.reason(question)` or `ai.ask(question)`, and (for trajectory grading)
  `ai.plan(question)` — all pre-existing `AiRuntime` methods, no changes to the ask pipeline
  itself. Times the call (`Instant`), captures `AiAnswer` (`answer`, `evidence_refs`,
  `token_usage` — new field, §2.4) into a `ScenarioRun`.
- **`retrieval_runner`** — for `mode: retrieval` scenarios (and reused internally by `agent_runner`
  when a scenario has `expected_objects`, so recall@k can be graded even on an LLM-answered
  scenario). Calls `Runtime::retrieve` with `RetrievalRequest::lexical`, no LLM involved.

Both runners are pure `fn`s over already-open `Runtime`/`AiRuntime` — the CLI command owns opening
the store and building the `LlmProvider` (`build_llm_provider`, already in `cli::commands::recover`,
unchanged), exactly like `ask.rs` does. `ekos-evals` never opens a store itself; this avoids a
`cli` ↔ `evals` circular dependency and matches how `docs-gen`/`marketing` are already wired.

#### 2.3 `evaluators/` — pure functions, `EvalOutcome { score: Option<f32>, .. }`, `None` = not
applicable to this scenario (excluded from that metric's average, never silently scored as 1.0)

| Module | What it grades | Reuses |
|---|---|---|
| `answer` | fraction of `expected_facts` present (case-insensitive substring) in the answer text | — |
| `evidence` | fraction of `evidence_refs` that resolve to a real `KnowledgeStore::get_evidence` entry (non-hallucinated citation) | — |
| `retrieval` | recall@10 of `expected_objects` (names → ids via `Runtime::list_objects`, same resolution as RFC 0126's `evaluate()`) | `ekos_runtime::retrieval_eval::recall_at_k` |
| `completeness` | `(matched_facts + matched_evidence_contains) / (total_facts + total_evidence_contains)` | `answer`'s and `evidence`'s match sets |
| `groundedness` | for normal scenarios: same ratio as `evidence`, reframed as a coverage score; for `should_refuse` scenarios: 1.0 iff the answer contains a refusal signal (builtin phrase list + optional `refusal_phrases` override) **and** cites no evidence, else 0.0 (a fabricated answer to an unanswerable question) | `evidence`'s citation-validity check |
| `trajectory` | for scenarios with `expected_query_type` set: does `ai.plan(question).query_type` (RFC 0123's rules planner) match? Offline, no LLM. `None` when unset — an honest v1 limit: this is the only trajectory signal available without deeper per-call instrumentation of the REASON pipeline's internal retrieve→expand→ground steps | `ekos_runtime::reason::plan_question` |

A scenario's **hallucination flag** (feeds the report's Hallucination rate) is `true` iff `evidence`
found an invalid citation, or a `should_refuse` scenario got a non-refusing, evidence-free (or
falsely-evidenced) answer. A scenario **passes** iff its composite score (mean of applicable
evaluator scores) meets `pass_threshold` (default `0.7`) **and** it is not flagged as a
hallucination.

#### 2.4 `AiAnswer` gains `token_usage`

`crates/runtime/src/ai.rs`: new `pub struct TokenUsage { pub input_tokens: u32, pub output_tokens:
u32 }`, `AiAnswer.token_usage: TokenUsage`, populated from the existing `LlmResponse.{input_tokens,
output_tokens}` at all three construction sites (`ask_with_history`, `reason_with_history`,
`ask_stream_with_history`). Purely additive — `AiAnswer` has exactly one non-test construction path
(all three inside `ai.rs` itself, confirmed by grep), so no other call site breaks.

#### 2.5 `report.rs`

`Report { dataset, agent, runtime: "local", scenarios: Vec<EvalOutcome>, gates: GateThresholds }`.
`aggregate()` computes the seven headline metrics as means over *applicable* scenarios only, `p95`
latency over all timed scenarios, and `Status` from `GateThresholds` (defaults: answer ≥ 0.85,
groundedness ≥ 0.90, completeness ≥ 0.80, recall@10 ≥ 0.80, hallucination ≤ 0.05 — **not** "zero
scenario failures"; a suite can have individual failures and still clear the aggregate bar, matching
the worked example's 3-failed-but-PASS). `render_text()` produces the exact box format above;
`Report` is also `Serialize` for `--json` / the saved `evals/reports/<timestamp>-<dataset>.json`
file every run writes (mirrors `ekos ledger audit`'s save-a-JSON-artifact pattern).

### 3. CLI — `ekos eval run`

```
ekos eval run [--dataset <name>] [--datasets-dir evals/datasets] [--category <cat>]
              [--agent claude|ollama|openai] [--limit N] [--json] [--output <file>]
```

`--agent` overrides `config.llm.provider` in an in-memory `EkosConfig` clone for this run only
(`claude` maps to the unset/default branch of `build_llm_provider`, i.e. Anthropic) — lets you
A/B two providers without editing `ekos.toml`. Wired into `Commands::Eval { subcommand: EvalCommands
}` in `crates/cli/src/bin/ekos.rs`, dispatching to `commands::eval::run`, following the exact
pattern every other subcommand uses (`&config, &cwd`). `--json` is added to `emits_machine_output`.

---

## Non-goals

- **Not an LLM-judge evaluator.** Every score above is deterministic string/id matching, consistent
  with EKOS's existing preference for reproducible passes over non-deterministic grading (RFC
  0009/0123's own "answer only from evidence" ethos). A future `judge` evaluator scored by a second
  LLM call is a real, separately-scoped follow-on if keyword matching proves too coarse — not
  attempted here.
- **Not padded to a fixed 100-scenario count.** The shipped datasets are real, hand-verified
  questions against this repo's own live ledger (~8-10 per category). Growing toward "100" is
  future incremental work per category, not manufactured now to match the illustrative example.
- **No CI gate wired for this harness.** Unlike RFC 0126 (offline, seconds, no credentials), this
  harness makes real LLM calls against a real workspace and is not free/fast/deterministic enough
  to run on every PR without further design (cost, flakiness, which provider). `ekos eval run` is a
  manual/on-demand command in this RFC; CI wiring is a named future item, not attempted.
- **No multi-agent / tool-call trajectory tracing.** `ekos ask`'s pipeline is a fixed
  retrieve→expand→ground→ask sequence, not an agentic tool loop — `trajectory` grades the one real
  offline signal available (planner routing), not a full step-by-step trace.

---

## Verification

- Unit tests per evaluator module (pure functions — no LLM, no store) covering match/no-match/`None`
  (not-applicable) cases, and `report::aggregate`/`render_text` against hand-computed fixtures.
- `retrieval` evaluator tests reuse a small seeded `FactLedger` (same shape as RFC 0126's
  `seed_reference_estate`) to confirm real name→id resolution + `recall_at_k` reuse, not a mock.
- `agent_runner` tested against `MockLlmProvider` (deterministic canned response) for wiring
  correctness (latency/token capture, citation extraction reuse) without a live LLM dependency in
  the default `cargo test --workspace` run.
- **Live-verified**: `ekos eval run` against this repo's own real `.ekos` workspace
  (~5,533 objects) with the real local Ollama provider (`llama3:latest`, already configured in this
  repo's `ekos.toml`) — real report, real pass/fail counts, not simulated.

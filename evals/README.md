# EKOS Evals

RFC 0138's checked-in scenario datasets + generated reports for `ekos eval run`. See
`ekos/docs/rfcs/0138-eval-harness.md` for the full design; this file is the quick-start.

## Layout

```
evals/
├── datasets/
│   ├── manifest.yaml       # named dataset -> list of category files (e.g. "ekos-full")
│   ├── architecture.yaml   # A — what EKOS is and how it's structured (20 scenarios)
│   ├── code.yaml           # B — where a symbol/function/crate lives
│   ├── dependencies.yaml   # C — what depends on X / what breaks if X changes
│   ├── lineage.yaml        # D — evidence/fact provenance (not the dependency graph — see C)
│   ├── history.yaml        # E — RFC/devlog provenance ("why was X built")
│   ├── security.yaml       # F — RFC 0043 redaction posture
│   └── adversarial.yaml    # G — hallucination resistance (every scenario should_refuse: true)
└── reports/                 # ekos eval run's saved JSON reports (gitignored except .gitkeep)
```

The runner logic itself — the two runners (`agent_runner`, `retrieval_runner`) and six evaluators
(`answer`, `evidence`, `retrieval`, `completeness`, `groundedness`, `trajectory`) — lives in the
`ekos-evals` Rust crate (`ekos/crates/evals/src/{runners,evaluators}/`), a real workspace member
like every other crate, so `ekos eval` is a first-class subcommand of the `ekos` binary rather than
a separate script tree. This directory holds only data (scenarios in, reports out).

## Category G — hallucination resistance is the point, not an afterthought

Every scenario in `adversarial.yaml` asks about something that genuinely does not exist, or asserts
a false premise about something that does (a password nothing stores, a write path the read-only
Runtime doesn't have). The only correct answer is a refusal — **"Insufficient evidence"** is the
canonical phrasing this suite expects (`evaluators::groundedness`'s builtin refusal-phrase list),
though any of the listed equivalent phrasings ("cannot find", "no such", "does not exist", …) also
passes. An answer that invents a plausible-sounding detail instead — a database name, a version
number, a person's name — is graded as a hallucination and lowers the report's Hallucination rate,
regardless of how fluent or confident the fabricated answer reads. This is deliberately the
harness's strictest category: a model can score well elsewhere and still fail here if it doesn't
know how to say "I don't know."

## Running it

```bash
# From the repo root (evals/ is a sibling of ekos/), against an already-built workspace:
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --dataset ekos-full

# One category only
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --dataset architecture
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --dataset hallucination  # Category G only

# Omit --dataset entirely to run every *.yaml here, named "ekos-<total scenario count>"
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run

# A/B a different provider than ekos.toml's configured one for just this run
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --agent ollama

# See every past run as a trend table (newest last)
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval history
```

Every run writes a timestamped JSON report to `evals/reports/` (unless `--output` is given) and
exits non-zero when the aggregate gate (`ekos_evals::report::GateThresholds`) fails — usable as a
manual/CI-adjacent quality check, though **this harness is not wired into CI** in this RFC (it
makes real LLM calls against a real workspace; see the RFC's Non-goals for why). `ekos eval
history` reads every saved report back and renders them as one line per run — that's the "run
history" a saved-JSON-per-run design gives you for free, no separate database.

### What each report measures

Beyond the five headline scores (Answer correctness, Evidence groundedness, Completeness,
Recall@10, Hallucination rate), every run also captures, per scenario and aggregated:

- **Latency** — wall-clock time per scenario, P95 across the run.
- **Tokens spent** — `input_tokens + output_tokens` per LLM-answered scenario (`AiAnswer.
  token_usage`, lifted straight from the provider's own billed response).
- **Tokens saved** — the token cost of any scenario whose answer was served from
  `CachedLlmProvider`'s disk cache instead of a fresh network call (`LlmProvider::cache_stats`,
  diffed before/after each call to attribute a hit to the specific scenario that got it).
- **CPU time / peak RSS** — best-effort, Linux-only (`ekos_evals::resource`, reads `/proc/self/
  {stat,status}` directly, no new dependency). `None`/`n/a` off-Linux or if unavailable — a
  diagnostic metric, never gates pass/fail, and never fabricated when it can't be read.

## Writing a scenario

One YAML file per category, `version: 1`. Every scenario needs `id` and `question`; everything
else is optional grading metadata — see the RFC §1 for the full field contract. A scenario with
none of `expected_facts`/`expected_evidence_contains`/`expected_objects`/`expected_query_type`/
`should_refuse` set is not gradable and always contributes a neutral pass — write at least one of
these for a scenario to mean anything.

Every scenario shipped here was verified against this repo's own real `.ekos` workspace before
being written down (`ekos query find`/`ekos ask --explain`) — not invented text. When adding a
scenario, do the same: check the fact is real, and that the REASON planner actually classifies
your phrasing the way you expect before setting `expected_query_type` (`ekos ask <question>
--explain`). **One more check specific to this repo**: the compiled self-analysis ledger this
suite runs against can genuinely lag the git history (confirmed live, 2026-09-05 — RFCs only up to
0095 and devlogs only up to 111 were actually compiled in at the time this dataset was written,
despite the real repo being much further along) — `ekos query find "<exact name>"` returning zero
hits for something you know exists in git means it hasn't been through `ekos build/recover/
resolve/compile/commit` yet, not that the fact is wrong. Ground new scenarios in what the ledger
actually has, or refresh the workspace first.

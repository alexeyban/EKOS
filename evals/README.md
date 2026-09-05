# EKOS Evals

RFC 0138's checked-in scenario datasets + generated reports for `ekos eval run`. See
`ekos/docs/rfcs/0138-eval-harness.md` for the full design; this file is the quick-start.

## Layout

```
evals/
├── datasets/
│   ├── manifest.yaml       # named dataset -> list of category files (e.g. "ekos-full")
│   ├── architecture.yaml
│   ├── code.yaml
│   ├── lineage.yaml
│   ├── security.yaml
│   └── adversarial.yaml
└── reports/                 # ekos eval run's saved JSON reports (gitignored except .gitkeep)
```

The runner logic itself — the two runners (`agent_runner`, `retrieval_runner`) and six evaluators
(`answer`, `evidence`, `retrieval`, `completeness`, `groundedness`, `trajectory`) — lives in the
`ekos-evals` Rust crate (`ekos/crates/evals/src/{runners,evaluators}/`), a real workspace member
like every other crate, so `ekos eval` is a first-class subcommand of the `ekos` binary rather than
a separate script tree. This directory holds only data (scenarios in, reports out).

## Running it

```bash
# From the repo root (evals/ is a sibling of ekos/), against an already-built workspace:
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --dataset ekos-full

# One category only
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --dataset architecture

# Omit --dataset entirely to run every *.yaml here, named "ekos-<total scenario count>"
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run

# A/B a different provider than ekos.toml's configured one for just this run
cargo run --manifest-path ekos/Cargo.toml -p ekos -- eval run --agent ollama
```

Every run writes a timestamped JSON report to `evals/reports/` (unless `--output` is given) and
exits non-zero when the aggregate gate (`ekos_evals::report::GateThresholds`) fails — usable as a
manual/CI-adjacent quality check, though **this harness is not wired into CI** in this RFC (it
makes real LLM calls against a real workspace; see the RFC's Non-goals for why).

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
--explain`).

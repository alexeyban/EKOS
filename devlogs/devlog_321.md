# Devlog 321 — BIRD-Test submission: EKOS K2 on BIRD dev (65.84%), v1.3.0, public submission repo (RFC 0244)

**Date:** 2026-11-05
**PRs:** — (worked on main; commits 93d67a1, aade3a9, 7a98460, 4c21dd6, f633a83; tag v1.3.0)
**Branch:** main

---

## Summary

The user asked for EKOS to be verified on BIRD's overall leaderboard. That means a BIRD-Test
submission: BIRD runs the system on its own machines against the hidden test set, following its
submission guidelines.

- **Decisions:** EKOS K2 + Claude Haiku 5.5 with evidence; ≤ $10; the Zen key, never in the
  email; a new public repo.
- **EKOS changes (RFC 0244):**
  - a `bird-dev` suite;
  - predict-only questions;
  - checkpoint and resume;
  - deadlines on profile queries for giant databases.
- **Dev result:** 65.84% EX on BIRD New Dev (1,534 questions), for $2.82 billed.
- **Release:** v1.3.0, published once the release workflow was moved off the retired `macos-14`
  runner.
- **Submission repo:** https://github.com/alexeyban/ekos-text2sql-bird, tested end to end with its
  Docker image.
- **Email:** drafted for the user to send. It contains no key.

---

## RFC 0244 — BIRD-Test submission

### What was built
| Component | Where |
|---|---|
| `Source.inner`: a zip inside the downloaded zip (`dev.zip` → `dev_databases.zip`) | `evals/src/bench/{suite,bird}.rs` (`unpack_inner`) |
| Suite `bird-dev` (New Dev, Hugging Face revision pinned) and cell `k2-dev` (K2 with evidence) | `evals/bench/bird-dev/` |
| A question with an empty gold SQL is predicted, left unscored, and the report says "Predictions only" | `bench/mod.rs::run_from`, `report.rs::predictions_only` |
| Checkpoint `<stamp>-<cell>-<split>.partial.jsonl` and `--resume` (refuses another cell's checkpoint; a torn line is skipped and the next result starts a new line) | `cli/src/commands/eval_bench.rs` |
| A 60-second deadline per profile and value-index query; on timeout the column is left out and logged | `bench/context.rs::bounded` |
| The submission repo: `run.sh`, Dockerfile (v1.3.0 musl, SHA-256), `cells/k2.toml` and `k2-anthropic.toml` ($10 caps), `dev/` | github.com/alexeyban/ekos-text2sql-bird |

### Results
See RFC 0244 §7. In brief:

- **EX 65.84%**, CI [63.6, 68.3]: simple 75.8, moderate 61.4, challenging 37.2.
- **Where it sits among New Dev entries:** between GLM-4.7 (63.8) and Claude 4.5 Sonnet (67.3).
- **Volume:** 15.5k input tokens per question, 2.22 calls.
- **Test estimate:** about $3.8 and 3.5 hours.

---

## Knowledge Captured

- **BIRD's test is run by BIRD.** You send code, the dev predictions and a token estimate. They
  run it with your API key and allow up to 3 fix rounds, with 1–2 submissions per 2 months.
  Resume-on-failure is required in practice: they won't pay for restarts.
- **BIRD's current dev is "New Dev"** (Hugging Face `birdsql/bird_sql_dev_20251106`). The databases
  are still the 2024 `dev.zip`, which nests `dev_databases.zip` inside it.
- **The `macos-14` GitHub runner label stopped being served (2026-11-05).** Jobs on it are cancelled
  1–2 seconds after start, with no runner, and the logs are empty. A cancelled build blocks the
  release's publish step. The fix was `macos-15`. Signature: `runner_name` empty, and
  completed_at − started_at ≈ 1 s.
- **`gh run watch` polls every few seconds.** Several of them at once exhausted the 5,000/hour API
  quota. Use a `sleep 60` loop on `gh run view --json status`.
- **The new SonarCloud CI step caught a failing integration test the local gate missed:** only
  `--lib` had been run. Run `cargo test -p ekos --tests` (or the whole workspace) before pushing.
- **Sonar `rust:S7493` flags `std::fs` directly inside an async fn.** It fails the reliability
  gate. Use `tokio::fs` there.
- **A crash can tear the last line of a JSONL checkpoint.** Appending after it glues the next
  record onto the fragment, so start a new line when the file does not end in `\n`. A test caught
  this.
- **Auto mode blocks pushing a release tag.** The user runs it with `!` (no leading space, on one
  line).
- **`wrong_columns` is 11% of BIRD dev answers.** The agent adds identifying columns that the
  terse gold SQL omits. That is the next lever.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0244-bird-test-submission.md` | New RFC with results |
| `ekos/crates/evals/src/bench/{bird,context,mod,report,suite}.rs`, `tests/bench_fixture.rs` | Nested zip, predict-only, run_from, deadlines, predictions-only render |
| `ekos/crates/cli/src/{app.rs,commands/eval_bench.rs}`, `crates/cli/tests/bench.rs` | `--resume`, checkpoints; test reads reports only |
| `evals/bench/bird-dev/` | New suite and cell |
| `.github/workflows/{release,ci}.yml` | macos-14 → macos-15 |
| `ekos/Cargo.toml`, lockfiles, `CHANGELOG.md` | v1.3.0 |

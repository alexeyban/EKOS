# RFC 0244 — Submitting EKOS to the BIRD-Test leaderboard

**Status:** Accepted
**Created:** 2026-11-05
**Builds on:** RFC 0240 (benchmark adapters), RFC 0241 (text-to-SQL quality), RFC 0242 (agent)

## 1. Goal

A verified entry on BIRD's overall leaderboard. BIRD scores the hidden test set (1,789 questions)
by running a submitted system on its own machines, following the "BIRD-Test Submission
Guidelines". The entry is **EKOS K2 + Claude Haiku 5.5 with evidence** (the Oracle Knowledge
setting), track 3/4: evaluation through an API, with EKOS's pipeline around the model.

**Decisions (user, 2026-11-05):**
- **Model:** Haiku 5.5.
- **Spend:** ≤ $10 metered on our side.
- **Key:** the OpenCode Zen key, rotated after BIRD's evaluation.
- **Evidence:** on.
- **Repo:** a new public repository, `alexeyban/ekos-text2sql-bird`.

## 2. What BIRD requires, and how each is met

| Requirement | How |
|---|---|
| Code that ran successfully on the **dev** set | Suite `bird-dev`: BIRD's current dev set ("New Dev", 2025-11-06, 1,534 questions, Hugging Face revision pinned) on the dev databases (`dev.zip`, pinned). Full run, scored with the RFC 0240 scorer, whose parity with the official evaluator is established |
| Dev predictions | `ekos eval bench export --target bird-submission`, included in the repository |
| Prompt-token count on dev, for their cost estimate | The run report's token totals, stated in the README |
| Logging, and restart from the failing question | Every answered question is appended to a checkpoint file (`<report>.partial.jsonl`); `--resume <file>` skips the answered questions. Every model call is already in the run log and `llm-usage.jsonl` |
| No code that could upload their databases | Only one network destination: the model API (OpenCode Zen, Anthropic's Claude Haiku 5.5). No telemetry and no other client. The model sees the schema, value samples and probe results, as every API-based entry's model does. The EKOS binary is a pinned, checksummed public release |
| README with commands, code zip | The repository: README, Dockerfile, `run.sh`, cell configuration |
| Their keys or ours | Ours (Zen), via the `OPENCODE_API_KEY` environment variable |
| `column_meaning.json` used? | No: descriptions come from `database_description/*.csv`, which the test databases carry like the dev ones |
| Test input = `test.json` (dev format, `SQL` empty) + `test_databases/` | A local suite (`suite.toml` pointing at their two paths). A question with an empty `SQL` is predicted, not scored |

## 3. Changes in EKOS

1. **Nested archives.** `Source.inner` names a zip inside the downloaded zip (`dev.zip` holds
   `dev_20240627/dev_databases.zip`). It is extracted to a temporary file and read like any
   archive, with the same allowlist.
2. **Predict-only questions.** A question whose gold SQL is empty is answered and left
   `unscored`; the report says how many.
3. **Checkpoint and resume.** `eval bench run` appends each finished question's result, as one
   JSON line, to `<reports-dir>/<stamp>-<cell>-<split>.partial.jsonl`. `--resume <partial>`
   loads it, checks the cell hash, skips those ids, and writes one complete report at the end.
4. **Giant databases.**
   - Every profile and value-index query runs under a deadline (60 s by default).
   - A column whose statistics time out is profiled without them: the column stays in the pack,
     and the timeout is logged.
   - The table row count is taken once.

## 4. Runs (≤ $10, Haiku 5.5 via Zen)

| Run | Questions | Estimate |
|---|---|---|
| Smoke run, cell K2-dev (K2 with evidence) | 10 | ~$0.03 |
| Full dev, K2-dev | 1,534 | ~$4 |

- Reported:
  - EX overall and by difficulty, Soft-F1 and R-VES;
  - prompt tokens, in total and per question;
  - cost.
- **Disclosure:** K2's design was tuned on Mini-Dev, whose questions were derived from BIRD dev,
  so a dev score is optimistic. The test score is the real one.

## 5. The public repository

`alexeyban/ekos-text2sql-bird` contains:

- **README:**
  - what it is;
  - exact commands for Docker and for bare Linux;
  - expected time and tokens;
  - the dev result;
  - the data-handling statement.
- **Dockerfile:** Debian slim with the EKOS release binary, pinned by SHA-256. No GPU.
- **`run.sh`:**
  - `prepare` (one EKOS project per database);
  - `run` with checkpoints;
  - `export` to `predict_test.json` (or `predict_dev.json`);
  - logs to `logs/`.
- **`cells/k2-dev.toml`.**
- **`dev/`:** dev predictions, the dev report and the token counts.
- **LICENSE.**

It contains no BIRD data. The submission email (to `bird.bench23@gmail.com`) is drafted for the
user to send; the key goes only in that email.

## 6. Verification

- Unit tests:
  - nested-zip extraction;
  - an empty-gold question is predicted and unscored;
  - resume skips answered ids and refuses a different cell;
  - a profile query past its deadline leaves the column without statistics.
- The repository's `run.sh` runs end to end on a 10-question local suite built from dev files laid
  out like BIRD's test input, from a clean Docker build.

## 7. Results (2026-11-05)

### 7.1 Dev: BIRD New Dev 2025-11-06, all 1,534 questions, cell K2-dev, Claude Haiku 5.5 via Zen

| | |
|---|---|
| **EX** | **65.84%**, 95% CI [63.56, 68.25] |
| Soft-F1 | 72.09 |
| R-VES | 60.67 |
| By difficulty | simple 75.8 (860), moderate 61.4 (443), challenging 37.2 (231) |
| Outcomes | correct 1,010, wrong_values 220, wrong_columns 174, wrong_rows 123, exec_error 2, no_sql 1, unscored 4 |
| Calls | 3,409 (2.22 per question) |
| Tokens | 23.8M input and 1.75M output (15.5k and 1.1k per question) |
| Cost | $3.24 at list price ($2.82 billed, after the provider's prompt-cache discount) |
| Wall time | 3.0 h |

- **Weak databases:** financial 28.3 and california_schools 39.3. The best are superhero 81.4 and
  debit_card_specializing 81.2.
- **The `unscored` 4:** gold SQL past the 30-second limit when scored.

**Where it sits.** The leaderboard's New Dev entries include:

| Entry | New Dev EX |
|---|---|
| Claude Opus 4.6 (baseline) | 68.77 |
| Claude 4.5 Sonnet (baseline) | 67.34 |
| GLM-4.7 | 63.82 |
| DeepSeek-R1 | 61.67 |
| Kimi-K2-Thinking | 60.63 |

EKOS K2 on Haiku 5.5 (65.84) is between GLM-4.7 and Claude 4.5 Sonnet, at about $0.002 a question.
The ranked systems at the top reach 72–78 on dev.

**The largest single failure class is `wrong_columns` (11%).** BIRD dev's gold SQL usually returns
exactly the asked column. The agent's verify step asks for "exactly the columns asked", yet it
still adds identifying columns. That is the next lever. It is not tuned here, because dev is the
reported number.

### 7.2 The submission path

- **v1.3.0** was released on 2026-11-05. Before it could publish, the release workflow's
  aarch64-apple-darwin build had to move from `macos-14`, which was no longer served, to
  `macos-15`.
- **Repository:** https://github.com/alexeyban/ekos-text2sql-bird. Its Docker image pins the
  v1.3.0 static binary by SHA-256.
- **Tested end to end** with that image, on 10 dev questions laid out like BIRD's test input:
  prepare, run, export, `predict.json` in BIRD's format.
- **Resume** was tested with `run.sh`: a second invocation made no model calls.

### 7.3 Spend

$2.85 of the approved $10:

- the dev run;
- a 10-question smoke run;
- two 10-question `run.sh` tests;
- the Docker test.

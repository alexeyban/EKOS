# RFC 0240 — Benchmark adapters for `ekos eval`, and the BIRD adapter

**Status:** Accepted — implemented 2026-11-03 (see §7 for what differs from the draft)
**Author:** EKOS team
**Created:** 2026-11-03
**Builds on:** RFC 0138 (eval harness), RFC 0139 (answer quality, the fair ruler), RFC 0177, RFC 0197
(model routing, cost, bake-off), RFC 0222 (a project-specific eval), RFC 0178 (EKOS home and
projects), RFC 0236–0239
**Plan:** `bench-program-plan.md`

## 1. Problem

RFC 0138's harness is built for EKOS's own questions: YAML scenarios, deterministic string and id
matching, one workspace. External benchmarks differ in every respect that matters:

- they are large (BIRD dev: 1,534 questions over 11 databases; the hidden test set is graded by the
  BIRD team) and bring their own data layout;
- they are scored by **executing SQL**, with an official scorer and specific metrics (EX, R-VES,
  Soft-F1);
- they have **leakage hazards** (gold answers and the evidence field sit next to the data);
- they have a submission format and leaderboard rules (an "oracle knowledge" column, a "New Dev"
  marker after the Nov 2025 cleanup);
- results are only meaningful as **comparisons between configurations on the same questions**, not
  as a single number.

Writing each benchmark as a one-off script would duplicate all of that. This RFC adds a small
adapter layer to `ekos-evals` and the first adapter, for BIRD.

## 2. Design

### 2.1 Adapter trait

In `ekos-evals`, module `bench`:

```rust
pub trait BenchAdapter {
    fn id(&self) -> &'static str;                         // "bird", "spider", ...
    fn prepare(&self, ctx: &PrepareCtx) -> Result<Vec<ProjectSpec>>;   // data -> EKOS projects
    fn scenarios(&self, split: &Split) -> Result<Vec<BenchScenario>>;
    fn run_one(&self, s: &BenchScenario, cfg: &ExperimentConfig, rt: &RunCtx) -> Result<Prediction>;
    fn score(&self, s: &BenchScenario, p: &Prediction) -> Result<Score>;
    fn export(&self, run: &RunReport, target: ExportTarget) -> Result<PathBuf>;   // submission files
}
```

CLI (all under the existing `ekos eval`):

```
ekos eval bench list
ekos eval bench prepare --suite bird-mini-dev
ekos eval bench run     --suite bird-mini-dev --cell c [--split tune|hold|all] [--limit N | --sample N]
ekos eval bench compare <report-a> <report-b>
ekos eval bench export  <report> --target bird-submission
```

Reports use the existing JSON report directory and `ekos eval history`.

### 2.2 Layout

```
evals/bench/<suite>/
├── suite.toml        # checked in: name, version, split definitions, archive URLs + sha256
├── configs/          # checked in: one TOML per experiment cell
└── data/             # gitignored: downloaded archives, extracted databases
```

`prepare` verifies archive checksums, extracts to `data/`, and creates **one EKOS project per
database** in a scratch EKOS home (RFC 0178), each with a `[[databases]]` entry of the right kind
(RFC 0236) and, where the benchmark ships one, a `[[data-dictionaries]]` entry (RFC 0237). It runs
the normal pipeline and stores the resulting ledger fingerprint. It is idempotent: an up-to-date
project is not rebuilt.

### 2.3 Leakage guards

These are tested, not documented:

1. **Allowlist ingestion.** Only `*.sqlite` and the dictionary files are ever given to a project.
   Question files, gold SQL, evidence and any `*.sql` answer file are not in any project directory.
2. **Ledger scan.** After `prepare`, the ledger is scanned for the normalised text of every gold SQL
   statement and every question in the splits used. Any hit fails `prepare`.
3. **Evidence is a prompt input only,** controlled by the experiment config. It is never written to
   a ledger.
4. **Few-shot examples** may come from the benchmark's *train* split only; the report records
   whether any were used.
5. **Tuning record.** Each report stores the split it ran on and a hash of the question ids. A
   hold-out split carries `tuned_on = false`; a run on the full set after the configuration was
   changed in response to hold-out results is marked `contaminated` by the harness when the
   hold-out hash appears in an earlier report with a different configuration.
6. Test-set predictions are produced only by `export`; no scorer for the hidden test set exists
   locally.

### 2.4 Experiment matrix

An experiment cell is one TOML (RFC 0239 §2.3). The standard BIRD set:

| Cell | Context | Evidence | Value lookup |
|---|---|---|---|
| A | raw DDL | off | off |
| B | raw DDL | on | off |
| C | EKOS pack | off | on |
| D | EKOS pack | on | on |
| C′ | EKOS pack | off | off |

C against A is the headline comparison (does EKOS recover what the evidence otherwise supplies);
D against B shows what EKOS adds when the standard hint is present; C′ isolates the contribution of
question-time value lookup. The model route, `n` and `r` are held fixed across cells.

### 2.5 Scoring

- **EX (execution accuracy)**, implemented natively: both queries run with the same limits and the
  result *sets* are compared the way BIRD's evaluator compares them (order-insensitive, duplicates
  collapsed). A **parity test** runs the official Python evaluator over the same predictions on a
  fixed sample and requires identical per-question verdicts; the official scripts are not
  vendored, only invoked from the test when present.
- **R-VES and Soft-F1** as defined in `bird-bench/mini_dev`, also with parity tests. The formulas are
  not re-derived in this RFC.
- **Per-difficulty and per-database** breakdowns.
- **EKOS-native, deterministic:** schema-linking recall, join-path recall, value-hit rate (RFC
  0238); SQL grounding rate (RFC 0239 §2.2 step 6).
- **Resource:** tokens, cost, latency and P95 from the existing report.

**Grounding denominator.** Per the RFC 0139 lesson, questions with an unparseable or non-executing
answer count as *ungrounded* and *incorrect*; they are never dropped from the denominator.

### 2.6 Statistics

A single accuracy number on 500 questions carries roughly ±4 points of sampling noise at 95%, so
the report always prints `n` and a bootstrap 95% interval, and `compare` runs a **paired**
comparison on the questions both runs answered (paired bootstrap on the per-question difference;
McNemar's test is an acceptable alternative). A difference is called a difference only when the
interval excludes zero. The report states which questions flipped in each direction, for error
analysis.

### 2.7 BIRD adapter

- **Suites:** `bird-mini-dev` (500 questions; SQLite first, the PostgreSQL variant later because it
  exercises RFC 0235), `bird-dev` (1,534; the cleaned split `bird-sql-dev-1106` is a separate suite
  so reports can be marked "New Dev"), later `bird-train-fewshot` for example selection.
- **Inputs:** the dev JSON (question id, `db_id`, question, evidence, gold SQL, difficulty), the
  per-database SQLite file and `database_description/*.csv`.
- **Export `bird-submission`:** the prediction file in the format given by BIRD's submission
  guidelines, plus a metadata sheet (method name, model, size, whether oracle knowledge was used —
  cell C is the "no oracle knowledge" entry). The guidelines were not read when this RFC was drafted;
  **confirm the exact format before implementing `export`.**

## 3. Non-goals

- Wiring LLM runs into CI (same position as RFC 0138). Only the deterministic metrics (schema-linking
  recall, parity tests on recorded predictions) run in CI.
- Local scoring of any hidden test set.
- A leaderboard publisher. `export` writes files; sending them is a human step.
- Tuning on test-set feedback.

## 4. Verification

- Adapter trait exercised by a tiny in-repo fixture suite (two databases, ten questions) with a mock
  model, so the whole prepare → run → score → compare path runs in `cargo test`.
- Leakage tests: plant a gold statement inside a project directory and assert `prepare` fails; plant
  evidence text in a ledger and assert the scan finds it.
- EX parity against the official evaluator on a recorded 100-question prediction set.
- `compare` on synthetic paired data with a known difference: the interval excludes zero when it
  should and does not when the two runs are identical.
- Budget stop: a run with a tiny `budget-usd` stops early and the report says so.

## 5. Acceptance criteria

- [x] `prepare` builds all eleven dev projects, with the leakage scan clean.
- [x] Cells A–D and C′ run on Mini-Dev with the same model and settings; each report carries the
      config, prompt hash, ledger fingerprint, n, interval, tokens and cost. (A 100-question
      hold-out sample per cell, Zen Haiku 5.5 and local qwen2.5-coder:3b; see §7.)
- [x] EX parity with the official evaluator on the sample: 704 recorded predictions (three reports),
      identical EX verdicts — see §7.
- [x] `compare C A` and `compare D B` print the paired result.

## 6. Open questions

- [ ] Split design: which fraction of Mini-Dev is the tuning split? Proposed: fixed seed, stratified by
      database and difficulty, 40% tune / 60% hold-out; the full dev set is the final check.
- [ ] Should the hold-out hash check be advisory or fail the run? Proposed: mark and print, not fail.
- [ ] Vendored scorer or invoke-only? Licence and update cadence (the Mini-Dev data was corrected on
      2026-10-28) argue for invoke-only with a pinned commit.

## 7. Implementation notes (2026-11-03)

What shipped, and where it differs from the draft above.

**Code.** `ekos-evals::bench` (`crates/evals/src/bench/`): `suite` (suite.toml, checksums, the
`data/` layout), `split`, `leakage`, `context` (raw DDL, the EKOS pack, value lookup, SQL shape
measures), `sqlexec` (read-only SQLite under a deadline; EX, Soft-F1, R-VES), `stats`, `report`,
`bird`, `fixture`. CLI: `crates/cli/src/commands/eval_bench.rs`; catalogue rows `eval bench list`
/ `compare` (read), `prepare` / `run` / `export` (local — never from the web console). The suite
is `evals/bench/bird-mini-dev/` (suite.toml + five cells); `data/` and
`evals/reports/bench/` are gitignored.

**The cell flag is `--cell`, not `--config`.** `--config` is the global `ekos.toml` path, so a
subcommand option of that name collides (clap panics on the type mismatch). `--cell c` resolves to
`configs/c.toml`; a path also works.

**`--sample N`.** A fixed, hash-ordered subset of the split, kept in question order, so a short
run spans every database instead of the first one (`--limit` keeps the first N, as drafted). The
report's question-id hash records exactly which questions ran; a sampled hold-out run therefore
does not match the full hold-out hash in the contamination check.

**RFCs 0236–0239 did not exist when this was built.** The draft leans on them; the substitutes:

| Draft dependency | What this implementation does instead |
|---|---|
| RFC 0236 — a `sqlite` `[[databases]]` kind | `prepare` reads the database's own catalogue (`PRAGMA table_info` / `foreign_key_list`) and writes one `schema.sql` per project: `CREATE TABLE` with double-quoted identifiers (read with the `postgres` dialect rule the project config adds), primary and foreign keys. No rows. |
| RFC 0237 — `[[data-dictionaries]]` | Each `database_description/<table>.csv` row naming a real column becomes `COMMENT ON COLUMN … IS '<description>; values: <value description>'` in the same file, which the SQL analyzer already turns into column descriptions (RFC 0146 Phase 2). CSVs are read as UTF-8 (BOM stripped) or Latin-1. |
| RFC 0238 — value lookup | `prepare` stores the distinct text values of every column with at most 5,000 of them (≤80 chars) in `data/values/<db>.json` — never a ledger. At question time a value whose normalised text appears as whole words in the question is offered (longest first, ≤3 per column, ≤12 in all), minus a stop list of question words; the tune split showed why ("What" is a card name in card_games). |
| RFC 0239 — experiment config, pack, grounding | `configs/<cell>.toml` is `ExperimentConfig` (context `raw-ddl` / `ekos-pack`, `evidence`, `value-lookup`, `few-shot`, `n`, `r`, `max-tokens`, `ves-iterations`, `timeout-seconds`, `[llm]` = an `ekos.toml` `[llm]` table). The EKOS pack is rendered from the project's ledger only: `Table` objects with their columns, types, primary keys and descriptions, plus `ForeignKey` edges as join paths. "Grounded" = the prediction ran and names only tables the context showed. |

**Prompt.** Every cell uses the Mini-Dev 1024 fixed baseline template
(`baseline_inference/prompt_template.txt`) byte for byte; only the schema text and the question
block (evidence line first, when on) differ. `r` repair rounds send an execution error back; `n > 1`
votes by result set (with a temperature-0 route and the LLM cache, candidates are identical — `n`
needs a sampling route to mean anything).

**Projects.** One per database in `data/home` (tenant `bench`), built by running `ekos --home …
--tenant bench --project <db> build|recover|resolve|compile|commit` as child processes with the API
key variables removed, so building a benchmark project never calls a model. Up to date = same
source digest and a compiled CKM; `--force` rebuilds. A project that compiles no `Table` fails
`prepare` (the silent-empty-schema failure of an unparsed dialect).

**Leakage.** Allowlist on archive extraction (`<db>/<db>.sqlite`, `<db>/database_description/*.csv`
only) and on each project directory; a scan of the generated sources before the pipeline and of
every object and relationship after it, for the normalised text of every question, gold statement
and evidence string (evidence of 40+ normalised characters). An evidence sentence that the
benchmark's own dictionary already contains is not a leak (it is a shipped input) and is dropped;
question and gold hits always fail. On Mini-Dev 1024 the scan searched 1,349 texts and found none.

**Scoring.** EX compares result sets with Python's equality (`1 == 1.0`): a real with an integral
value is the same cell as the integer. Soft-F1 follows `evaluation_f1.calculate_f1_score` line by
line. R-VES follows `evaluation_ves`: note that the reference drops *every* sample when all ratios
are equal (σ = 0 fails its strict 3σ test, and the empty list is a caught division by zero), so a
single timed run never earns R-VES; the cells use 3 runs (the official scorer uses many more, so
local R-VES is indicative only). Budget: the cell's `[llm] budget-usd` goes through the RFC 0197
router; a `LlmError::Budget` stops the run and the report says "STOPPED EARLY". A metered route
needs `--allow-metered`.

**Export `bird-submission`.** The prediction format is the one the official evaluator reads
(`evaluation_utils.package_sqls`): a JSON object `question_id → "<SQL>\t----- bird -----\t<db_id>"`
in question order (written by hand — a JSON map would sort the keys, and the evaluator pairs
predictions with gold lines by position), plus `<stem>.metadata.json` (method, model, size,
oracle knowledge, New Dev, n, hashes, fingerprints). BIRD's site asks for test submissions by e-mail
following its guideline; no public machine-readable spec was found, so sending stays a human step.

**Parity.** `crates/evals/tests/bird_parity.rs` imports the official `execute_sql`, `package_sqls`,
`calculate_ex` and `calculate_f1_score` from a `bird-bench/mini_dev` checkout (invoke-only, never
vendored; checked against commit `1c49a375`) and requires identical per-question EX and Soft-F1
verdicts on a recorded report. It is skipped unless `EKOS_BIRD_EVALUATOR`, `EKOS_BIRD_SUITE_DIR`
and `EKOS_BIRD_REPORT` are set.

**Open questions, as decided.** Split: fixed seed 240, stratified by database and difficulty, 40%
tune / 60% hold-out. Hold-out hash check: advisory (mark and print). Scorer: invoke-only.

**First results (2026-11-03, a fixed 100-question sample of the hold-out split).**

| Cell | Zen Claude Haiku 5.5 EX | Billed in / out tokens | Local qwen2.5-coder:3b EX |
|---|---|---|---|
| A raw DDL | 52% | 285,414 / 125,511 | 5% |
| B raw DDL + evidence | 47% | 285,066 / 122,310 | 4% |
| C pack + value lookup | 48% | 286,097 / 124,654 | 2% |
| D pack + evidence + value lookup | 46% | 299,123 / 127,218 | 3% |
| C′ pack alone | 53% | 269,832 / 122,912 | 3% |

- **Zen cost:** $0.45 processed, $0.39 billed. The cache served 108 calls whose prompt was
  identical across cells (B equals A when a question has no evidence).
- **No pair differed significantly** (C − A −4, CI −13 to +5).
- **The local 3B model executes only ~60% of its answers**, so it cannot tell the cells apart.
  It is a zero-cost check that the pipeline runs, not a measurement.
- **Mini-Dev 1024 is 92% "challenging"** (460 of 500).

These hold-out results motivated RFC 0241, which tuned on the tune split only: on the full
hold-out split it reached 63.9% against 53.3% for the baseline (+10.6, CI +4.3 to +16.9).

**Parity, run 2026-11-04** against `bird-bench/mini_dev` at `1c49a375`, with the evaluator on
Python 3.10 / SQLite 3.37.2:

| Report | Questions | EX mismatches | Soft-F1 mismatches | Engine-version differences |
|---|---|---|---|---|
| G (hold, final) | 302 | 0 | 0 | 0 |
| A-4k (hold) | 302 | 0 | 0 | 1 |
| C (hold sample) | 100 | 0 | 0 | 0 |

The one difference is A-4k question 217. Its prediction nests 16 `replace(…)` calls. SQLite
before 3.45 has a fixed parser stack and fails it with "parser stack overflow", which the official
scorer counts as 0; the bundled SQLite 3.46 grows the stack and runs it. The answer is wrong in
both (EX false), so only Soft-F1 differed: 0.2 here, 0.0 there. The test now reports such cases
separately and still fails on any other mismatch. A correct answer that hits the same limit would
be EX-correct here and wrong under an old evaluator, so the scorer's SQLite version belongs in any
reported number.

To run it again, with a `bird-bench/mini_dev` checkout:

```
EKOS_BIRD_EVALUATOR=<checkout>/evaluation EKOS_BIRD_SUITE_DIR=$PWD/evals/bench/bird-mini-dev \
EKOS_BIRD_REPORT=<a report> cargo test --manifest-path ekos/Cargo.toml -p ekos-evals \
  --test bird_parity -- --nocapture
```



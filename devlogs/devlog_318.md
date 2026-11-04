# Devlog 318 — Benchmark adapters, BIRD Mini-Dev, and Text-to-SQL quality (RFC 0240, RFC 0241)

**Date:** 2026-11-04
**PRs:** — (local branch, merged to main after local gates)
**Branch:** feat/rfc-0240-bench-bird → main

---

## Summary

`ekos eval bench` is a benchmark layer on top of RFC 0138's harness. It has an adapter trait, a
fixed-seed tune/hold-out split and leakage guards, and it scores by execution, mirroring the
official BIRD scorer. It reports bootstrap intervals and paired comparisons, and exports a
submission file. The first adapter is BIRD, with the Mini-Dev 1024 release (2026-10-28)
checked in as a suite.

The first runs showed EKOS's pack no better than raw DDL. RFC 0241 then added four things,
**all tuned on the tune split only**:

- column profiles;
- value lookup v2;
- an EKOS prompt with result checks;
- failure categories and cache-aware token accounting in the reports.

On the full hold-out split, run once, the best cell (G) scored **63.9% EX against 53.3%** for
the same baseline with the same output allowance: **+10.6 points, 95% CI [+4.3, +16.9],
p = 0.0014**. Total metered spend was $2.06 (RFC 0240 runs $0.39, RFC 0241 runs $1.67, against
an approved $2 for RFC 0241).

---

## RFC 0240 — benchmark adapters and the BIRD adapter

### Problem / motivation
RFC 0138 grades EKOS's own questions by string matching. An external benchmark brings its own
data, is scored by executing SQL, has leakage hazards and a submission format, and only means
something as a comparison of configurations on the same questions.

### What was built

| Component | Where |
|---|---|
| `BenchAdapter` trait, `ExperimentConfig` (a cell), `run` loop with budget stop, `ledger_fingerprint` | `crates/evals/src/bench/mod.rs` |
| `suite.toml` (archives pinned by SHA-256), the `data/` layout | `bench/suite.rs` |
| Fixed-seed stratified split, 40% tune / 60% hold-out; `--sample N` | `bench/split.rs` |
| Leakage guards: archive and project allowlists; scan of sources and ledgers | `bench/leakage.rs` |
| Raw DDL, the EKOS pack from a ledger, value lookup, SQL shape measures | `bench/context.rs` |
| Read-only SQLite under a deadline; EX, Soft-F1, R-VES | `bench/sqlexec.rs` |
| Bootstrap interval, paired comparison, McNemar | `bench/stats.rs` |
| Report, contamination check, `compare` | `bench/report.rs` |
| BIRD adapter: extraction, `schema.sql` + `COMMENT ON COLUMN` from the dictionary, prompt, export | `bench/bird.rs` |
| Two-database, ten-question fixture with a scripted model | `bench/fixture.rs` |
| CLI `ekos eval bench list|prepare|run|compare|export`; bench runs in `ekos eval history` | `crates/cli/src/commands/eval_bench.rs` |
| Suite + cells A, B, C, D, C-prime; Zen cells | `evals/bench/bird-mini-dev/` |
| Official-scorer parity test (env-gated, invoke-only) | `crates/evals/tests/bird_parity.rs` |

Two leakage guards are worth spelling out:

- **Archive and project allowlists:** only `<db>.sqlite` and the dictionary CSVs leave the
  archive, and only `schema.sql` may sit in a project directory.
- **Scan of sources and ledgers:** they are searched for every normalised question, gold
  statement, and evidence string of 40 or more characters.

### Implementation details worth remembering
- **Projects are built by running the `ekos` binary on itself.** Each stage runs as
  `ekos --home <suite>/data/home --tenant bench --project <db> <stage>`, with the API key
  variables removed, so building a benchmark project never calls a model.
  `project::create_quiet` was split out of `project add`, because `add` prints to stdout and
  that broke `--json`.
- **The pack comes from the ledger only.** It holds `Table` objects with their columns, types,
  primary keys and the descriptions the SQL analyzer took from the generated `COMMENT ON
  COLUMN` lines, plus `ForeignKey` edges as join paths.
- **EX uses Python equality**, so a real with an integral value equals the integer
  (`1 == 1.0`).
- **Prediction files are written by hand, in question order.** A JSON map would sort the keys,
  and the official evaluator pairs predictions with gold lines by position.
- **Rerunning `prepare` is cheap.** An unchanged source digest plus an existing CKM means the
  project is up to date. A project that compiles no `Table` fails.

### Decisions
- **Substitutes for the RFCs this one leans on.** RFCs 0236–0239 (a `sqlite` database kind, data
  dictionaries, value lookup, an experiment config) did not exist. They are replaced by a
  generated `schema.sql`, `COMMENT ON` lines and a value index kept outside the ledger, all
  recorded in RFC 0240 §7.
- **The cell flag is `--cell`.** `--config` is a global flag, and a subcommand flag of the same
  name makes clap panic.
- **The official scorer is invoked, never vendored** (licence, and the data changes often).

---

## RFC 0241 — Text-to-SQL quality

### Problem / motivation
On the first runs (100 hold-out questions, Zen Haiku 5.5), A raw DDL scored 52%, C-prime pack
53% and C pack + value lookup 48%. Re-executing the misses sorted them by cause:

- 6% had no SQL, because the 1,024-token output cap ran out mid-reasoning;
- 7% hit an execution error;
- 27% returned results of the right shape with wrong values: grain errors, off-by-a-few counts,
  float last digits.

Value lookup made things worse by pushing `= 'Creature'` for a list column.

### What was built

| Change | Effect on the tune split |
|---|---|
| Column profiles (rows, nulls, distinct, unique, examples, list columns) at `prepare` | E − A +11.1 |
| Value lookup v2 (one line per value; list columns use `LIKE`; spelled-as-stored rule; rarity order) | F − E −2.5 (not significant) |
| `ekos-v1` prompt (asked columns only, row grain, value spelling) + result checks (empty / single NULL → one repair round) | G − F +11.6 |
| Three varied-context candidates with voting | H − G −1.5 (not significant), 1.9× the cost: dropped |
| Failure categories, truncation flag, billed vs cached tokens, category moves in `compare` | measurement |

### Results

**Tune split (198 questions):**

| Cell | EX |
|---|---|
| A | 45.5 |
| E | 56.6 |
| F | 54.0 |
| **G** | **65.7** |
| G-nv | 63.6 |
| H | 64.1 |

**Final, full hold-out split (302 questions, run once):**

| Cell | EX | 95% CI | Soft-F1 | R-VES | Billed tokens in / out | Cost |
|---|---|---|---|---|---|---|
| A-4k (cell A, `max-tokens` 4096) | 53.3 | 47.7–59.3 | 57.6 | 49.5 | 468,612 / 308,078 | $0.201 |
| **G** | **63.9** | 58.6–69.2 | 67.8 | 59.7 | 1,509,241 / 295,405 | $0.299 |

G − A-4k = +10.6, CI [+4.3, +16.9], McNemar p = 0.0014. By database the gain is uneven:

| Database | G | A-4k |
|---|---|---|
| thrombosis_prediction | 65 | 33 |
| student_club | 78 | 43 |
| california_schools | 53 | 37 |
| toxicology | 95 | 68 |
| formula_1 | **55** | **79** |

formula_1 is worse under the pack; that is not examined yet.

### Decisions
- **A-4k as the final baseline, not A.** A's 1,024-token cap cost it 9% of the tune split to
  truncation. Comparing against A would credit EKOS with a harness setting.
- **G over H and over G-nv,** on tune results only. H costs 1.9× for no gain, and G-nv is 2
  points lower (not significant).
- **New cell fields are not serialised at their defaults**
  (`#[serde(skip_serializing_if = …)]`), so cells A–D keep their configuration hashes and the
  contamination check stays valid.

---

## Knowledge Captured

- **BIRD Mini-Dev 1024 is 92% "challenging"** (460 of 500 questions), and 27% of its questions
  have no evidence. Older Mini-Dev numbers are not comparable.
- **BIRD list columns use `,` with no space** (`Artifact,Creature`). A comma alone does not
  make a list: card names contain commas, and `leadershipSkills` holds a serialised dict. The
  working rule is that list elements also occur as values on their own.
- **The bundled SQLite has no math functions.** `libsqlite3-sys` 0.30's bundled build has no
  `SQLITE_ENABLE_MATH_FUNCTIONS`; Ubuntu 22.04's Python `sqlite3` (3.37) has them. Gold SQL
  using `FLOOR` failed only here.
  - They are now registered in `Db::open` (rusqlite `functions` feature).
  - SQLite 3.37 computes `log10(x)` as `ln(x) × (1/ln 10)`, so `log10(100)` is
    1.9999999999999998. A test pins that.
- **The official R-VES gives 0 when every timing ratio is equal.** σ = 0 fails its strict 3σ
  filter, and the empty list is a caught division by zero. So one timed run never earns R-VES.
- **Ollama ignores `[llm] base-url`** and reads `OLLAMA_BASE_URL`. The CLI end-to-end test sets
  that variable to reach its stand-in server.
- **The LLM cache serves identical prompts across cells** (B with no evidence is A's prompt).
  So processed tokens exceed billed tokens: 108 of 724 calls on the first Zen run. Reports now
  separate the two, and cost counts billed calls only. Real spend is in
  `data/llm/llm-usage.jsonl`.
- **The local 3B model (qwen2.5-coder:3b on a 4 GB GTX 1650)** takes about 30 s per question,
  executes only ~60% of its answers and scores 2–5%. It is useful only as a free check that the
  pipeline runs. A 16k context pushed 45% of the model to the CPU; 10k was faster.
- **Value lookup is easy to make harmful.** Exact-value hints on list columns, question words
  that are also values ("What" is a card name) and one line per column all hurt. v2 fixed the
  harm but is not shown to help.
- **Official-scorer parity:** 704 recorded predictions (reports G, A-4k and C) gave identical EX
  verdicts to `bird-bench/mini_dev`'s scorer. It ran on Python 3.10 / SQLite 3.37.2, once the user
  asked for it explicitly; the auto-mode classifier had blocked it before. The one Soft-F1
  difference is a SQLite version effect: SQLite before 3.45 has a fixed parser stack and rejects
  16 nested `replace(…)` calls ("parser stack overflow"), while the bundled 3.46 runs them. The
  test lists such cases apart. A reported number should name the scorer's SQLite version.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/evals/src/bench/*.rs` | New: the benchmark layer and the BIRD adapter (RFC 0240), profiles, value lookup v2, ekos-v1, result checks, categories, cache accounting, math functions (RFC 0241) |
| `ekos/crates/evals/tests/bench_fixture.rs`, `bird_parity.rs` | Fixture end to end; leakage, budget, contamination, result-check and cache tests; parity test |
| `ekos/crates/evals/Cargo.toml` | async-trait, toml, sha2, hex, rusqlite (hooks, limits, functions), sqlparser, zip, tracing |
| `ekos/crates/cli/src/commands/eval_bench.rs` | New: `ekos eval bench …` |
| `ekos/crates/cli/src/app.rs`, `commands/{catalogue,eval,mod,project}.rs` | Wiring, catalogue rows, bench runs in `eval history`, `project::create_quiet` |
| `ekos/crates/cli/tests/bench.rs` | Real binary: prepare → run (stand-in Ollama) → compare → export → history; the metered-route flag |
| `ekos/docs/rfcs/0240-benchmark-adapters-and-bird.md`, `0241-text-to-sql-quality.md` | RFCs with implementation notes and results |
| `evals/bench/bird-mini-dev/` | `suite.toml`, cells A–D and C-prime (Ollama), `configs/zen-haiku/` (A–H, A-4k, G-nv) |
| `evals/README.md`, `.gitignore`, `CLAUDE.md`, `TODO.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | Docs |

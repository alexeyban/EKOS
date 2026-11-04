# RFC 0241 — Text-to-SQL quality on BIRD: profiles, value lookup v2, result checks, failure categories

**Status:** Accepted
**Created:** 2026-11-03
**Builds on:** RFC 0240 (benchmark adapters, BIRD)

## 1. Problem

The first RFC 0240 runs (Zen Claude Haiku 5.5, a fixed 100-question sample of the hold-out split)
scored A raw DDL 52%, C-prime EKOS pack 53%, C pack + value lookup 48%, B/D with evidence 47%/46%.
No difference was significant. Re-executing the failed answers sorted them by cause:

| Failure | A | C-prime |
|---|---|---|
| No SQL: the answer hit the output cap while reasoning | 6 | 6 |
| Execution error | 7 | 6 |
| Wrong number of columns | 4 | 3 |
| Wrong number of rows | 4 | 5 |
| Right shape, wrong values | 27 | 27 |

"Right shape, wrong values" holds grain errors (counting the printings of a card when its rulings
were asked), off-by-a-few counts from NULL or duplicate handling, and float last-digit differences.
Value lookup made things worse. It offered `'Creature'` as an exact value of `cards.types`, which
holds lists ("Artifact, Creature"), so the model wrote `= 'Creature'` where the gold SQL has `LIKE`.
It also offered question words that happen to be values, and repeated a value once per column.

Those hold-out results motivated this RFC. Every change below is therefore tuned on the tune split
only, as a new cell. The hold-out split is used once, at the end.

## 2. Design

### 2.1 Measurement

- **Failure category** per question: `correct`, `no_sql`, `exec_error`, `wrong_columns`,
  `wrong_rows`, `wrong_values`. It is computed in `score` from the two row sets, and the report
  counts each category.
- **Truncated** per question: no SQL and the output tokens reached the cap.
- **Billed vs processed tokens.** A call the LLM cache served is counted as `cached_calls`, through
  `LlmProvider::cache_stats()` before and after the call. The report prints processed tokens and
  the share served from cache, and cost covers billed calls only.
- **`compare`** also prints how failure categories moved between the two runs.

### 2.2 Context

- **Column profiles** (`Profile`), built at `prepare` from the database file and stored in
  `data/values/<db>.profile.json` (never in a ledger). They hold:
  - rows per table;
  - per column: nulls, distinct values, whether it is unique, three example values;
  - **list-valued** columns: at least 5% of the distinct values hold a comma, every value is at
    most 60 characters long, and at least half of the elements of those values also occur as
    values on their own (this is what separates `Artifact,Creature` from a name with a comma).

  The pack renders them as one line per column.
- **Value lookup v2.**
  - One line per value, naming every column it occurs in.
  - A list-valued column is marked "use LIKE '%v%'".
  - A one-word value counts only when the question spells it exactly as stored, case included,
    and not as its first word, or quotes it, or it holds digits. The tune design first said
    "capitalised"; that would have rejected correct lowercase values such as `retail`.
  - Values are ranked by rarity: fewer columns first, then longer.

### 2.3 Generation (EKOS-prompt cells only)

`prompt = "baseline" | "ekos-v1"`. The default is `baseline`, and fields left at their default are
not serialised, so existing cells keep their configuration hash. `ekos-v1` is the baseline
template with three extra instructions:

- return exactly the asked columns, in order;
- check the row grain before counting;
- give the SQL block first and keep the explanation short.

**Result checks** (`result-checks = true`) reuse the `r` repair rounds: a query that returns no
rows, or a single NULL, goes back with that fact. Nothing about the gold answer is ever shown.

**Varied-context candidates** (`n > 1` with `vary-context = true`). The route forces temperature
0, so a repeated prompt returns the same answer. Candidate *i* therefore gets a different
rendering instead: full pack; pack without examples; raw DDL plus profiles. The existing vote by
result set picks the answer.

## 3. Cells (Zen Haiku 5.5; budget ≤ $2 for all of this RFC's runs)

| Cell | Prompt | Context | Values | Checks | n |
|---|---|---|---|---|---|
| E | baseline | pack + profiles | off | off | 1 |
| F | baseline | pack + profiles | v2 | off | 1 |
| G | ekos-v1 | pack + profiles | v2 | on | 1 |
| H | ekos-v1 | varied | v2 | on | 3 |

All four cells use `max-tokens = 4096`. E, F and G run on the tune split. H runs on the tune split
only if spend allows. The best cell and A then run on the 300-question hold-out split.

## 4. Verification

Unit tests cover profile rendering, the value lookup v2 rules, the failure categories, a
result-check repair round with the scripted model, and cached-call counting. The fixture
end-to-end tests also pass. Real runs report EX with its interval, the category counts and the
token split.

## 5. Results (2026-11-04, OpenCode Zen, Claude Haiku 5.5)

**Tune split (198 questions; all tuning happened here):**

| Cell | EX | 95% CI | Billed cost |
|---|---|---|---|
| A — RFC 0240 baseline, `max-tokens` 1024 | 45.5% | 39–52 | $0.185 |
| E — pack + profiles | 56.6% | 50–64 | $0.201 |
| F — E + value lookup v2 | 54.0% | 47–61 | $0.118 (88 of 199 calls from the cache) |
| **G — F + ekos-v1 prompt + result checks** | **65.7%** | 59–72 | $0.191 |
| G-nv — G without value lookup | 63.6% | 57–70 | $0.114 |
| H — G, three varied-context candidates | 64.1% | 58–71 | $0.361 |

Paired on the tune split:

| Pair | Difference | 95% CI | McNemar p |
|---|---|---|---|
| G − A | **+20.2 points** | +12.1 to +28.3 | < 0.0001 |
| E − A | +11.1 | +4.0 to +18.2 | 0.005 |
| G − F (prompt + checks) | +11.6 | +6.1 to +17.7 | 0.0003 |
| F − E (value lookup v2) | −2.5 | −6.6 to +1.5 | 0.33 |
| G − G-nv (value lookup inside G) | +2.0 | −2.5 to +6.1 | 0.50 |
| H − G (voting) | −1.5 | −5.6 to +3.0 | 0.65 |

What moved, read from the outcome categories:

- A lost 18 answers to the output cap and 17 to execution errors. With 4,096 output tokens and
  the profiles, every new cell had none of either.
- G turned 22 `wrong_values` and 18 `wrong_rows` answers into correct ones, against 19 the other
  way.
- Varied-context voting did not pay at 1.9× the cost, so H was not taken forward.
- Value lookup v2 is no longer harmful but is not shown to help: G keeps it, G-nv is 2 points
  lower, and the difference is not significant.

**Final, the full hold-out split (302 questions, run once):** G against **A-4k**, which is cell A
with the same 4,096-token allowance, so the difference is EKOS's alone.

| Cell | EX | 95% CI | Soft-F1 | R-VES | Billed tokens in / out | Cost |
|---|---|---|---|---|---|---|
| A-4k | 53.3% | 47.7–59.3 | 57.6 | 49.5 | 468,612 / 308,078 (55 of 304 calls cached) | $0.201 |
| **G** | **63.9%** | 58.6–69.2 | 67.8 | 59.7 | 1,509,241 / 295,405 | $0.299 |

**G − A-4k = +10.6 points, 95% CI [+4.3, +16.9], McNemar p = 0.0014**: 38 `wrong_values` and 25
`wrong_rows` answers became correct, against 32 that went the other way. Neither run is marked
contaminated.

- **G costs more on input:** about 1.5 million billed input tokens against 0.47 million, because
  profiles and dictionary text triple the prompt. Output is the same.
- **By database (G vs A-4k):**
  - thrombosis_prediction 65 vs 33;
  - student_club 78 vs 43;
  - california_schools 53 vs 37;
  - toxicology 95 vs 68;
  - formula_1 **55 vs 79**: the pack is worse there; not examined yet.
- By difficulty, challenging (275 of 302) went 52.4 → 63.6.

**Spend:** RFC 0241 runs billed $1.67 of the approved $2 (Haiku 5.5 at $0.10 / $0.50 per million
tokens). That figure is read from the router's usage log, not estimated.

**Corrections found on the way:**

- `cards.types` lists are written `Artifact,Creature`, with no space, so the delimiter is `,`.
  A column counts as a list only when its elements also occur as values on their own: a name like
  "Kaiso, Memory of Loyalty" is not a list.
- The bundled SQLite has no math functions, while the official scorer's Python `sqlite3` does.
  The gold SQL of question 245 (`FLOOR`) failed here, and 4 local-model predictions were marked
  as errors. SQLite 3.37's math functions are now registered in `Db::open`, with a test pinning
  Python's values.

**Next:**

- Few-shot examples from `bird23-train-filtered` (a new suite; train split only).
- Look at formula_1.
- A stronger model, once a budget exists.
- The official-scorer parity run on these reports, which a human has to start.

## 6. Why formula_1 is worse under the pack (2026-11-04)

On the final hold-out run, G scored 55% on formula_1 against 79% for A-4k: it lost 9 questions
and gained 1. This analysis read hold-out results, so the fix below is a new cell (G2) that has
to be validated on the tune split before any new hold-out run, and that run will be marked as
following hold-out analysis.

**Cause, for 7 of the 9.** `qualifying.q2/q3` record "no time" two ways: NULL, and the empty
string. `q3` has 4,935 NULLs and 28 `''`.

- Profile v1 reported only the NULLs (`4935 null`), and the ekos-v1 prompt tells the model the
  comments give each column's null count. The model took NULL as the only missing marker, so
  `COUNT(q3)` and `MIN(q3)` counted or chose the `''` rows. In question 284 the fastest Q3 time
  came back as `''` for 9 drivers.
- With raw DDL, the same model wrote `q3 <> ''` in every one of those 7 questions, as the gold
  SQL does.
- The other two losses are a filter error (288) and lap times compared as text (255).

The pack stated a true but incomplete fact, and the model trusted it over its own caution.

**How common.** 44 of the 500 gold queries guard against empty strings: 18 in formula_1, 6 in
card_games, 6 in student_club, 5 in thrombosis_prediction. Sixteen columns in six databases hold
empty strings, among them `qualifying.q2/q3`, `trans.k_symbol` (53,433), `patient.sex` (13) and
`patient.admission` (34).

**Fix: profile v2 (`profile-version = 2`).**

- The profile file (now versioned; `prepare` rebuilds an older file) counts blank text values per
  column.
- A v2 line says `28 empty strings ('' also means missing: exclude with <> '' as well as IS NOT
  NULL)`.
- Example values never include a blank.
- Cells E–H keep the v1 rendering, so their results stay reproducible. Cell `G2` is G with v2.

**Status.** Built and unit-tested. Not yet measured: the RFC 0241 budget is spent, and measuring
G2 on the tune split costs about $0.20.


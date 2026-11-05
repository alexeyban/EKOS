# RFC 0242 — Text-to-SQL agent with gold-free checks: BIRD first, then real SQL and dbt

**Status:** Accepted
**Created:** 2026-11-05
**Builds on:** RFC 0031 (SQL dialects), RFC 0240 (benchmark adapters, BIRD), RFC 0241 (text-to-SQL quality)

## 1. Problem

RFC 0241's best cell, G2, has three parts:

- the EKOS pack: column profiles, value lookup v2 and empty-string counts;
- the `ekos-v1` prompt;
- one repair round, taken when a query fails, returns no rows, or returns a single NULL.

G2 scored 66.9% on the BIRD Mini-Dev hold-out split (302 questions). A-4k, with raw DDL and the
same model, scored 53.3%.

The obvious next step is more repair rounds. The G2 hold-out report says that alone cannot help:

| Final answer of a failed question | Count |
|---|---|
| Ran; same shape as gold, wrong values | 79 |
| Ran; wrong number of rows | 20 |
| Ran; wrong number of columns | 1 |
| No SQL, or an execution error | 0 |

The judge saw nothing wrong in 92 of the 100 failures. It sent 16 questions back, and 8 of them
became correct. With the same judge, more rounds can only reach the other 8.

**Varying the context did not help either.** Cell H (three candidates from different schema
renderings, then a vote) scored 64.1% on tune, against 65.7% for G.

**The failures are about meaning, not syntax.** We scanned the failure patterns and read samples:

| Pattern | Example |
|---|---|
| A value spelled differently in the data | `Location = 'Germany'` where the data says `'Berlin, Germany'` |
| A sentinel inside an aggregate | `AVG(height_cm)` over 0, which this data uses for "unknown" |
| A join that fans out before `COUNT`/`SUM` | — |
| Integer division | 13 of the 100 failures |
| `DISTINCT` missing or extra | 19 |
| Top-1 vs `MAX`, and ties | 12 |
| Date formats | 20 |
| A definition only the question's evidence states | — |

Several of these can be detected without the gold answer, either by a static check against what
EKOS knows about the schema or by a cheap probe query on the same database.

**Hold-out inspection.** The counts above come from the hold-out report, inspected on 2026-11-05
to motivate this RFC. Detector design is therefore redone on the tune split. The final measurement
uses a question set that has not been looked at (§5).

## 2. Design

### 2.1 `plugins/sql-dialect-sqlite`

This is a `SqlDialectParser` over sqlparser's `SQLiteDialect`, registered as `"sqlite"`. Its preprocessing is the identity.

The crate also exports `SQLITE_FACTS`: the dialect behaviour the checks consult, stated as data.

- `integer_division`: `7 / 2 = 3` when both operands are integers.
- `nulls_sort_first_ascending`: NULL is the smallest value.
- `like_case_insensitive_ascii`.
- `has_ilike`: false.
- `dates_are_text`: dates are compared as text, so the stored format matters.

Other dialects get the same `DialectFacts` struct (in `ekos-sqlgen`), filled from their own documented behaviour:

| Dialect | Integer `/` truncates | NULLs sort |
|---|---|---|
| PostgreSQL | yes | last ascending |
| ClickHouse | no (`/` is float) | — |
| Trino | yes | last |

### 2.2 `crates/sqlgen` (`ekos-sqlgen`)

The crate is dialect-generic. It holds no configuration, credentials or provider construction.

- **`SchemaView`** (trait) is what a check knows about the schema:
  - tables and their columns, with declared types;
  - primary keys;
  - foreign keys as join paths;
  - and, when a profile exists, per-column facts: row count, nulls, empty strings, distinct count,
    unique, examples, list-valued, and *frequent sentinel values*. A sentinel is a numeric `0` or
    `-1` that holds at least 1% of a column's rows while the rest of the column has the opposite
    sign or is non-zero. Profile v3 records this.

  There are two implementations:
  - `PackSchema`, in `ekos-evals`, from `DbPack`, `Profile` and `ValueIndex`;
  - `LedgerSchema`, in `ekos-sqlgen`, over `&dyn KnowledgeReader`, which reads `Table` objects,
    their `columns` and the `ForeignKey` edges, the same data `DbPack::from_ledger` reads.

- **`lint`**: static findings over the sqlparser AST.

  A `Finding` carries:
  - `code`;
  - `severity` (`error` | `warn`);
  - `message`;
  - the `subject` (a table, column or literal);
  - `fetch`: the context that would settle the finding, e.g. `DescribeTable("cards")`,
    `FindValues { text, column }` or `JoinPath(a, b)`.

  | Code | Rule |
  |---|---|
  | `L001` | Unknown table or column. Up to 3 nearest names are suggested (edit distance, case-folded). |
  | `L002` | A join whose column pair is not a known foreign key, in either direction, and neither side is a primary key. |
  | `L003` | `=` or `IN` on a list-valued column. |
  | `L004` | A string literal compared with `=` to a column with a value index, where the literal is not among the column's values. Case-insensitive matches and containing values are offered. |
  | `L005` | `/` with both operands integer-typed, when the dialect truncates. |
  | `L006` | `AVG`, `MIN` or `SUM` over a column with a frequent sentinel. |
  | `L007` | `ORDER BY x [ASC] LIMIT n` on a nullable `x`, when NULLs sort first. |
  | `L008` | A date-shaped literal compared with a text column whose examples have another format. |

- **`probe`**: findings that need the database, behind the `Probe` trait:
  - `count(sql) -> Result<u64>`;
  - `preview(sql, rows) -> Result<Preview>`, with the column names, the first rows and a
    truncation flag.

  | Code | Rule |
  |---|---|
  | `P001` | The query returns no rows. |
  | `P002` | The query returns a single NULL. |
  | `P003` | Value spelling. For each `col = 'lit'`: the exact, case-insensitive and `LIKE '%lit%'` match counts. The finding is raised when exact = 0 and either of the others is > 0. |
  | `P004` | Fan-out. For a query with an aggregate over a join: `COUNT(*)` vs `COUNT(DISTINCT <pk of the first table>)` of the `FROM … WHERE` part. Raised when they differ and the query has no `DISTINCT`. |
  | `P005` | Tie at the top. `ORDER BY … LIMIT 1` where the first two rows' sort keys are equal. |

  `P001` and `P002` are RFC 0241's result checks, moved here.

- **`agent`**: a tool loop over `LlmProvider::complete` with `LlmRequest::history`.
  - **Tool protocol.** `LlmProvider` has no native tool use, so the protocol is text: each turn the
    model writes one fenced ```` ```tool ```` block holding JSON `{"tool": …, "args": {…}}`, or a
    final ```` ```sql ```` block.
  - **Tools:**

    | Tool | Returns |
    |---|---|
    | `search_schema(text)` | Tables and columns whose name, description or examples match |
    | `describe_table(name)` | Columns, types, keys, profile and descriptions |
    | `join_path(from, to)` | The shortest FK path |
    | `find_values(text, column?)` | Value lookup, with the list-column note |
    | `run_sql(sql)` | Up to 20 rows plus the row count, read-only |
    | `check_sql(sql)` | Lint and probe findings |

  - **Submitting.** A ```` ```sql ```` block counts as `submit`. The submission is checked once
    more. If an `error` finding is open, or a `warn` finding the model has not yet seen for this
    SQL, the findings go back while turns remain.
  - **Budgets.** `max_turns` (default 6: the first answer and five more) and `max_tokens` per call.
    A turn with neither a tool block nor SQL is answered once with the protocol reminder.
  - **Transcript.** The tool calls, finding codes and tokens are recorded per question.
  - **Never shown to the model:**
    - gold SQL or its results;
    - the question's evidence, unless the cell turns it on;
    - other questions.

### 2.3 BIRD integration

`ExperimentConfig` gains three fields. Each is `skip_serializing_if` at its default, so existing
cells keep their hashes.

- `strategy`: `single` (default) | `loop` | `agent`.
  - `loop`: RFC 0241's candidate loop with the judge replaced by lint and probe findings. Each
    round's feedback carries the findings and the context their `fetch` names. Up to `r` rounds.
    It is the cheap ablation that separates "better detectors" from "an agent".
  - `agent`: §2.2's agent over `PackSchema` and a SQLite `Probe` that uses `sqlexec`'s read-only
    connection, deadline and row cap. The first message is the cell's full prompt (pack, values,
    template) plus the tool protocol, so the agent starts from G2's context.
- `max_turns`: the agent's turn budget.
- `checks`: the finding codes in use. Empty means all.

The report gains, per question: `turns`, `tools` (names in order), `findings` (codes raised) and
`open_findings` (codes left at submission). `Metrics` gains, per finding code, how often it fired
on answers scored correct vs wrong. That is each detector's precision, measured without changing
the scoring.

`ekos eval bench checks --report <file>` replays the findings over an existing report's final SQL,
with no model calls, and prints that table. This is how detectors are chosen (§5).

### 2.4 Real use

- **`ekos sql check [FILE|-]`** (catalogue class `read`) runs `LedgerSchema` lint over a project's
  ledger. With `--dialect` (default: the project's first `dialect-rules` dialect, else `generic`),
  it prints findings as a table, or as JSON with `--json`. Probes against live databases are not in
  this RFC. The `Probe` trait is where they plug in, through pg-live and ClickHouse `readonly=1`.
- **MCP tool `ekos_sql_check`** takes `{sql, dialect?}` and returns the lint findings. It reads
  only through the store, so it is in `SCOPED_TOOLS`. It is also listed in the console's
  `mcp_policy.SCOPED_TOOLS` as `read`.
- **Skill `.claude/skills/text-to-sql`**: the agent's procedure for Claude Code with the EKOS MCP
  server:
  1. search;
  2. describe tables;
  3. find the join path from the FK edges;
  4. check values;
  5. draft;
  6. run `ekos_sql_check`;
  7. preview the result when a database is at hand;
  8. answer with the SQL and the findings it resolved.
- **dbt regeneration benchmark (`dbt-regen` adapter).** For each documented model of a dbt
  project:
  1. hide the model's file and its ledger objects;
  2. let the agent write it from the description and the rest of the ledger;
  3. check that `ref()`/`source()` resolve;
  4. compare results with the original model in `PgSandbox`, one rolled-back transaction each.

  It needs a dbt project with seeds or a fixture database. It is specified here and built in a
  follow-up, once a suitable project is chosen.

## 3. Cells (Zen Claude Haiku 5.5; ≤ $3 in total for this RFC)

G3, the baseline the other cells are compared with, was added during the work (§6.2).

| Cell | Base | Change |
|---|---|---|
| J | G2 | `strategy = "loop"`, `r = 5`, checks kept by the replay |
| K | G2 context | `strategy = "agent"`, `max_turns = 6` |
| K2 | K | One revision of tools and prompt from tune transcripts |

Spend is read from `data/llm/llm-usage.jsonl` before each run.

## 4. Verification

- `ekos-sqlgen` unit tests: a positive and a negative case per lint and probe rule over an
  in-memory SQLite schema, and agent loops with a scripted provider (tool use, the turn cap,
  submission with an open finding, the protocol reminder).
- `ekos-evals`: J and K on the fixture suite with the scripted model; existing cell hashes are
  unchanged.
- The MCP tool list and console `SCOPED_TOOLS` parity tests, and the read-scope leak suite.

## 5. Choosing detectors and the final measurement

1. Replay the findings over G2's tune report. Keep a code when it fires on wrong answers at least
   twice as often as on correct ones, and on at least 3 wrong answers.
2. Run J, K and K2 on tune.
3. Final: the best of J/K/K2 and G2 on a set nobody has looked at.
   - If BIRD dev (1,534 questions, the same 11 databases) shares no question text and no gold SQL
     with Mini-Dev 1024, that set is a fixed-seed sample of 300 from it.
   - Otherwise it is the Mini-Dev hold-out, and the report says it was inspected.

## 6. Results (2026-11-05, OpenCode Zen, Claude Haiku 5.5)

### 6.1 The detector replay (no model calls)

`ekos eval bench checks` over G2's tune answers (198 with SQL, 64 wrong) found that the gold-free
detectors fire on few answers:

| Code | Right answers | Wrong answers |
|---|---|---|
| E000 (a probe timeout) | 1 | 0 |
| L002 | 2 | 1 |
| L004 (after excluding `= ''`) | 0 | 1 |
| L006 | 0 | 1 |
| P002 | 0 | 1 |

Decisions:

- **L002 was dropped.** It flagged sibling joins such as `qualifying.raceId = results.raceId`.
- **The rest were kept.** With so few firings, the §5 rule (≥ 3 wrong answers) would keep
  nothing. The kept codes never fired on a right answer except through a timeout.
- **`= ''` is now exempt from L004 and P003.** It is a test for a missing value, not a value
  spelling.

### 6.2 A confound found on the way: the pack had changed

All 11 ledgers had been rebuilt after G2 ran (SqlAnalyzerPass v6, `COMMENT ON` for quoted
identifiers), so the pack now carries the dictionary's column descriptions. Cell **G3** is G2's
configuration on the current ledgers; J and K are compared with G3, not G2.

### 6.3 Tune split (198 questions)

| Cell | EX | Δ vs G3 (paired, 95% CI, McNemar p) | Replies | Cost |
|---|---|---|---|---|
| G2 (pack without descriptions) | 67.7 | −1.0 | — | $0.13 |
| G3 | 68.7 | — | 204 | $0.01 (cache) |
| J: detector loop | 68.2 | −0.5 [−1.5, 0.0], p = 1.0 | 210 | $0.23 |
| K: agent | 74.2 | +5.6 [+1.5, +10.1], p = 0.019 | 229 | $0.25 |
| K2: agent + verify | **75.8** | **+7.1 [+2.5, +12.1], p = 0.007** | 466 | $0.51 |

- **Descriptions (G3 vs G2):** +1.0, CI [−4.0, +6.6]. Not shown: they turned 16 answers right and
  12 wrong.
- **J changed no outcome.** The loop sent back 12 answers and fixed none, as the replay
  predicted.
- **K: the model called tools on only 16 questions.**
  - All 5 tool-using flips went wrong → right.
  - The other flips (10 right, 4 wrong) are single-turn answers, changed by the appended
    protocol text alone.
- **K2:**
  - It shows the first clean answer's result once, asks for a check against the question
    (columns, one row per thing asked, form of the values), and asks for compared literals to be
    checked with `find_values`.
  - It used tools on 43 questions.
  - It beat K by +1.5 (not significant) at twice K's cost.

### 6.4 Hold-out split (302 questions)

**About the set.** Mini-Dev 1024 is built from BIRD dev's questions and databases, so BIRD dev
overlaps it by construction. By §5's rule the final set is therefore the Mini-Dev hold-out:

- Its G2 failure patterns were **inspected on 2026-11-05**, to motivate this RFC.
- No cell was tuned on it.

**Two departures from plan:**

- **G3 stopped at its own $0.30 cap** after 242 questions. `G3-full` is the same configuration
  with a $0.60 cap. Its first 242 answers came from the LLM cache, so it equals G3 there.
- **K2's hold-out run used later check code than its tune run.** These were the fixes from RFC
  0243's live tests:
  - the L005 wording;
  - scalar-subquery division;
  - P003 querying the table as the query names it;
  - E001.

  They are general fixes, none tuned on the hold-out.

| Comparison (paired, 302) | EX | Δ, 95% CI, McNemar p |
|---|---|---|
| K2 vs G3-full | 76.2 vs 71.5 | +4.6 [0.0, +9.3], p = 0.06: **not shown at 5%** |
| G3-full vs G2 (column descriptions) | 71.5 vs 66.9 | +4.6 [+0.3, +8.9], p = 0.065 |
| K2 vs A-4k (raw DDL, same model and output cap) | 76.2 vs 53.3 | **+22.9 [+16.9, +28.8], p < 0.001** |

- **Flips, K2 vs G3-full:** K2 turned 31 answers right that G3-full had wrong, and 17 the other
  way.
- **K2's cost:** $0.78 for the 302 questions (714 replies, 78 questions with tool calls), against
  $0.30 for G3.
- **Spend:** this RFC's runs spent **$2.13** of the approved $3.

**Reading.**

- **The agent's gain is real on tune** (+7.1, p = 0.007). On the hold-out it is the same size in
  the same direction (+4.6), but the interval touches zero. With 302 questions a 4–5 point effect
  is at the edge of what can be shown.
- **The column descriptions** in the pack are worth about the same on the hold-out (+4.6), against
  +1.0 on tune.
- **Together**, pack + profiles + descriptions + the agent take the same model from 53.3% to
  76.2%.
- **Gold-free detectors** (§2.2) catch few of the remaining errors. Most wrong answers run, return
  the right shape, and mean something else. What helped was letting the model look at the data
  and its own result.

# RFC 0164 — Logic lowering, constrained reconstruction and differential execution

**Status:** Phase 1 implemented (2026-10-11, devlog_252): deterministic view lowering with a
round trip, the anti-invention check, the fidelity gate and trigger proposals. Constrained
reconstruction and V5 differential execution are not started — see "Phase 1" below
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0163 (the procedural IR this consumes — a hard prerequisite), RFC 0154 (coverage
requirement), RFC 0156 (V5 lives here), RFC 0159/0165 (target designs), RFC 0150 (anti-invention
discipline), RFC 0027/0028 (Transformation IR and `ekos_transformation_diff`)

---

## Summary

Turns recovered logic into target logic: deterministic lowering where the IR maps cleanly, IR-
constrained LLM reconstruction where it does not, and **differential execution** — the same fixtures
run against PostgreSQL and the target, outputs compared by the RFC 0155 canonical form — as the
pass/fail oracle for both.

This is validation tier V5. It is the only place in EKOS Migrate where a model's output can become a
shipped artifact, and it is fenced accordingly.

## Motivation

Views and functions are where migrations quietly fail. A table either has the right rows or it does
not, and RFC 0156 settles it. A function can produce the right answer on every row anyone tested and
the wrong answer on the branch nobody hit — the fiscal-year-end branch, the negative-quantity
branch, the branch that only runs when a nullable column is null.

Two things are therefore required, and neither is sufficient alone: a check that the *logic* maps to
recovered source logic (so nothing was invented), and a check that the *outputs* match on data that
exercises the branches (so nothing was misread). This RFC provides both.

## Design

### Deterministic lowering first

Most objects never need a model.

| Source | ClickHouse target |
|---|---|
| View over tables, no procedural code | `VIEW`, or `MATERIALIZED VIEW` where the workload justifies it |
| Materialized view with a refresh pattern | Refreshable `MATERIALIZED VIEW` |
| SQL-language function, single statement | `VIEW` or a parameterized query in the calling application |
| `ProcedureIr` whose body is one `ProcStmt::Sql` | direct lowering, same as a view |
| Set-returning function over one query | `VIEW` |

Lowering walks the IR and emits target SQL through the dialect builder from RFC 0160 — never string
concatenation of identifiers. The result is a `MigrationProposal` with `author: "lowering"`, and it
is checkable: the emitted SQL is re-parsed, re-lowered to IR, and diffed against the source IR. A
round-trip that does not match is a lowering bug and fails rather than shipping.

**ClickHouse incremental materialized views are a semantic trap and are treated as one.** A CH
`MATERIALIZED VIEW` fires on insert to its source table; it is a trigger, not a maintained
projection, and it does not see pre-existing rows or updates. Lowering a PostgreSQL matview to one
silently changes the semantics. The lowerer therefore emits a refreshable MV where the version
supports it, and otherwise emits a compatibility finding (RFC 0158) requiring a disposition. It does
not pick the convenient option.

### Constrained reconstruction

Where lowering cannot proceed — real procedural control flow, exception handling, cursor-driven
iteration — an LLM proposes a target implementation under three constraints.

**1. The anti-invention check, which is now real.** Every predicate, branch condition, constant,
table reference and column reference in the generated output must map to a node in the source
`ProcedureIr`. Unmapped elements are listed, and a proposal with any of them is **rejected**, not
flagged for review.

This check was vacuous before RFC 0163, because everything mapped to `Unmapped`. It has teeth now
precisely because there is a real node set with real spans to map against — which is the entire
reason for the ordering of these two RFCs.

**2. Fidelity gating.** A source object whose `ProcedureIr` fidelity is `Signature` or `Partial` is
**not eligible** for reconstruction. A model given a partial recovery will confidently fill the gaps,
and the anti-invention check cannot catch an invention that fills a hole the check cannot see. A
`Partial` object is reported with its unrecovered spans and requires a human — it is a finding, not
a proposal.

**3. Every proposal is a proposal.** `MigrationProposal` with `author: "llm"`, the model and prompt
hash recorded, status `proposed`. R3 approval with evidence review is required before it can be
generated into an artifact, per RFC 0161.

`DynamicExecute` nodes are never reconstructed. A constructed statement is not statically known, and
the honest output is a finding naming the construction site and its `USING` arguments.

### Differential execution — V5

The oracle. Both checks above are static; this one runs the code.

```
1. Build a fixture dataset per object: seeded deterministic rows, plus rows derived from the
   source IR's own branch conditions, plus boundary values from RFC 0157's profile (min, max,
   null, empty, the value at each range boundary the IR compares against).
2. Load the fixture into an ephemeral PostgreSQL schema and into a target sandbox.
3. Execute the source object and the migrated object over it.
4. Canonicalize both result sets (RFC 0155) and compare, order-insensitively unless the
   object declares an ORDER BY.
5. Record a MigrationValidationResult at tier V5, with per-row divergences where they exist.
```

Step 1 is what makes this more than a smoke test. Fixture rows are **derived from the IR's own
branch conditions**: if the source IR compares `qty < 0`, the fixture contains a negative quantity.
Branch coverage over the source IR is measured and reported — "V5 passed at 82% branch coverage" is
an honest result, and "V5 passed" alone is not one this system prints.

Objects with side effects (writes, `RAISE`, `NOTIFY`) run inside a transaction that is rolled back on
the PostgreSQL side and inside a disposable sandbox database on the target side, with written rows
compared as part of the output.

**Non-determinism is detected, not tolerated.** An object reading `now()`, `random()`, `clock_timestamp()`
or a sequence is identified from its IR; the harness pins what it can (a fixed transaction timestamp,
a seeded sequence) and reports what it cannot as an explicit V5 limitation on that object rather than
letting it produce flaky results that get retried until green.

### Triggers

Never auto-translated. RFC 0163 classifies them; this RFC proposes a redesign per class:

| Class | Proposed target shape |
|---|---|
| `Audit` | An explicit write in the load path, or target-side change data feed |
| `DerivedColumn` | A computed column in the target, or a transformation in the load |
| `Validation` | A DQ rule (RFC 0158) plus an application-side check — targets do not enforce |
| `Cascade` | Explicit downstream writes, ordered in the load |
| `Mixed` | Human decision required; no proposal generated |

Each is a proposal requiring a disposition, because every one of them moves *when* the logic runs.
A validation trigger becoming a DQ rule means bad rows now arrive and are detected afterwards
instead of being rejected on write — a real behavioural change that a human must accept explicitly.

## Phase 1 — what is built (2026-10-11)

A new crate, `ekos-migrate-logic`, and two commands. No model is called and no database is opened;
nothing is written to the ledger or the target.

| Piece | Where | What it does |
|---|---|---|
| View lowering | `lower.rs`, `ekos migrate lower` | A PostgreSQL `CREATE VIEW` becomes a ClickHouse `CREATE VIEW`, or a list of named blockers |
| Round trip | `lower.rs` | The emitted text is parsed with the ClickHouse grammar and lowered again; it must come back identical |
| Anti-invention check | `invention.rs`, `ekos migrate check-candidate` | A candidate is rejected when it states a table, column, literal, function or comparison its source does not |
| Fidelity gate | `gate.rs` | Only a routine at `Statements` fidelity with no dynamic SQL may be reconstructed |
| Trigger proposals | `triggers.rs` | One proposed shape per class, with the behaviour change it implies; `Mixed`/`Unknown` get none |

### Decisions that differ from the text above

- **The lowerer writes SQL through one walker, not through RFC 0160's builder.** That builder
  emits `CREATE TABLE` from a design; it has no query model. The rule it stood for is kept: every
  name goes through one quoting function, and nothing from the source is copied as text.
- **The walker is a whitelist.** Each `match` over the parsed statement ends in an arm that
  blocks. A construct with no rule stops the lowering by name (`LOGIC.CH.FUNCTION: string_agg()
  has no lowering rule`); it is never passed through.
- **The round trip is a fixed point, not an IR diff.** The source has no dataflow IR for a view
  yet (the first open item under Phase 6 in `TODO.md`). What is checked: the text parses under the
  target grammar, and the walker reading it as ClickHouse writes the same text. It found two
  defects on the first run: an escaped backtick the target grammar does not read, and type names
  the parser prints in another case.
- **Every lowering is also run through the anti-invention check** against its own source before
  it is shown. A rewrite that added a literal or a comparison would be blocked as
  `LOGIC.INVENTION`.
- **A materialized view is always a blocker.** The refreshable form needs a schedule and a target
  table, which is a design decision, so no statement is emitted for it.
- **Proposals are printed, not persisted.** `MigrationProposal` objects, their R3 approval and the
  completeness accounting are the next step; until then `ekos migrate lower --json` is the record.

### What the lowering rewrites, and why

Each row is a case where the same text runs on both engines and returns something else. All were
measured on ClickHouse 24.8 (`crates/migrate-logic/tests/clickhouse_semantics.sql`, live suite).

| PostgreSQL | ClickHouse default | Emitted |
|---|---|---|
| `sum`/`min`/`max` over no rows is `NULL` | `0` for a non-Nullable column | `sumOrNull` … |
| an outer join fills the missing side with `NULL` | with the type's default (`0`, `''`) | `SETTINGS join_use_nulls = 1` |
| a name in `WHERE` is the column | the `SELECT` alias of that name | `SETTINGS prefer_column_name_to_alias = 1` |
| `ORDER BY x DESC` puts `NULL` first | last | `DESC NULLS FIRST` |
| `UNION` removes duplicates | needs a setting | `UNION DISTINCT` |
| `CAST(NULL AS integer)` is `NULL` | an error | `CAST(… AS Nullable(Int32))` |
| `length` counts characters | bytes | `char_length` |
| unquoted names fold to lower case | names are case-sensitive | folded, then quoted |

Blocked, because the answer depends on something this layer cannot see or cannot keep: `/`
(integer or float division depends on operand types), `NOT IN` over anything but non-null
literals, `IS TRUE`/`IS FALSE`, `avg` (exact in PostgreSQL, float in ClickHouse), every function
without a rule, window functions, `DISTINCT ON`, CTEs, `EXCEPT`/`INTERSECT`, subqueries in
expressions, casts with a lossy or unmeasured mapping, view column lists.

### Measured on LedgerSMB

| | |
|---|---|
| Views | 17: 7 lowered, 10 blocked (7 on functions without a rule, 7 on query shape, 4 on a cast type, 3 on `/`) |
| Triggers | 42: 34 proposals, 8 left to a person (`Mixed`) |
| Routines | 556: 233 eligible, 21 ineligible (dynamic SQL or gaps), 302 not PL/pgSQL |

The 41% of views lowered is the honest floor for this rule set, not a target. The largest single
gain is function coverage (`string_agg`, `repeat`, `to_char`).

### Still open

- Constrained reconstruction (the LLM step) and its `MigrationProposal` records.
- V5 differential execution, fixtures from branch conditions, branch coverage.
- Lowering `ProcStmt::Sql`, SQL-language functions and set-returning functions.
- Persisting proposals and trigger dispositions; wiring them into approvals and the report.

## Testing

- Lowering round-trip: emitted SQL re-parses and re-lowers to an IR matching the source IR, over
  every view in both corpora.
- Anti-invention: a fixture proposal containing a predicate absent from the source IR is rejected;
  a faithful proposal passes. Both asserted, because a check that never rejects and a check that
  always rejects are equally useless.
- Fidelity gate: a `Partial` object cannot enter reconstruction, asserted by test.
- Differential: a deliberately wrong translation (inverted comparison, off-by-one boundary, missing
  `ELSE` branch) is caught by V5 on generated fixtures. This is the planted-defect discipline of
  RFC 0156 applied to logic, and it is required, not optional.
- Branch coverage: reported per object, and a floor enforced on the corpora as a ratchet.
- Non-determinism: an object using `now()` is flagged rather than producing an unstable result.
- Trigger classes: each produces its proposed shape; `Mixed` produces none.

## Alternatives considered

- **LLM translation without an IR constraint.** Rejected — it is the industry default and it is how
  migrations acquire silent behavioural changes. The whole RFC 0163 → 0164 ordering exists to avoid
  it.
- **Allowing reconstruction on `Partial` fidelity with a warning.** Rejected: the gaps are exactly
  where invention happens and exactly where the check is blind.
- **Differential execution on production data instead of fixtures.** Rejected: it puts production
  data through a sandbox, and it under-covers branches — real data is dominated by the common path,
  which is the path least likely to be wrong.
- **Trusting V5 alone and dropping the static check.** Rejected: fixtures cannot reach 100% branch
  coverage on real procedures, and the static check covers what the fixtures miss. They are
  complementary, which is why both are required.
- **Auto-translating audit triggers, as the "obviously safe" class.** Rejected: audit triggers are
  where compliance requirements live, and a silent change of when the audit row is written is
  precisely the kind of thing that must be someone's decision.

## Open questions

- [ ] Branch-coverage floor for V5 — what is achievable on LedgerSMB before the number is set?
- [ ] Should reconstruction be restricted to a local model (per the no-metered-calls constraint) or
      is a cloud model acceptable when the user explicitly opts in per run?
- [ ] For objects that cannot reach V5 at all (non-deterministic by nature), is a documented
      human-signed exemption the right exit, or do they block sign-off?

## Acceptance criteria

- [ ] Every view in both corpora lowers deterministically and round-trips. *(Every lowered view round-trips; LedgerSMB lowers 7 of 17, Pagila not run.)*
- [x] The anti-invention check rejects an invented predicate and accepts a faithful one.
- [ ] Reconstruction is impossible on `Signature` or `Partial` fidelity. *(The gate exists and `check-candidate` enforces it; there is no reconstruction path yet to enforce it in.)*
- [ ] Planted logic defects are caught by V5 on generated fixtures.
- [ ] Branch coverage is reported per object, with a ratcheted floor.
- [x] No trigger is auto-translated; each produces a classified proposal or a human-decision finding.

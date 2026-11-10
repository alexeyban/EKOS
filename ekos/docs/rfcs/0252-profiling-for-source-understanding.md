# RFC 0252 — Profiling for source understanding: what the data says when the code is silent

**Status:** Accepted (2026-11-10, owner's request to start work; built in order 0252 → 0253 → 0254)
**Date:** 2026-11-09
**Depends on:** RFC 0157 (live PostgreSQL profiling tiers, redaction at the live entry point), RFC 0211 (live
Oracle), RFC 0158 (inferred foreign keys, inclusion check), RFC 0170 (business semantics, `EnumMeaning`,
`SemanticGap`), RFC 0172 (`ConflictingEvidence`), RFC 0212 (write behaviour), RFC 0213 (review subjects,
sign-off), RFC 0029 (cross-system identity), RFC 0043 (redaction)
**Followed by:** RFC 0253 (people as a source), RFC 0254 (the analyst loop)

---

## 1. Motivation

EKOS recovers meaning from **traces**: DDL, views, routines, ETL, application code, documents. RFC 0170 states
the hard limit plainly — *no trace, no recovery*. The profiling that exists today (RFC 0157, 0211) was built for
migration: it answers "what type does this column need on the target" and "which column is a watermark".

A systems analyst preparing a source asks different questions, and most of them are answered by the data itself:

| Question | Today | What answers it |
|---|---|---|
| Which code values are actually used, and since when? | `EnumMeaning` says what code *mentions*; nothing says what data *holds* | per-literal counts, first/last seen |
| Which values does no code explain? | `SemanticGap` covers literals code uses without a meaning; values only data holds are invisible | the *unexplained mass* of a column |
| What does `status = 7` imply about the rest of the row? | nothing | conditional null rates |
| What is the grain of this table? | declared PK only | candidate (composite) keys |
| Which columns reference which, when no code joins them? | RFC 0158 candidates are seeded **only** by code joins (`infer::candidates(observations, …)`) | value-domain overlap |
| Does `cust_no` in the old system hold the same customers as `customer_id` in the new one? | RFC 0029 scores names and structure | value overlap across databases |

The target estate in the plan has an Informix database **with no source repository**. For it, code-seeded
inference produces nothing; the data is the only trace. This RFC makes the data a first-class trace, under the
same rules as every other one: aggregates only, redaction first, everything a hypothesis until a person decides.

## 2. Goals and non-goals

**Goals**

1. Five new profile signals (§3.2), all aggregates, computed in the same session and redaction discipline as
   RFC 0157.
2. **Data-seeded relationship discovery** (§3.3) beside the code-seeded one, verified by the RFC 0158 inclusion
   check.
3. **Cross-database value overlap** (§3.4) as a new evidence signal for RFC 0029 identity candidates.
4. Live sources beyond PostgreSQL and Oracle: SQL Server and Informix (§3.5).
5. Everything feeds existing review machinery: new `EnumMeaning` evidence, new `SemanticGap` kinds, new
   `ConflictingEvidence` types, one new RFC 0213 review subject (`relationships`).

**Non-goals**

- Persisting row values. Unchanged from RFC 0157: no sampled value ever reaches the ledger. The only literal
  values this RFC writes are ones **code already contains** (they are in the ledger already).
- Data quality monitoring over time (RFC 0196 monitors pipelines; this RFC profiles sources for understanding).
- Any LLM in profiling. Every signal is deterministic; LLM use starts in RFC 0254, on top of these facts.
- Replacing RFC 0157's tiers. P0/P1/P2 and their cost and approval rules apply unchanged; this RFC adds signals
  to tiers.

## 3. Design

### 3.1 One profile shape for every live source

`pg-live` and `ora-live` each produce a profile today. This RFC extracts the shared part into
`ekos-observation-sdk::profile` (pure types, no database):

```rust
pub trait LiveProfiler {
    fn dialect(&self) -> &str;
    fn catalog(&self) -> Result<LiveCatalog, LiveError>;            // tables, columns, keys, comments
    fn estimate(&self, sql: &str) -> Result<CostEstimate, LiveError>; // EXPLAIN, RFC 0157
    fn aggregate(&self, q: &AggregateQuery) -> Result<AggregateResult, LiveError>;
}
```

`AggregateQuery` is a **closed vocabulary** (count, count distinct, group-by-count with a limit, min/max,
null count, conditional count), rendered per dialect. A profiler never runs free SQL, so the read-only and
aggregates-only properties are structural, not a convention. Session safety (read-only role check, timeouts,
`application_name` with the run id, replica preference) stays per crate, as RFC 0157 and 0211 define it.

### 3.2 New signals

All signals land in the existing column/table profile facts as new properties. Each records the tier and the
sample size it was measured on; a sampled signal is labelled `estimated`, never presented as exact.

**(a) Literal coverage — P1.** Input: for each column, the literals the compiled estate mentions about it (RFC
0170 `PredicateSite`s, `CASE` branches, `check_constraints`, seed rows, Pentaho `FilterRows`). The profiler
receives these literals and counts, in process:

| Property | Meaning |
|---|---|
| `literal_counts` | rows per code literal (literal values are already in the ledger) |
| `unexplained_rows` | rows whose value no code literal names |
| `unexplained_distinct` | how many distinct such values (HLL) |
| `unexplained_top_k` | salted hashes with counts, **only** under RFC 0157's top-k rule (non-PII column) |

This is what turns "code mentions 1, 2, 3" into "code mentions 1, 2, 3; data also holds 5 and 9 in 4.1% of
rows". The *values* 5 and 9 are not persisted; RFC 0254 §3.2 reveals them on read, live, under read scope.

**(b) Temporal profile — P1.** When a table has a creation- or update-time column (a monotonic candidate from
RFC 0157, or a column a person marked), per code literal: `first_seen`, `last_seen`, bucketed to the month.
Output: **dead codes** (a literal not seen for longer than `[profile] dead_after`, default 24 months) and
**new codes** (first seen after the latest code that mentions the column — the code is behind the data).

**(c) Conditional profile — P1.** For each low-cardinality column `X` (≤ `[profile] conditional_max_distinct`,
default 32; never a PII column) and each nullable column `Y` of the same table: null rate of `Y` per value of
`X`. A value of `X` for which `Y` is never null (or always null) while it is mixed overall is recorded as an
**implication** (`status=7 ⇒ cancelled_at IS NOT NULL`, support and confidence). Bounded: at most
`conditional_max_pairs` per table, chosen by `X`'s code usage first. Implications become `ConstraintCandidate`
evidence (RFC 0170) and often name a meaning on their own ("7 is the state where `cancelled_at` is set").

**(d) Candidate keys and grain — P1 measures, P2 confirms.** Single columns with duplicate rate 0 on the sample
are already measured (RFC 0157). This adds composite candidates: up to `[profile] key_max_width` (default 3)
columns, searched only among non-null, high-distinct columns plus columns joined by code, levelwise with
pruning (a superset of a key is never tested). A sample key is `estimated`; P2 confirms it exactly, under the
RFC 0157 approval rule. Output: `GrainCandidate` (new keyed kind) per table — the analyst's first question about
any table.

**(e) History and deletion patterns — P1.** Detects `valid_from`/`valid_to` pairs (named or by pattern: two
date columns, the second null on a subset that is unique on the business key), overlapping validity periods
for the same key, and soft-delete flags (a boolean or code column with a value whose rows code filters out
everywhere). Feeds RFC 0212 `WriteBehaviour` as additional evidence; it does not create a second write-behaviour
model.

### 3.3 Data-seeded relationship discovery

RFC 0158's candidates come from code joins. This adds a second seed for columns no code joins:

1. **Parents.** Columns that are unique (declared, or single-column key from §3.2(d)), not PII, not boolean,
   distinct ≥ `[relationships] min_parent_distinct` (default 20).
2. **Children.** Columns whose type family matches a parent's (integer, text-ish, uuid, date), whose distinct
   count ≤ the parent's and whose min/max fall inside the parent's.
3. **Overlap.** Per parent and child, a MinHash signature of salted value hashes is computed in process during
   P1 (RFC 0157's project salt). LSH banding proposes pairs, so the search is not quadratic in the number of
   columns. Estimated containment ≥ `[relationships] min_containment` (default 0.9) makes a candidate.
4. **Verification.** Every candidate in one database is then checked exactly by RFC 0158's existing inclusion
   SQL (`orphan_sql`, `population_sql`), and gets RFC 0158's verdict (`CleanRelationship`, `RelationshipWithOrphans`, `Unclear`, `NotARelationship`, `NoData`).
5. **Names are a tie-breaker, never a seed.** Name affinity (token match, `_id`/`_no` suffixes) raises the
   score; a name-only pair with no overlap is never a candidate.

Each candidate is the existing `FkCandidate` with a new `seed: code | data | both` field and the evidence that
produced it (join sites for `code`, the overlap estimate and the inclusion counts for `data`). A pair seeded by
both is the strongest evidence EKOS has and is ranked first.

Signatures are persisted **only** for parent and child columns that pass the filters above (key-like,
non-PII). A MinHash of salted hashes of a high-cardinality, non-personal key does not allow enumeration;
low-cardinality and PII columns get none — the same reasoning as RFC 0157's top-k rule, and the same ledger-scan
test.

### 3.4 Cross-database overlap

When a project has several live databases (RFC 0223), signatures from §3.3 are compared **across** databases
with the same LSH. A cross-database pair cannot be verified by a join, so it never becomes an `FkCandidate`; it
becomes a new signal on RFC 0029's cross-system `SameAs` candidates: `value_overlap` (estimated containment
both ways, sample sizes, signature run ids). Two columns with different names and 97% overlap in both directions
are a far better identity candidate than any name score — and they still only become a reviewable `unconfirmed`
relationship, never a merge.

This needs the same project salt on both sides, which RFC 0157's project-scoped salt already provides.

### 3.5 New live sources

| Source | Catalog | Usage evidence | Driver |
|---|---|---|---|
| SQL Server | `sys.tables`, `sys.columns`, `sys.foreign_keys`, `sys.indexes`, extended properties for comments, `sys.sql_modules` for routine text | Query Store (`sys.query_store_query_text`, runtime stats) when on, else `sys.dm_exec_query_stats` | `tiberius` |
| Informix | `systables`, `syscolumns`, `sysconstraints`, `sysindexes`, `sysprocedures`/`sysprocbody` for routine text, `sysviews` | `sysmaster` (`syssessions`, `syssqlstat` where enabled) | ODBC through `odbc-api` and the Informix CSDK; the plugin is feature-gated like `oracle` |

Routine and view text recovered from the catalog goes through the existing recovery path, so an Informix
database with no repository still yields Transformation IR, predicates and therefore code literals for §3.2(a).
Informix SPL needs a dialect plugin (`sql-dialect-informix`); until it exists, routine text is kept as
`Procedure` objects without IR, and the coverage gap is reported (RFC 0154 completeness).

Credentials, keychain and `ekos database add|test` are RFC 0223's, unchanged.

### 3.6 Where results go

| Signal | Becomes | Reviewed in |
|---|---|---|
| literal counts, first/last seen | evidence on `EnumMeaning`: `rows`, `share`, `first_seen`, `last_seen`, `dead` | `glossary` (RFC 0213) |
| unexplained mass | `SemanticGap` with `gap_type: data_only_values` | `glossary`; questions in RFC 0254 |
| implications | evidence on `ConstraintCandidate`; may seed an `EnumMeaning` | `glossary` |
| grain candidates | `GrainCandidate` (keyed) | `scope` facts shown per table |
| history/deletion patterns | evidence on `WriteBehaviour` | `write-behaviour` |
| data-seeded FKs | `FkCandidate` with `seed` | **new subject `relationships`** |
| cross-database overlap | `value_overlap` on `SameAs` candidates | `identity` |
| code literal never seen in data | `ConflictingEvidence`, type `code_vs_data` (RFC 0254 §3.4) | `conflicts` |

The new review subject `relationships` follows RFC 0213 exactly: items are inferred FKs not declared in the
schema; signed facts are the pair, direction and verdict; counts are shown-only `info` (they change with every
profile and must not stale the sign-off). Decisions: `confirm` / `reject` / `confirm-with-orphans` (a real relationship
with orphans the target must tolerate). Human-only, listed in `review_guard::HUMAN_ONLY`.

### 3.7 Surfaces

- CLI: `ekos profile run [--database <db>] [--tier P1] [--signals literal,temporal,conditional,keys,history]`,
  `ekos relationships discover [--cross-database]`, `ekos review relationships`.
- MCP: read-only `ekos_profile` (a table's or column's profile with the new signals; in `SCOPED_TOOLS`), and
  `ekos_relationships` (candidates with seed, verdict and status). No write tool.
- Console: the table page shows grain, code usage over time (a small chart from the monthly buckets), dead and
  unexplained codes, and inferred relationships with their seed.
- Docs: RFC 0214's per-table page gains a "What the data says" section (RFC 0254 §3.5).

## 4. Decisions

| Decision | Alternative rejected | Why |
|---|---|---|
| Literals come from code, counts from data | Persist the top values of every column | The ledger is append-only; values code already contains are the only ones safe to write |
| MinHash + LSH for overlap | Pairwise inclusion SQL for all type-compatible pairs | Quadratic in columns and costly on the source; the inclusion SQL is kept for verification only |
| Cross-database overlap is identity evidence, not an FK | An FK across databases | It cannot be verified by a join; RFC 0029 is where unverifiable cross-system links already live |
| Closed `AggregateQuery` vocabulary | Free SQL from the profiler | Read-only and aggregates-only become properties of the type |
| Signatures only for key-like non-PII columns | Signatures everywhere | Same enumeration argument as RFC 0157's top-k |
| Counts are `info`, not signed facts | Signing counts | RFC 0213: a sign-off that stales on every re-profile teaches people to ignore staleness |

## 5. Tests (written first)

- Literal coverage on a fixture with code literals `1, 2, 3` and data holding `1, 2, 3, 5, 9`: counts per
  literal, unexplained rows and distinct are right; the ledger scan finds neither `5` nor `9`.
- Temporal: a literal last written 30 months ago is `dead`; a value first seen after the last commit touching
  the column produces a `data_only_values` gap marked `new`.
- Conditional: `status=7 ⇒ cancelled_at IS NOT NULL` found with support and confidence; a PII column is never an
  `X`; the pair cap holds.
- Keys: a composite key `(order_id, line_no)` found; no superset of it is tested (counter on queries).
- Discovery: a fixture with an undeclared, unjoined `invoice.cust_ref → customer.id` is found with `seed: data`
  and verdict `CleanRelationship`; a same-named pair with no overlap is not a candidate; a pair joined in code **and**
  overlapping is `seed: both`.
- Cross-database: two fixture databases sharing a customer key under different names yield a `SameAs`
  candidate with `value_overlap`; nothing is merged.
- Privacy: after a full P1 with all signals on a fixture seeded with recognizable values, the RFC 0157 ledger
  scan finds no source value; no signature exists for a PII or low-cardinality column.
- Review: `relationships` subject digest stales when a verdict changes and does **not** stale when only counts
  change; `review relationships` is unreachable from MCP (`HUMAN_ONLY` scan).
- Determinism: shuffled catalog order gives identical objects (reversed-input test, as RFC 0170).
- Live SQL Server and Informix fixtures (Docker images exist for both) round-trip catalog counts.

## 6. Phases

1. `LiveProfiler` seam over `pg-live` and `ora-live`; signals (a), (b), (d); `EnumMeaning` evidence and
   `data_only_values` gaps. Measured on LedgerSMB and iDempiere.
2. Signals (c), (e); data-seeded discovery (§3.3); `relationships` review subject.
3. SQL Server live source.
4. Informix live source (catalog, routine text, profiling); cross-database overlap (§3.4).

## 7. Open questions

1. Should a person be able to mark a column as the time axis for §3.2(b) when no monotonic column exists — and
   is that an RFC 0247 `instruction` (`time_axis` rule) rather than a new mechanism? Proposed: yes, an
   instruction.
2. LSH parameters (bands × rows) trade recall against candidates to verify. Measure on iDempiere (903 tables)
   before fixing defaults.
3. Informix through ODBC needs the IBM CSDK at runtime, which complicates the release image (RFC 0179). Ship it
   as a separate image variant, like Oracle?
4. Is `dead_after` a project setting or per table class (RFC 0213 classes)?

## 8. Acceptance criteria

- [ ] On LedgerSMB, every `EnumMeaning` has row share and last-seen; at least one dead code and one
      data-only value are reported and checked by hand.
- [ ] On iDempiere, data-seeded discovery finds relationships the code-seeded pass did not, with a measured
      precision (confirmed / proposed) reported in the devlog.
- [ ] A database with no repository (Informix fixture) produces grain, relationships and code usage from data
      and catalog alone.
- [ ] Ledger scan after all signals: zero source values, zero credentials.
- [ ] BIRD Mini-Dev re-run with the new signals in the EKOS pack (RFC 0240/0241), reported against the current
      pack on the hold split.

## 9. Implementation log

### 2026-11-10 — §3.1: one profile shape for every live source

Built first, at the owner's request ("profiling for all supporting databases"): the RFC 0211 §3.4 tiers
for **every** `[[databases]]` kind EKOS reads today — Oracle, PostgreSQL, ClickHouse. SQL Server and
Informix arrive with their live sources (§6 phases 3–4).

| Part | What |
|---|---|
| `observation-sdk::profile` | `ProfileTier`, `ProfileStats`, `Flavor`; `family` per engine; `plan_table` renders a closed set of aggregates (row count, non-null count, number/date bounds, longest text) per dialect; `TablePlan::apply` writes `detail.profile` and each column's `profile`. Pure; the "never a text value, never a personal bound" rule is tested on the SQL of all three dialects |
| Oracle (`ora-live::profile::scan`) | now plans through the shared module; same SQL as before, same output |
| PostgreSQL (`plugins/postgres/src/profile.rs`) | **P0 is new**: `pg_class.reltuples` and `pg_stats` counts (null fraction, `n_distinct`, width — never `most_common_vals` or `histogram_bounds`); P1/P2 through the shared planner on pg-live's read-only session |
| ClickHouse (`plugins/clickhouse/src/catalog.rs`) | P0 from `system.tables.total_rows` (not exact); P1/P2 with `readonly=1` and `max_execution_time`; **only engines that store their own rows** (`*MergeTree`, `Memory`, `Log`, `TinyLog`, `StripeLog`) — a stream engine consumes what a `SELECT` reads, an integration engine scans another system |
| config, console | `profile` accepted for every kind (RFC 0235's refusal lifted; `dictionary` stays Oracle-only); the Sources form offers the tier for every kind |

**Departure from §3.1 as drafted.** No `LiveProfiler` trait: the shared part is a pure plan/apply
pair, and each engine runs the planned text its own way (Oracle and PostgreSQL synchronously, ClickHouse
over async HTTP). A trait would have forced one execution model on all three; the plan is what carries
the safety rules, and it is shared. `catalog()` stays with the existing observers.

**Measured** on the repo's test servers (`docker-compose.migrate.yml`), `profile = "p2"`, build →
commit: 42 tables profiled (PostgreSQL `shop` + `public`: 11; ClickHouse `lsmb_raw`: 31, 303 columns
measured); row counts, null counts and bounds equal to direct queries on both servers; a text value
from each source searched for in the ledger and in `.ekos/`: not found.

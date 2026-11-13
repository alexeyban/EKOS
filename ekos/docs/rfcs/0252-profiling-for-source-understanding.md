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

### 2026-11-10 — §3.5 / Phase 3: SQL Server as a live source

`[[databases]] kind = "sqlserver"` (`connect = "host[:port]/database[?trust-server-certificate=true]"`,
`owners` = schemas), built as the fourth kind of RFC 0235's live-catalog shape, so the shared catalog
pass, `ekos database add|test`, `ekos sql check --database` and the console's Sources form needed only
a new arm each.

| Part | What |
|---|---|
| `plugins/sqlserver` (`ekos-plugin-sqlserver`) | `tiberius` 0.13 (`tds80`, `rustls`, `chrono`; no OpenSSL, no Windows auth). `catalog.rs`: ten constant queries over `sys.objects`, `sys.tables`, `sys.columns` + `sys.types`, `sys.indexes`/`sys.index_columns` (PK, UQ and unfiltered unique indexes), `sys.check_constraints`, `sys.foreign_keys`/`sys.foreign_key_columns`, `sys.sql_modules`, `sys.parameters`, `sys.trigger_events`, `sys.extended_properties` (`MS_Description`); a pure `normalize` into `OwnerCatalog`s, redacted; `reconcile` refuses a catalog shorter than `sys.objects`' count of tables, views, procedures, functions and triggers |
| read-only session (`session.rs`) | see *Decision* below |
| profile (`profile.rs`) | P0 = `sys.partitions.rows` (approximate, `row_count_exact: false`; readable with metadata visibility, unlike `sys.dm_db_partition_stats`); column statistics are histograms of values and are never read. P1/P2 through `profile_table(Flavor::SqlServer, …)` |
| `observation-sdk::profile` | `Flavor::SqlServer`: `[bracket]` identifiers (`]` doubled), `COUNT_BIG`, `CONVERT(nvarchar(100), MIN/MAX(c), 126)` bounds, `MAX(LEN(c))`; `text`/`ntext`/`image`, `uniqueidentifier`, `bit`, binary, `xml` counted only. The no-text-value / no-PII-bound test covers it |
| probe (`probe.rs`) | one `SELECT` (`single_select`, T-SQL grammar); `SET ROWCOUNT n+1` instead of `wrap_rows`' `TOP (n) * FROM (…)`, because T-SQL refuses an `ORDER BY` inside a derived table; a login that can write is refused unless `--allow-writable-role` |
| recovery | `CatalogDialect::SqlServer` (`SqlServerCatalog` keyed kind, diagnostics `MSLIVE…`); `tsql_routines.rs`: views re-headed from the `sys.sql_modules` text; routines and triggers at **signature** fidelity (no T-SQL procedural front end), footprint from a real `MsSqlDialect` parse — whole statement, else body, else `;` fragments with session options, block keywords, `RETURN`/`SET @v =`/`DECLARE … =` values handled — and `inserted`/`deleted` dropped from a trigger's relations; a trigger is a `Procedure` body plus a `Trigger` binding, Oracle's shape |
| cli | build observer, `database add|test` (census vs compiled; an Oracle or T-SQL trigger body is counted with the triggers, not the routines — a latent Oracle mismatch too), `sql check` (dialect `mssql`), recover summary; `tiberius=error` in the default log filter (its per-connection warning about a trusted certificate landed on stdout and broke `database test --json`) |
| console | `sqlserver` in `KINDS` / `DatabaseIn.kind` / the Sources form |

**Decision: read-only without a read-only flag.** SQL Server has no equivalent of PostgreSQL's
`default_transaction_read_only` or ClickHouse's `readonly=1`. Four locks instead, each sufficient for the
common case: (1) `ApplicationIntent=ReadOnly` at login — routed to a readable secondary in an
Availability Group, no effect on a standalone server; (2) every statement inside `BEGIN TRANSACTION` …
`IF @@TRANCOUNT > 0 ROLLBACK` with `XACT_ABORT ON` — even a statement that wrote leaves nothing (the live
suite inserts as `sa` and counts the same rows after); (3) only planned text is sent — catalog
constants, the closed profile vocabulary, the single-`SELECT` guard for probes; (4) the login's rights
are reported. Plus `SET LOCK_TIMEOUT 10000`, `DEADLOCK_PRIORITY LOW`, a 120 s command timeout (a timed-out
session is out of step in tiberius, so it is treated as lost and the remaining tables are skipped, then
reconnected per schema).

**Departures.** (a) The write check reads permissions (`HAS_PERMS_BY_NAME` INSERT/UPDATE/DELETE on any
listed table, ALTER on any listed schema, `DATABASEPROPERTYEX(…, 'Updateability')`) instead of attempting
a refused write: an attempted `INSERT … SELECT TOP 0` fails on identity columns for reasons unrelated to
rights and would read as "refused". The JSON shape is unchanged (`result`), with `method: "permissions"`.
(b) §3.5's usage evidence (Query Store, `sys.dm_exec_query_stats`) is not built — it feeds signals of
Phases 1–2 that do not exist yet. (c) T-SQL routines have no `ProcedureStatement` objects, so RFC 0212
write behaviour and statement-level lineage do not see their writes; routine → table `DependsOn` does.
(d) A named instance is reached by its port (no SQL Browser).

**Measured** against `docker-compose.sqlserver.yml` (SQL Server 2022 Developer, `shop` fixture: schemas
`sales` + `hr`, 6 tables, 4 foreign keys, 1 view, 1 procedure, 1 function, 1 trigger), `profile = "p2"`,
login `ekos_ro` (`db_datareader` + `VIEW DEFINITION`), build → recover → resolve → compile → commit on a
scratch project: 6 tables, 4 foreign keys (0 unresolved), 1 view (0 unread), 3 routines + 1 trigger;
`ekos database test`: census = compiled for tables (6), views (1), routines (2), triggers (1); write
check `refused_by_server`; 28 columns = `sys.columns`; every row count (customers 3, orders 4,
order_lines 4, order_log 4, products 3, employees 2), null count, `MAX(LEN)` and date/number bound equal
to direct `sqlcmd` queries; `birth_date` bounds withheld (`date_of_birth`); cross-references 100 %
resolved (6 routine → table `DependsOn`); `ekos sql check --database` previews a joined, ordered query.
Six fixture values (an e-mail, a product title, a note, a birth date, a name, the login's password)
searched in every file under `.ekos/` (zstd decompressed): not found.

### 2026-11-10 — §3.5 / Phase 4 (Informix): Informix as a live source

`[[databases]] kind = "informix"` (`connect = "host[:port]/database@server"` — the IBM Informix Client
SDK over SQLI — or `drda://host[:port]/database` — the IBM Data Server Driver over DRDA; optional
`?driver=` and `?db-locale=`; `owners` = table owners), the fifth kind of RFC 0235's live-catalog shape.

| Part | What |
|---|---|
| `plugins/informix` (`ekos-plugin-informix`) | `odbc-api` 30 (no `prompt`/winit) behind the crate feature `informix`, passed through by `ekos`'s feature `informix` (off by default, like `oracle`). `catalog.rs`: 19 constant queries over `systables`, `syscolumns` (+ `sysxtdtypes`, `sysdefaults`), `sysconstraints`/`sysindexes` (PK, UQ and unique indexes no constraint stands on, columns from `part1`…`part16`), `syschecks`, `sysreferences` (+ the referenced tables' columns, whatever their owner), `sysviews`, `sysprocedures`/`sysprocbody` (`T` text, `D` = `DOCUMENT` → comment; SPL only, `internal = 'f'`), `systriggers`/`systrigbody`; types decoded from `coltype`/`collength` (decimal/money precision, varchar max/min, `datetime`/`interval` qualifiers, serials, extended types by name); text pieces joined at their **declared** width, read from `syscolumns` (`sysviews.viewtext` is 64 in older servers, 256 in 15.0), so a blank at a boundary survives a driver that trims `CHAR`; overloaded routines named `name#procid`; a pure `normalize`, redacted; `reconcile` refuses a catalog shorter than the census |
| read-only session (`session.rs`) | see *Decision* |
| profile (`profile.rs`) | P0 = `systables.nrows` (`row_count_exact: false`); `colmin`/`colmax`/`sysdistrib` are values and never read (a test scans every query). P1/P2 through `profile_table(Flavor::Informix, …)` |
| `observation-sdk` | `Flavor::Informix`: `"…"` identifiers (doubled), `COUNT`, number bounds `CAST(MIN(c) AS LVARCHAR(100))`, date bounds `CAST(EXTEND(MIN(c), YEAR TO FRACTION(5)) AS LVARCHAR(100))` (a `DATE` would otherwise render in the client's `DBDATE` order), `MAX(LENGTH(c))`; only a `datetime year to …` is a date; `text`/`byte`/`blob`/`clob`/`boolean`/`interval` counted only. `signals`: `CAST(c AS LVARCHAR(4000))`, `TO_CHAR(x, '%Y-%m')`, `GROUP BY 1`, a typed `CAST(NULL AS CHAR(7))`. `overlap`: `signature_sql` returns `Err("not supported on Informix …")` — no MD5 built-in (`ifx_checksum` is a CRC no other engine has); `inclusion_sql` works |
| probe (`probe.rs`) | one `SELECT` (`single_select`, generic dialect — there is no Informix grammar, so Informix-only syntax is refused, not sent unchecked); no row-limit wrapper (Informix refuses `ORDER BY` in a derived table): the session stops fetching at `max_rows + 1`; a user that can write is refused unless `--allow-writable-role` |
| recovery | `CatalogDialect::Informix` (`InformixCatalog` keyed kind, diagnostics `IFXLIVE…`); `spl_routines.rs`: views re-headed from the `sysviews` text and read in the generic dialect; SPL routines as `Procedure` **without IR** (`ir: false`, `fidelity: signature`, `coverage_gap`), arguments and `RETURNING` types from the header, footprint from each `;` statement with SPL's words taken off (`IF … THEN`, `FOREACH [c FOR]`, a cursor `SELECT` running into the loop's first statement, `LET v =`, `RETURN`, `SELECT … INTO vars`, `EXECUTE PROCEDURE` → `calls`); a trigger is a `Procedure` (its action list, split at commas that start a statement) plus a `Trigger` binding. `IFXLIVE003` and the recover summary count the routines kept without IR (RFC 0154 completeness) |
| cli | build observer, `database add|test` (an Informix trigger body is counted with the triggers), `sql check` (dialect `generic`), `profile`/`relationships` (`Flavor::Informix`), recover summary |
| console | `informix` in `KINDS` / `DatabaseIn.kind` / the Sources form; the form refuses `?driver=` (a shared library to load) — `ekos.toml` only |
| testbed | `docker-compose.informix.yml` (developer edition 15.0, SQLI 59088 / DRDA 59089 on loopback) + `deploy/fixtures/informix/{shop.sql,load.sh}`; live suite `EKOS_INFORMIX_LIVE=1` |

**Decision: read-only without a read-only login.** (1) Autocommit off and a rollback after every
statement; (2) every transaction opens with `SET TRANSACTION READ ONLY` — the server refuses a write
with -878, measured: the live suite's `INSERT` as the DBA is refused and the row count is unchanged;
(3) only planned text is sent; (4) the write check reads the user's rights — owner of a listed table,
`U`/`I`/`D`/`A` in `systabauth.tabauth` or `U` in `syscolauth`, directly, as `public` or through a role,
or `RESOURCE`/`DBA` in `sysusers` — with `method: "permissions"` and the session's locks
(`transactions_rolled_back`, `read_only_transactions`, `isolation`, `delimited_identifiers`). A
database created without logging has no transactions: the check then carries a `note` saying the
rollback lock is absent. `COMMITTED READ LAST COMMITTED` (else `DIRTY READ`) so a reader never waits on
a writer; `SET LOCK MODE TO WAIT 10`; a 120 s query timeout (a timed-out session is lost and the rest of
its owner's tables are skipped, then reconnected per owner). `SET ENVIRONMENT DELIMIDENT 'on'` (the
value `'y'` is refused by the server) makes `"name"` an identifier over DRDA too, where the CSDK's
`DELIMIDENT=y` connection keyword does not reach.

**Departures.** (a) Two drivers, not one: §3.5 names the CSDK, which needs an IBM account to download,
so it was not used here; the IBM Data Server Driver (public download, no account) reaches the same
server over DRDA and runs the same Informix SQL, so it is supported and is what the live suite ran
through. The CSDK path (`host:port/db@server`, `DELIMIDENT=y`, `CLIENT_LOCALE`) is unit-tested for its
connection string only — **not run live**. (b) That driver's `libdb2.so` uses a 32-bit `SQLLEN` under
unixODBC (IBM's 64-bit `libdb2o.so` is not in the free package), which `odbc-api`'s own buffers read as
a negative length and panic on. The session therefore fetches one row at a time into buffers whose
indicators start at 0 and rebinds before each fetch; a 32-bit `-1`/`-4` then reads as `u32::MAX` /
`u32::MAX - 3` (`session::reported`), and a 64-bit driver reads as before. Slower per row, correct for
both. (c) §3.5's usage evidence (`sysmaster:syssessions`, `syssqlstat`) is not built, as for SQL
Server. (d) No `sql-dialect-informix`: routine bodies have no IR, as the RFC foresaw, and the generic
dialect reads views and statements; Informix-only syntax in them (`OUTER t`, `MATCHES`, `FIRST n`) is
counted as unparsed, not guessed. (e) No overlap signature for Informix columns (no MD5); references
into Informix are still verified exactly by `inclusion_sql` when another seed proposes them.

**Measured** against `docker-compose.informix.yml` (Informix 15.0.1.0.3 Developer Edition, `shop`
fixture: owners `sales` + `hr`, 6 tables, 4 foreign keys (one cascading, one self-reference), 2 checks,
a unique index, 1 view, 1 SPL procedure with `DOCUMENT`, 1 SPL function, 1 trigger), through the IBM
Data Server Driver (v12.1 `clidriver`) over DRDA, user `ekos_ro` (OS user; `CONNECT` + `SELECT`, the
`PUBLIC` grants revoked), `profile = "p2"`, build → recover → resolve → compile → commit on a scratch
project: 6 tables, 4 foreign keys (0 unresolved), 1 view (0 unread), 3 routines (2 SPL + the trigger's
action) + 1 trigger, all without IR (`IFXLIVE003`); CKM 13 objects; 9 routine/view → table `DependsOn`;
cross-references 100 % resolved. `ekos database test`: census = compiled for tables (6), views (1),
routines (2), triggers (1); 28 columns; write check `refused_by_server` for `ekos_ro`, `user_can_write`
(`dba: true`) for `informix`. Live suite (3 tests): every row count (customers 3, orders 4, order_lines
4, products 3), the `note` null count and `MAX(LENGTH)`, the `placed_at` and `hired_on` bounds equal to
direct queries; `birth_date` bounds withheld; an `INSERT` in the read-only session refused (-878),
count unchanged; the probe previews a joined, ordered query (1 row, truncated) and refuses a `DELETE`
and a DBA user. `ekos profile run`: 6 tables, grain 5 declared + 1 found by key search; the coded
column `orders.status` (`CHECK (status IN (1, 2, 9))`) measured 1 → 3 rows, 2 → 1, 9 → 0 (never seen),
equal to the data. `ekos sql check --database` previews a joined query read-only. Every catalog,
profile, signal and inclusion query was also run as `ekos_ro` through `dbaccess` (SQLI) without error.
Eleven fixture values (two e-mails, a product title, two notes, a birth date, three names, a product
code, the user's password) searched in every file under `.ekos/` (149 files, zstd decompressed): not
found.

### 2026-11-10 — §3.2 signals, §3.3–3.4 discovery, §3.6 review subject, §3.7 surfaces

| Part | What |
|---|---|
| `observation-sdk::signals` | (a) literal coverage: one `GROUP BY` per coded column whose value cell is `CASE WHEN text(col) IN (<code literals>) THEN text(col) END` — a data value never leaves the database, only a code literal or `NULL` ("everything else"); unexplained distinct count. (b) first/last month per literal on a time column chosen by name (creation before update; birth dates excluded). (c) `column = v ⇒ other IS [NOT] NULL` per code value, support ≥ 5, only where the other column is mixed. (d) levelwise key search (≤ 3 columns, ≤ 24 queries per table, a superset of a key never tested). (e) validity periods (`valid_from`/`valid_to` and kin, by name) and soft-delete flags (counted against EKOS's own literal set `Y/1/true/…`). Rendered for Oracle, PostgreSQL, ClickHouse, SQL Server, Informix |
| `ekos profile run` | per `[[databases]]` entry with p1/p2, through `ekos sql check`'s read-only probe session (a writable role refused unless `--allow-writable-role`); `<state>/profile/current.json`; keyed `GrainCandidate` per table; history onto `WriteBehaviour.data_history`; then the semantics step and the RFC 0254 questions step. `ekos profile show [TABLE]` |
| `semantic::data_usage` | `EnumMeaning.data_usage` (rows, share, first/last seen, `dead`, `never_seen` per database) and `implies`; one `data_only_values` gap per column holding unexplained values (counts only). Neither is a signed field and the gap's evidence names the measurement, not its counts: a re-profile never stales a review (tested) |
| `observation-sdk::overlap` + `ekos relationships discover` | MinHash computed **by the database**: 4 salted MD5 digests per distinct value, 16 32-bit-slice minimums (plus the distinct count and, for whole numbers, the span) are all that leave it; the salt is `<state>/profile/salt` (0600, never in the ledger). LSH bands (2 rows) propose; containment ≥ 0.9 in a unique parent keeps; every code join (RFC 0158's harvester) is added; each pair checked exactly by an inclusion `LEFT JOIN` on text. Keyed `FkCandidate` with `seed: code | data | both` and the RFC 0158 verdict. `--cross-database`: key overlap both ways becomes `value_overlap` on a `SameAs` candidate (never a merge) |
| review | optional subject `relationships` (listed and required only once a discovery produced items): signed child/parent/seed/verdict, counts shown only; `ekos review relationships --confirm \| --reject \| --confirm-with-orphans` (human-only) |
| MCP | `ekos_profile`, `ekos_relationships` — read-only, in `SCOPED_TOOLS` (the table is found through the scoped store; file entries only for tables it returned) |

**Departures.** (a) Signatures come from the database, not from values read into the process: no
value crosses the wire, and MD5 is defined identically on every engine but Informix, so cross-database
comparison works (Informix has none; its columns are not signed). (b) Names are still a tie-breaker
for data seeds — but required in two cases measured live: a column that is itself a key, and any
parent that is a *dense id sequence* (distinct ≥ 0.8 × span), because every small whole number is
"contained" in one. Without that rule LedgerSMB produced ~260 candidates such as `oe.id → payment.id`;
with it, 9 per run. Generic names (`id`, `code`, `name`, …) never count as a match. Cross-database
overlaps between few-value or dense columns need the same table name. (c) "new codes" (first seen after
the code's last change) is not built: it needs per-literal git dates. (d) The sample/`estimated`
labels are unused: every signal here is measured exactly on the table; `--max-rows` (default 10 M,
by the catalog's estimate) is the cost guard instead of EXPLAIN. (e) Phase 1's `LiveProfiler` trait
stays a departure (see the §3.1 entry).

**Measured** on LedgerSMB (`sql/` of the source repo, `[semantics]` on) against the repo's test
servers: PostgreSQL `ledgersmb` and its ClickHouse copy `lsmb_raw`, profile p1. `ekos profile run`
(80 s): 70 coded columns, 199 tables (158 declared keys, 32 found by search, 9 without), 228 code
values measured — 167 never seen in this data, 0 dead — and 3 columns holding values no code names
(`account_link.description`: 28 of 36 rows, 13 values; `defaults.value`); every count equal to a
direct query (e.g. `account.category` A 9, E 30, I 4, L 21, Q 2). Implications: 4, e.g.
`entity_credit_account.entity_class = 1 ⇒ business_id IS NULL` (25 rows) and `= 2 ⇒ IS NOT NULL` (60),
equal to the data. History: 19 patterns (`entity_credit_account.startdate/enddate`: 85 of 85 open).
`ekos relationships discover --cross-database` (73 s): 338 columns signed, 9 candidates (e.g.
`invoice.parts_id → partstax.parts_id` clean over 11,130 rows; `acc_trans.trans_id → gl.id` from a code
join, `not_a_relationship`, 43,200 orphans — the join is to the transactions table), and 30 cross-
database `SameAs` overlaps, 28 of them the same table on both engines, plus `ar = invoice`
(`trans_id`) and `ar = open_item` (`invnumber` = `item_number`).

### 2026-11-12 — leftovers: new codes, per-month usage, usage evidence, the Informix CSDK path

| Part | What |
|---|---|
| new codes (§3.2(b)) | `RationaleSource::introduced(path, line, literal)` (default `None`, so other implementations keep compiling); `GitBlame` answers it with `git log -L<line>,<line>:<file>` — the oldest commit of the unbroken run of the line's versions, from the newest, whose text still names the literal (`names_literal`: a quoted literal verbatim, anything else as a whole token) — summary redacted, excluded paths never read, answers memoised per run. `semantic::data_usage::date_codes` runs after `fold` when a rationale source exists: each value the data holds with a first month gets `code_history` (introduced, sha, summary, path, line, `data_first_seen`, `order` = `data_before_code` / `same_month` / `code_before_data`), tracing at most 3 file lines per value and 400 values per run; each `data_only_values` gap gets `new` and `code_last_changed` (the latest `git blame` over every line citing the column). Neither is a signed field (tested). Stats `data_before_code`, `new_value_columns`; `profile run` prints them; `profile show` adds "code since … — the data held it first"; the passport's Codes column says "new: …" |
| per-month usage | `signals::literal_monthly_sql` (value cell a code literal or `NULL`, grouped by month in the database, all five dialects; Informix `GROUP BY 1, 2`), `parse_monthly` (latest 120 months per value; a month with fewer than `[questions] min-cell` rows kept as `rows: null`). `Coverage.monthly` in `<state>/profile/current.json` — never in the ledger. New signal name `monthly`. `profile show` draws the last 24 months per value as a sparkline (`·` = a withheld month); `ekos_profile` returns it; console: `GET /api/workspaces/{id}/profile?table=` (`ekos profile show --json <table>`, `read`; readproc allows one table-name positional) and a "Usage over time" bar chart on an `EnumMeaning` / value gap / data-only gap in the Semantics panel, with the month the code first named the value marked |
| usage evidence (§3.5) | `observation-sdk::usage`, pure: per source one aggregate query in which table names are matched against statement text **inside the database** (`LIKE` / regex with the catalog's names) — only names from the query's own list, a fixed statement-kind vocabulary, counts and a month come back. SQL Server: Query Store (when `actual_state` is READ_ONLY/READ_WRITE), else `sys.dm_exec_query_stats` (`dbid` = `DB_ID()`); Informix: `sysmaster:syssqltrace` (tracing at `med` or higher; `sql_tablelist`) and `sysmaster:sysptprof` (row counters); PostgreSQL: `pg_stat_statements` (when installed) and `pg_stat_user_tables`. EKOS's own statements are left out: SQL Server sessions now set `DATEFIRST 5` (`usage::EKOS_DATEFIRST`), which Query Store and plan attributes record per statement; Informix and PostgreSQL leave out the session user's statements. Counters cannot (reads are flagged `includes_own_reads`); a write count is never EKOS's. `DataSignals.usage` (signal `usage`, read first in a run); `profile show`, `ekos_profile` |
| guards | the Informix probe guard parses `sysmaster:t` as `sysmaster.t` (only the parse; the original text is sent; a write is still refused); every usage query passes the SQL Server, PostgreSQL and Informix guards (tests) |
| fix found on the way | Informix conditional profile (§3.2(c)) grouped by an expression — the server answered -201 for every coded column; it now groups by position |

**Departures.** (a) "When the code knew a value" is the commit that put the literal on the cited line, followed back through that line's history: a literal that moved between files, or that the code knew elsewhere first, is dated by its current lines only (up to three). A value cited only by live-catalog routines (`postgres://…`) has no file history and is not dated. (b) `new` on a data-only gap means *all* unexplained values are newer than the code's last change (the profile keeps one first month for "everything else", never per value). (c) Usage matches a table by its name: statement text does not say whose `orders` it is, so two owners with one name are both counted and flagged `ambiguous`. (d) The SQL Server statement kind is the first of `select|insert|update|delete|merge` in the text (a `WITH … UPDATE` reads as `select`); Informix's comes from `sql_stmtname`. (e) Informix SQL tracing is off by default and `sysmaster:syssqltrace` is not readable by a plain user (measured: -272); both are the DBA's to grant, and EKOS says "unavailable" with the reason. Query Store needs `VIEW DATABASE PERFORMANCE STATE` (2022) and the plan cache `VIEW SERVER PERFORMANCE STATE`; reported the same way. (f) Row counters count rolled-back work too: LedgerSMB's `currency__is_used` probes with a `DELETE` it rolls back, and `pg_stat_user_tables` counted 3 deleted currency rows. (g) The monthly series is not on the passport (it reads the ledger, and the series lives in the profile file); "new" is.

**Informix CSDK path.** Still not run live: the IBM Informix Client SDK needs an IBM account to download, which this work did not obtain. The connection string for it (`host:port/db@server`, `DELIMIDENT=y`, `CLIENT_LOCALE`) stays unit-tested only. Everything Informix above was measured through the IBM Data Server Driver over DRDA, which runs the same SQL; what the CSDK path would add is SQLI and the `informix` server name. Open question 3 (a separate image variant) stands.

**Measured.**

*LedgerSMB* (the `ledgersmb` database of the repo's migrate server copied with `pg_dump` into a scratch PostgreSQL 16 with `pg_stat_statements.track = all`; code: a clone of the LedgerSMB repo at `544bcd947`, `sql/` observed, `[semantics]` on; read-only role `ekos_ro` with `pg_read_all_stats`): build → commit 24 min (9,529 objects); `ekos profile run` 2 min: 168 tables, 56 coded columns, 222 code values (161 never seen, 0 dead), 1 data-only column. Per month: 11 coded columns have a time column; `oe.oe_class_id = 1` (dated by `transdate`) 19 months 2024-12 → 2026-06, sum 2,112 and every month equal to a direct `GROUP BY to_char(transdate, 'YYYY-MM')`. New codes: 2 values have a dated first row and both were traced — `oe.oe_class_id = 1` introduced by `a374caf23` (2007-05-20, `sql/Pg-database.sql:1574`), `eca_to_location.location_class = 1` by `45bdc1a45` (2008-07-07, `sql/modules/Company.sql:654`, the oldest of the line's 5 versions, checked by hand with `git log -L`): both `code_before_data`. So 0 values the data held before the code named them and 0 "new" columns (the one data-only column, `defaults.value`, has no time column) — the fixture's data is from 2024–2026, the code from 2007–2008. First run found only one of the two: its first three cited lines were live-catalog routines; fixed (file lines only). Usage: a workload of real LedgerSMB functions and direct statements run 3× as `lsmb_app` — `pg_stat_statements` gave 15 (table, kind) rows over 13 tables, equal to a direct query with the same rule; EKOS's own (`ekos_ro`) left out; the foreign-key checks PostgreSQL runs as the table owner are counted (they are the application's work). `pg_stat_user_tables`: 168 tables, 3 written (`audittrail` 3 inserts, `currency` 3 deletes from the rolled-back probe above). Ledger and `.ekos/` scan (964 files, zstd decompressed) for five data values (two invoice numbers, three company names): not found; two account-link codes were found, both from `sql/Pg-database.sql` seed rows.

*SQL Server* (own SQL Server 2022 with the repo's `shop` fixture; Query Store capture mode `ALL`): as `ekos_ro` without rights both sources reported "unavailable" with the server's message; with `VIEW DATABASE PERFORMANCE STATE`, Query Store gave all 6 tables (e.g. `sales.orders` select 4 statements / 6 executions, update 1 / 3), and the 5 statements EKOS ran on `hr.employees` were all left out (`date_first = 5` in Query Store); `orders.status` per month by `placed_at`: 4 months of 1 row, each shown withheld.

*Informix* (own Informix 15.0.1.0.3, `shop` fixture, DRDA): `sysptprof` per table (e.g. `sales.orders` 4 inserts from the fixture load, reads including EKOS's own); `syssqltrace` "unavailable" (-272) until the DBA granted `SELECT` on it, then `sales.orders` select 5 / update 5 — equal to a direct count of uid 200's traced statements, while the 6 traced statements of `ekos_ro` (uid 1001) were left out. The conditional profile, which failed (-201) before the fix, now runs.

### 2026-11-12 — §8 measurements: iDempiere precision, BIRD Mini-Dev ceiling

Scripts and the labelled candidate list are in `evals/rfc0252/` (they read the Oracle seed through
`python-oracledb`'s thin mode as `EKOS_RO`, password from `EKOS_RO_PASSWORD`). They take no paths:
each reads and writes fixed names in the folder it runs from — `profile.json` and `relationships.json`
(copies of `<state>/profile/`), `truth.json`, `idempiere-candidates-labelled.json`; for BIRD
`mini_dev_sqlite.jsonl`, `split.json`, `databases/`, `packs/` and `reports/`.

**Foreign keys between the same two tables were merged (fixed during this measurement).**
`semantic::dedup_relationships` keyed relationships by (from, to, kind), so the second foreign key between
two tables disappeared. On the iDempiere seed, 3,686 constraints became **3,100** `ForeignKey` relationships,
which is exactly the number of distinct table pairs. It now keys a `ForeignKey` by its `constraint` /
`fk_desc` (`SemanticCompilerPass` `v4-identity3`), and after a recompile the ledger holds **3,686**. Numbers
below are given before → after the fix where it changed them.

**Setup.** A scratch home project: iDempiere `release-13` @ `3355eb5fc9`, `db/` only (Oracle/PostgreSQL
functions and procedures), plus the Oracle XE seed as `[[databases]] kind = "oracle"`, `profile = "p1"`,
`dictionary = "idempiere"`, `[semantics] enabled = true`. Release build with `--features oracle`. Times on a
machine shared with other agents' jobs (load 20–38): build 28 s, recover 15 s, compile 3 s, first commit
1,172 s (re-commit after the fix 59 s), `profile run` 318 s / 117 s, `relationships discover` 281 s / 117 s.

**Truth for references.** The application dictionary:
- every active `AD_Column` whose reference is Table Direct (19), Table (18) or Search (30) through
  `AD_Ref_Table`, or a fixed-target type (Location, Account, Locator, Attribute Set Instance, Assignment,
  Image, Chart);
- resolved to the parent table and key, with both ends base tables (`DBA_TABLES`, not views).

That gives 6,793 pairs. The 3,687 declared foreign-key column pairs are added to them. 3,244 dictionary
references are not declared in the database. A candidate counts as correct when it is in either set.
Everything else was labelled:
- `wrong_parent`: the child is a dictionary reference, but to a different table;
- `no_reference`: the 13 of these were checked by hand.

**Relationships (§3.3).** 7,148 columns signed. Before the fix: 281 pairs checked, 267 candidates. After:
279 pairs checked, **265 candidates** (264 data-seeded, 1 code-seeded — the code is `db/` functions only,
which join little).

| Label | Before → after the fix |
|---|---|
| declared FK proposed again | 4 → **0** (`AD_TABLE.AD_WINDOW_ID → AD_WINDOW`, hidden behind `PO_WINDOW_ID`, and three like it) |
| `wrong_parent` | 250 → 252 (two children that had lost a declared parent now get a wrong one: `AD_TABLE.AD_WINDOW_ID → AD_MENU`, `AD_WF_NODENEXT.AD_WF_NODE_ID → C_REGION`) |
| `no_reference`, true by hand | 2: `AD_TreeNodePR.Node_ID → M_Product` (data, 55 of 55; a product tree's nodes are products), `AD_Language.CountryCode → C_Country.CountryCode` (code seed, 1 orphan in 109) |
| `no_reference`, false by hand | 11: other tree nodes into unrelated id ranges (true parents `C_BPartner`, `AD_Menu`, …), `SeqNo`/`StartNo`/`A_Period` into `AD_Reference` |

- **Precision after the fix: 2 / 265 = 0.75 %** (data seed alone 1 / 264). Before the fix it was 6 / 267 = 2.2 %
  only because the 4 re-proposed declared keys counted as correct. Restricted to `clean_relationship`:
  1 / 73.
- **Why so low:** 252 of the 265 children already have a declared foreign key. Their real parent is
  excluded as declared, so the shared rule (`overlap::data_pairs`) offers any other sparse id range that
  contains the values (the demo seed's ids overlap across tables). A rule "never propose a second parent for a
  child with a declared or code reference" would leave 13 candidates, 2 of them true (15 %).
- **Recall: 0 / 0.** Of the 3,244 undeclared dictionary references, 1,618 have a child column holding data,
  and **every one** points at a parent with fewer than 20 distinct values in this seed (`AD_User` has 9 rows;
  `AD_Org`, `AD_Client`, `C_BPartner`, …). That is below `min_parent_distinct = 20`, so the reachable set is
  empty: the threshold that protects against tiny parents also hides every real undeclared reference here.
  Measuring recall needs a populated iDempiere (or data the size of LedgerSMB's).
- The §8 criterion "finds relationships the code-seeded pass did not" is met only literally (one:
  `AD_TreeNodePR.Node_ID → M_Product`), at a precision no reviewer would accept.

**Other signals** (same run; the fix did not change them):

| Signal | Measured | Checked against | Result |
|---|---|---|---|
| Literal coverage (a) | 1,966 coded columns, 4,150 code values; 2,653 never seen; 49 columns with unexplained values | 40 random columns re-counted directly (121 literal and unexplained counts) | **121 / 121 equal** |
| Unexplained values (a) | 49 columns | `AD_Ref_List` (the dictionary's own value lists) | 22 real: every extra value is a dictionary list entry the code never names; **20 false**: measures (`QtyEntered`, `PriceActual`, `LineNetAmt`, `FieldLength`, …) made "coded" by a comparison with `0`; 7 other |
| Dead codes (b) | 199 | 10 sampled by hand | 0 of 10 is a retired code. Flags set by **update** (`IsActive='N'`, `IsTranslated='Y'`) last appear at *creation* years ago, and the dictionary rows were created once at install. The creation-time axis does not fit a flag that is updated later |
| Implications (c) | 860 | each re-counted directly | **860 / 860 exact**, but 596 (69 %) imply a state that already holds for ≥ 95 % of the table (`IsActive='N' ⇒ AD_Chart_ID IS NULL`). 157 move the base rate below 95 %, 72 below 80 %, 35 below 50 % (e.g. `AD_Menu.Action='F' ⇒ AD_Workflow_ID IS NOT NULL` against a base of 2.7 % — a real meaning) |
| Keys (d) | 892 declared, 3 found, 8 without | `DBA_INDEXES` / `DBA_CONSTRAINTS` | `CREATE$JAVA$LOB$TABLE.NAME` and `JAVA$CLASS$MD5$TABLE.NAME` are correct, but are in fact **declared unique** constraints the grain does not count as declared; `DBREPLICASYNCVERIFIER.LASTUPDATE` is on a one-row table, so it says nothing |
| History (e) | 932 patterns (885 flags, 47 validity pairs) | not checked | — |

Suggested follow-ups (not built):
- skip a second parent for a child that already has a declared or code reference;
- let a person lower `min_parent_distinct` for a run on a small seed;
- do not treat a measure column (a number compared only with 0) as a coded column;
- give an update-set flag an update-time axis, or no dead-code verdict;
- rank implications by how far they move the base rate;
- count declared unique constraints as declared grain.

**BIRD Mini-Dev (§8, last criterion): bounded at ≈ 2 % of the tune questions; not measured with a model.**

RFC 0252's discovery reads live `[[databases]]`; BIRD is SQLite read in process. Built for this measurement:
- `Flavor::Sqlite` in `observation-sdk`, with arms in `profile` (affinity families, `"…"` identifiers,
  aggregates), `signals`, `probe` and `overlap`. Overlap uses an `md5(text)` function that the caller
  registers on the connection.
- The data-seeded proposal rule moved from the CLI into `overlap::data_pairs`, so the CLI and the bench
  share it. On iDempiere, `discover`'s 267 candidates were identical before and after the move.
- `ekos_evals::bench::data_signals`: signatures in SQLite, the shared rule, the exact inclusion check, and
  measured single-column keys of tables that declare none. `prepare` writes the result to
  `data/values/<db>.signals.json` (names and counts only).
- The cell option `data-signals = true`, which lists the references and keys found after the declared join
  paths. Cell `c-prime-sig` is `c-prime` plus the signals.

Discovery on the 11 databases (`prepare`, all of them: 3 min): 597 columns signed, 9 candidates, **1 true**
(`cards.setCode → sets.code`, clean over 56,822 rows) — precision 1 / 9. The 8 false ones are integer
columns contained in a sparse integer key with no name affinity (`formula_1`: `results.raceId →
constructorResults.constructorResultsId`, …; `financial`: `order.account_id → client.client_id`, unclear,
1,119 orphans). No keys were found: every table declares one, except three junction tables whose keys are
composite.

**Model-free ceiling**, tune split: 198 questions, gold SQL parsed with `sqlglot`, base-table equality joins,
199 join pairs in 147 questions. The table is the share of gold join pairs the pack offers.

| | Without signals | With signals |
|---|---|---|
| Before the FK fix | 179 / 199 (90.0 %); all joins offered in 128 / 147 questions | 182 / 199 (91.5 %); 131 / 147 |
| After the FK fix | **190 / 199 (95.5 %)**; 138 / 147 | **193 / 199 (97.0 %)**; 141 / 147 |

- The fix brought back 11 declared join paths, for example `connected.atom_id`/`atom_id2` → `atom` ×5 and
  `match.home_player_*` → `player`. european_football_2's pack went from 7 FKs to 31, codebase_community's from
  11 to 13. That is worth almost four times what the signals add.
- The signals add one line that matters, `cards.setCode = sets.code`. Gold uses it in tune questions 60,
  102 and 112: **3 of 198 questions (1.5 %)**. That is the upper bound on what any model could gain from
  the signals here.
- The 6 still missing:
  - 1 is BIRD's own wrong foreign key (`Match.league_id REFERENCES League(league_id)`; the key is `id`);
  - 5 are sibling joins (`disp.account_id = trans.account_id`) that no FK states and discovery rightly
    does not propose.
- All BIRD tables declare a key except three junction tables, and there is no code, so literal coverage,
  dead codes and grain do not apply.

**Why no model measurement.** A model would have to resolve an effect of at most 3 questions in 198. The
only allowed model is the local `qwen2.5-coder:3b`, which scored 2–5 % EX in RFC 0240 and executes about 60 %
of its answers, and the GPU is shared. A local-model run of `c-prime` against `c-prime-sig` (80 tune questions
on the four databases whose pack changes, on the pre-fix packs) finished before it was called off. Its result
is noise and is recorded only so nobody reads it as evidence: EX 1 vs 4 of 80, exact McNemar p = 0.25. None of
the 3 gains used a join found in the data, and on questions 60, 102 and 112 the model wrote `cards.setCode =
sets.code` without the signals anyway. Its reports are in `evals/reports/bench/bird-mini-dev/20261112T*`.
Those numbers are **not comparable** with the published RFC 0240/0241/0243 results, which used paid models.
No hold run was made.

**Verdict for §8.**
- iDempiere: precision is measured (0.75 % after the FK fix). The data-seeded rule needs the "child already
  has a reference" filter before it is worth a reviewer's time, and recall needs a populated database.
- BIRD: the signals add 1 correct and 8 wrong join lines, and their effect is bounded at 3 of 198 tune
  questions. The FK fix, not the signals, is what moves the pack.

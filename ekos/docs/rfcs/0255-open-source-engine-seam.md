# RFC 0255 — An open source-engine seam: out-of-tree engines, declared capabilities, declared gaps

**Status:** Accepted (2026-11-11, on the maintainer's instruction to start work)
**Date:** 2026-11-11
**Depends on:** RFC 0149 / 0191 (extension seam), RFC 0216 (source-engine seam, Oracle), RFC 0155 / 0156 (canonical
form, validation tiers), RFC 0157 / 0211 (live PostgreSQL and Oracle), RFC 0235 (`[[databases]]` kinds), RFC 0252
(`LiveProfiler`, SQL Server and Informix), RFC 0192–0196 (bronze, silver, schema changes, monitoring), RFC 0254
(source passport)
**Repos:** ekos-lakehouse (`bronze`, `lakehouse-ext`), EKOS (`migrate-validate`, `compiler-core`, `cli`)
**Followed by:** RFC 0256 (interaction map), RFC 0257 (transformation chain view)

---

## 1. Problem

The product decision of 2026-11-11: the open build supports PostgreSQL-like sources (and keeps the Oracle and Java
plugins, as the iDempiere demonstration). Every other source engine — SAP, Dynamics 365, others — is a separate,
separately sold plugin.

RFC 0149 already states how a sold capability must be built: the private crate depends on the public crates, and
the public binary offers a place to plug it in. For source engines there is no such place. The engine is decided
by four closed enums, matched in public code:

| Where | Closed point | What it decides |
|---|---|---|
| `ekos-lakehouse` `bronze/src/model.rs` | `enum SourceEngine { Postgres, Oracle }` | type map (`plan.rs`), watermark-orderable types and load mode (`mode.rs`) |
| `ekos-lakehouse` `lakehouse-ext/src/plan_cmd.rs` | `connect_source` matches `SourceEngine` | which `Source` is connected |
| `ekos-lakehouse` `lakehouse-ext/src/reconcile.rs` | `source_dialect` matches `SourceEngine`; `digest_columns: src == Dialect::Oracle` | the source side of a reconciliation |
| EKOS `migrate-validate/src/dialect.rs` | `enum Dialect { Postgres, ClickHouse, Trino, Oracle }` | canonical-form SQL (RFC 0155) |
| EKOS `compiler-core/src/config.rs` | `enum DatabaseKind { Oracle, Postgres, Clickhouse, Sqlserver, Informix }` | which live connector reads a `[[databases]]` entry |

`settings.rs` and `doctor.rs` in `lakehouse-ext` also branch on the engine. An out-of-tree crate can therefore add
observers, passes, commands and console pages (RFC 0149, 0191, 0198), but it cannot add a source engine without a
change to public code for each engine.

There is a second problem, independent of who sells what. Nothing states **what a source engine must provide, and
what it cannot**. PostgreSQL gives a catalog, declared keys, trigger bodies, usage statistics and a SQL reader. A
SaaS ERP gives an API and no SQL. A custom database gives a schema with no foreign keys. Today the difference
shows up as an `Ok(vec![])` default (`Source::value_lists`) or as a missing branch, and a consumer cannot tell
"this source has no value lists" from "this engine cannot read them".

## 2. Goals and non-goals

**Goals**

1. An engine is added by registering it, in or out of tree, with no edit to a public `match`.
2. One declared contract per engine: its **capabilities**, each `provided`, `partial` or `absent` with a reason,
   recorded as a ledger fact and shown to the operator.
3. An absent capability is never a silent default: every consumer either degrades in a stated way or refuses.
4. A public **conformance kit**: the tests an engine must pass, so a plugin proves its type map and canonical
   forms the same way the built-in engines do.
5. PostgreSQL and Oracle become two registered engines with **byte-identical** output: lock files, generated
   pipelines and reconciliation SQL do not change.

**Non-goals**

- Writing any new engine. SQL Server and Informix are live sources for profiling (RFC 0252 §3.5), not lakehouse
  source engines; making them engines, and every sold engine, comes after this RFC and uses it.
- Sources without SQL access (API-only SaaS). The capability model has room for them (§3.6); extraction through an
  API is a later RFC.
- Target engines. Iceberg through Trino stays the only lakehouse target.
- Loading plugins at runtime (dynamic libraries, WASM). An engine is compiled in, as every extension is
  (`ekos::app::main_with`).
- Licence enforcement, entitlement checks or a plugin registry. A sold engine is a private repository and a
  private binary, as RFC 0148's decompiler is.
- Renaming lock-file fields. `SourceColumn::pg_type` keeps its name (lock files carry it), as RFC 0216 decided.

## 3. Design

### 3.1 Engine identity

`bronze::SourceEngine` becomes an open identifier:

```rust
/// A source engine's stable name: `postgres`, `oracle`, or a registered out-of-tree engine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceEngine(Cow<'static, str>);

impl SourceEngine {
    pub const POSTGRES: Self = Self(Cow::Borrowed("postgres"));
    pub const ORACLE: Self = Self(Cow::Borrowed("oracle"));
    pub fn is_postgres(&self) -> bool { self.0 == "postgres" }
}
impl Default for SourceEngine { fn default() -> Self { Self::POSTGRES } }
```

The enum serialized as `snake_case`, so `"oracle"` reads and writes as before, and
`skip_serializing_if = "SourceEngine::is_postgres"` keeps a PostgreSQL lock file without the field. Names follow
the `[[databases]]` name rule (`[a-z0-9][a-z0-9_-]{0,62}`).

A lock file naming an engine the running build does not have is refused with a message that names the engine and
says the build lacks it. This is the case of a pipeline planned by a build with a sold engine and opened by the
public build.

### 3.2 `EngineSpec`: what an engine is without a connection (`bronze`)

Everything `bronze` decides from the engine today, behind one trait. It is pure: no database, no I/O.

```rust
pub trait EngineSpec: Send + Sync {
    fn engine(&self) -> SourceEngine;
    /// The engine's type → Iceberg type, its lossiness and its compare rule (RFC 0192, RFC 0216 §2.4).
    fn map_column(&self, column: &SourceColumn) -> Mapping;
    /// Whether a column of this source type can order a watermark (`mode.rs`).
    fn orderable(&self, source_type: &str) -> bool;
    /// What this engine provides and what it does not (§3.5).
    fn capabilities(&self) -> Capabilities;
}
```

`Lossiness` and `CompareRule` stay **closed**. An engine maps its types into them and cannot add a member: a new
lossiness class or a new compare rule changes what "reconciled" means for every engine, and is a core change with
its own RFC. A type with no faithful landing is `Refused`, naming the column, as today.

`plan.rs` and `mode.rs` take `&dyn EngineSpec` where they match on the enum now. Phase 1 starts with an audit of
every use of `SourceEngine` in the workspace (`bronze`, `silver`, `lakehouse-ext`, tests); any engine-specific
rule found outside the type map and `orderable` either moves into `EngineSpec` or is recorded here as a
deliberate exception.

### 3.3 `SourceProvider`: connecting an engine (`lakehouse-ext`)

```rust
pub trait SourceProvider: Send + Sync {
    fn spec(&self) -> Arc<dyn EngineSpec>;
    /// Which `[extensions.lakehouse]` keys this engine reads and which it refuses (RFC 0216 §2.2).
    fn check_settings(&self, settings: &Settings) -> anyhow::Result<()>;
    /// The engine, connected read-only.
    fn connect(&self, settings: &Settings, purpose: &str) -> anyhow::Result<Box<dyn Source>>;
    /// Extra `lakehouse doctor` checks for this engine (RFC 0196 §4). Default: none.
    fn doctor(&self, _source: &dyn Source) -> Vec<DoctorCheck> { vec![] }
}
```

`source::Source` (RFC 0216) is unchanged except that `engine()` returns the new `SourceEngine`. `writes_refused`
stays a required method: an engine that cannot show its session is read-only is not a source.

`connect_source` becomes a lookup in an `Engines` registry — a cheap `Clone` over `Arc<[Arc<dyn SourceProvider>]>`,
passed explicitly as `Extensions` is (RFC 0149: dependency injection, not global state). The Lakehouse extension
is constructed with the built-in engines by default and accepts more:

```rust
// the public ekos-lakehouse binary
ekos::app::main_with(Extensions::of([Lakehouse::new()]));                     // postgres, oracle

// a private binary with a sold engine
ekos::app::main_with(Extensions::of([Lakehouse::new().with_engine(sap::provider()), sap::extension()]));
```

Startup refuses two providers with the same engine name, as `main_with` refuses two extensions with the same
command (RFC 0191).

### 3.4 Canonical form for an out-of-tree dialect (EKOS `migrate-validate`)

Reconciliation renders each compared column with the canonical-form rule for the source's `Dialect` and the same
rule for `Dialect::Trino`. `Dialect` is closed, so a new engine has no source side.

The rendering moves behind a trait that the four built-in dialects implement, and that an engine may implement:

```rust
pub trait CanonDialect: Send + Sync {
    fn name(&self) -> &str;
    /// The canonical-form expression (RFC 0155) for `column` under `rule`.
    fn canon_expr(&self, column: &str, rule: ColumnRule) -> String;
    /// The engine cannot build one row string; columns are digested one by one (RFC 0216 §2.5).
    fn digest_columns(&self) -> bool { false }
    // plus the key concatenation, hash and bucket expressions the built-in dialects define today
}
```

`EngineReader::dialect()` returns `&dyn CanonDialect`. `Dialect` stays as the name of the built-in four and
implements the trait, so existing callers that pass `Dialect::Trino` compile unchanged. The Oracle special case in
`reconcile.rs` becomes `src.digest_columns()`.

**The conformance kit.** Agreement between a dialect and the canonical form is proven today by golden forms, the
Rust hashes and bucket checksums, run live (31/31 on Trino, RFC 0190). That suite becomes a public module,
`ekos_migrate_validate::conformance`: given an `EngineReader` on a scratch schema, it creates the golden rows,
runs every rule and reports each mismatch. An engine without a passing run has `reconcile: absent` (§3.5) whatever
it implements. A parallel kit in `bronze` checks a type map: every source type is mapped or refused, never
silently dropped; `NarrowingSafe` appears only with a measurement; a refused column refuses its table.

### 3.5 Capabilities: the contract

`Capabilities` is a fixed list. Each entry is `Provided`, `Partial { note }` or `Absent { reason }`.

| Capability | What it is | Consumed by | When absent |
|---|---|---|---|
| `catalog` | tables, columns, types | everything | **required** |
| `read_only_proof` | `Source::writes_refused` | `lakehouse doctor`, every connection | **required** |
| `declared_keys` | primary and foreign keys | silver keys and tests, mart joins (RFC 0248 resolves along declared keys only) | data-seeded discovery (RFC 0252 §3.3) is the only seed; each candidate is a review item and an `is_this_a_reference` question (RFC 0254) |
| `descriptions` | table and column comments, business names | estate docs, term resolution | `SemanticGap`; the passport says the engine has none |
| `value_lists` | enum labels, `IN (...)` checks (RFC 0227) | silver value tests | none generated; literal coverage (RFC 0252 §3.2a) is the evidence |
| `routines` | trigger and procedure source | watermark proof (RFC 0216 §2.3), write behaviour (RFC 0212) | no watermark is proven from code; load mode falls back as `mode.rs` already does for a table without evidence |
| `column_identity` | a column's identity across renames (`attnum`) | schema changes (RFC 0194) | a rename is seen as a drop and an add; the proposal says so |
| `usage_stats` | per-table and per-statement statistics (RFC 0157) | interaction map (RFC 0256), design hints | observed interaction is empty and labelled "engine provides none", not "nobody writes" |
| `profiling` | RFC 0252's `LiveProfiler` (closed aggregate vocabulary) | profiling signals, questions, probes (RFC 0254) | no data-derived signals; the passport says so |
| `reconcile` | an `EngineReader` whose dialect passes the conformance kit | source ↔ bronze reconciliation (RFC 0156) | tables are never `healthy`: RFC 0196 reports them `unverified`, with this capability as the evidence |

Rules:

- The list is closed and versioned (`CAPABILITIES_VERSION`). A new capability is an RFC; engines built before it
  report it `Absent { reason: "predates capability" }` by default.
- An engine declares; the build **checks what can be checked**. `reconcile: provided` requires a recorded
  conformance run. `declared_keys: provided` with zero keys read in a non-empty schema is downgraded to `partial`
  with the count.
- `plan` records one `SourceCapabilities` fact per source (keyed by the `[[databases]]` name, or the Lakehouse
  source): engine, capabilities, the conformance run's id. The latest version is the current state.
- `lakehouse doctor` prints the table. The source passport (RFC 0254) and the estate docs (RFC 0214) show it at the
  top of the source: an analyst reading "no relationships" must see whether that is a fact about the database or
  a limit of the engine.
- A consumer added later must name, in its RFC, what it does when its capability is absent.

### 3.6 The core side

A sold engine also needs its live connector in core (catalog, profile). Two changes:

- `DatabaseKind` gains a path for registered kinds. `kind` in `[[databases]]` stays a string in `ekos.toml`; a
  value that is not a built-in kind is accepted when an extension registers a live connector for it and refused
  otherwise, naming the kind. The built-in five keep their enum and their JSON schema.
- `EkosExtension` gains `fn live_sources(&self) -> Vec<Arc<dyn LiveSourceProvider>>` (default empty). A provider
  names its kind, checks the entry's fields, and returns the `LiveProfiler` of RFC 0252 §3.1 for it. This RFC
  depends on that trait being in `ekos-observation-sdk`; until RFC 0252 lands it, the two `CatalogSource` traits
  in `pg-live` and `ora-live` stay as they are and are not part of the public contract.

Room for sources without SQL: `profiling`, `usage_stats` and `reconcile` may all be `absent`, and the engine may
still land bronze. Such a source is useful and is honestly labelled. Profiling the landed bronze instead of the
source is the natural next step and is not in this RFC.

### 3.7 What stays public

The traits, the registry, both conformance kits, the PostgreSQL and Oracle engines, the Java plugin, this RFC and
the guide in §5. A sold engine is a provider crate in its own repository plus a binary, and depends on the public
crates.

## 4. Phases and exit

| Phase | Deliverable | Exit |
|---|---|---|
| 1 | Audit of every `SourceEngine` use; `SourceEngine` as an identifier; `EngineSpec`; PostgreSQL and Oracle specs | Pagila and iDempiere lock files and generated pipelines byte-identical before and after; unit suites green |
| 2 | `SourceProvider`, `Engines`, `Lakehouse::with_engine`; settings and doctor per provider | PostgreSQL and Oracle live suites green; a lock file with an unknown engine is refused by name |
| 3 | `CanonDialect`; `migrate_validate::conformance`; the type-map kit | conformance passes for PostgreSQL, Oracle and Trino live; `reconcile.rs` has no engine `match` |
| 4 | `Capabilities`, `SourceCapabilities` fact, doctor table, passport and estate-doc section | the iDempiere passport shows the table; a planted engine declaring `reconcile: provided` without a run is downgraded |
| 5 | `live_sources`, registered `[[databases]]` kinds; a fixture engine built **out of tree** in CI | the fixture plans, lands bronze and reconciles with no change to a public crate; `docs/writing-a-source-engine.md` is executed by that CI job |

Phase 5's fixture is the acceptance test of the whole RFC: if adding it needs an edit to a public crate, the seam
is not open.

## 5. Tests

- Round trip: a lock file written before this RFC reads back and re-serializes byte-identically, for PostgreSQL
  (no engine field) and Oracle.
- Two providers with one engine name stop startup with a message naming both.
- A provider's `check_settings` refusal names the key and the engine (today's Oracle rule for `source`,
  `source-user`, `source-password-env`).
- An engine whose type map drops a column without refusing it fails the type-map kit.
- An absent `declared_keys` produces no silver relationship test and one review subject per data-seeded candidate.
- An absent `reconcile` never yields `healthy` in `lakehouse status`.
- `Extensions::none()` and the default `Lakehouse::new()`: command tree, catalogue and `ekos extensions --json`
  unchanged except for the new capability output.

## 6. Open questions

1. `Dialect` is passed by value today and compared with `==` (`src == Dialect::Oracle`). Moving callers to
   `&dyn CanonDialect` touches every reconciliation path in `migrate-validate`. The alternative is a fifth variant
   holding a registered dialect's id, which keeps `Dialect: Copy` and leaves callers alone but adds a lookup.
   Decide after the phase 1 audit counts the call sites.
2. *Decided (owner, 2026-11-11):* the `ekos-lakehouse` crates (`bronze`, `lakehouse-ext`) can be consumed from an
   external repository, so phase 2 has no packaging work. Phase 5's out-of-tree fixture is the standing proof.
3. Does silver hold engine-specific rules (RFC 0216 leaves Oracle's `Y/N` → boolean and the empty-string rule to
   step 6)? If it does, they belong in `EngineSpec`, and phase 1's audit must list them.
4. `SourceColumn` carries PostgreSQL's shape (`TypeKind` from `typtype`, `attnum`, `infinite_values`). This RFC
   documents how another engine fills them. A neutral shape would break lock files and is deferred.
5. Should `SourceCapabilities` be recorded by core for every `[[databases]]` entry, including ones the Lakehouse
   does not read (ClickHouse)? It would make the passport uniform, at the cost of a second producer of the fact.

## 9. Implementation log

### 2026-11-11 — phases 1–4, and phase 5's out-of-tree fixture

Branches `rfc0255` in EKOS and in ekos-lakehouse. The lakehouse was built against this EKOS branch
through a local, uncommitted `[patch]`; its committed `Cargo.lock` still pins an older EKOS revision
and needs `cargo update -p ekos -p ekos-migrate-validate -p ekos-kir …` once EKOS is merged.

**Phase 1 audit — every use of `SourceEngine`** (lakehouse, 54 mentions in 14 files before the change):

| Where | Use | Now |
|---|---|---|
| `bronze/model.rs` | the enum | an open identifier, `bronze/engine.rs` |
| `bronze/plan.rs` | `PlanSettings::source_engine` (the lock field); type map `match` | field kept, unwritten for PostgreSQL; `EngineSpec::map_column` via `plan_for` |
| `bronze/mode.rs` | `orderable(engine, type)`; `choose*` thread the engine | `EngineSpec::orderable`; `choose_for/with` take `&dyn EngineSpec` |
| `bronze/extraction.rs` | `orderable` when an operator makes a table incremental | `apply`/`apply_all` take the spec |
| `lakehouse-ext/source.rs`, `source_oracle.rs` | `Source::engine()` | unchanged, returns the identifier |
| `lakehouse-ext/plan_cmd.rs` | `connect_source` `match` | `Engines::get(..).connect` |
| `lakehouse-ext/settings.rs` | `source-engine` key; `if Oracle { resolve_database }` | `load_with`: the provider's `check_settings` |
| `lakehouse-ext/reconcile.rs` | `source_dialect` `match`; `digest_columns: src == Oracle` | the source reader's dialect; `Dialect::digest_columns()` |
| `lakehouse-ext/doctor.rs` | the reachability address from `[[databases]]`; the Trino connector hint for PostgreSQL; the role name | `SourceProvider::{address, connector_hint, role, doctor}` |
| `lakehouse-ext/extraction.rs` | — (via bronze) | `finalize` takes `&Engines` |
| tests (`silver`, `extraction`, `oracle_live`) | constructors | `SourceEngine::ORACLE` |

Deliberate exceptions, engine knowledge outside the type map and `orderable`, left where they are:

- `silver::one_char` recognises the one-character text spellings of both built-in engines
  (`CHAR(1`, `VARCHAR2(1`, `BPCHAR(1` …) for a **code-declared** `Y`/`N` flag. An out-of-tree engine
  whose spelling it misses keeps that flag as text — the safe direction — and a **declared** flag
  (a check constraint) does not depend on it.
- `deletes.rs` caps an `IN (…)` list at 500 keys for every engine because Oracle takes at most
  1,000: conservative for all, not a branch.
- `gold` matches column names case-insensitively for Oracle's upper case — engine-neutral.

**Open question 3 — answered: silver holds no engine-conditioned rule.** The `Y`/`N` → boolean rule is
decided by evidence (a check constraint, or the application model), not by the engine; there is no
empty-string rule in silver. Only the spelling exception above.

**Phase 1 exit — byte-identical, measured.** `lakehouse-ext/tests/engine_identity.rs` digests, for a
lock, every generated file (`render::files` + `silver::files`) and every reconciliation query
(counts, aggregate pack, bucket checksum both sides, `digest_columns`, columns not compared). The
digests were recorded on the commit before the seam (`b75120d`) from a Pagila lock (PostgreSQL, no
engine field) and a slice of the published iDempiere demo lock (Oracle), and the test now compares:
both locks re-serialize byte for byte and both digest files match after phases 1–4. The full
published locks, outside the repository (`RFC0255_EXTRA_LOCK`), give the same digests before and
after: `ekos-lakehouse-oracle/pipeline` (849 tables, 6,402 digest lines)
`6e0196ce312386f704eb5f9c1545a6929ca30a5a70f5d6ed5afcc2051708bea7`, `pipeline-sales` (234 lines)
`4773910414d9de0930b5e8b70b7cc7df48a513fadffeefd2474b9ac142a211de`.
`migrate-validate`'s SQL snapshots for the four built-in dialects are unchanged.

**Phase 2.** `lakehouse-ext/src/engines.rs`: `SourceProvider` (`spec`, `check_settings`, `connect`,
`dialect`, `conformance_literals`, `address`, `role`, `connector_hint`, `doctor`, `provider_name`),
`Engines` (a clone over `Arc<[Arc<dyn SourceProvider>]>`; `builtin`, `try_with`, `get`, `spec`),
`LakehouseExtension::new().with_engine(..)` / `try_with_engine`. Deviations from §3.3:
`check_settings(&self, &mut Settings, &[DatabaseConfig])` — Oracle fills in its `[[databases]]` entry,
so the check needs the databases and may resolve; the extension type keeps its name
`LakehouseExtension` (now a value, `LakehouseExtension::new()`); `Settings` carries the build's
`Engines` (`#[serde(skip)]`, as the resolved `database` already was), so every command that loads
settings connects through the registry without a second parameter. An engine the build lacks is
refused by name in settings (`load_with`) and in a deployed lock (`plan_cmd::read_lock_for`, used by
`generate --from-lock`, `reconcile`, `deletes`); `plan_with` (built-ins only) refuses every table of
such an engine. Tests: `engine_seam.rs` (5), `engines.rs` (3), bronze `plan.rs` (2).
Live: the PostgreSQL provider connects to the sandbox source; `bronze_live`, `changes_live` and
Trino's planted-defect test fail before any engine code, on `CATALOG_NOT_FOUND` (`pagila`, `m3`,
`postgresql`): the running Trino has no such catalogs. The sandbox was not reconfigured. Oracle's
live suite needs the testbed, which was not running.

**Open question 1 — decided: a fifth variant holding the dialect, not `&dyn` everywhere.** Counted at
phase 3: 261 `Dialect::` references in 16 EKOS files plus the lakehouse, `Dialect` passed by value
and compared with `==`. `Dialect::Registered(&'static dyn CanonDialect)` keeps `Dialect: Copy + Eq`
(equality by name) and needs no lookup and no registry — the engine's crate declares
`static CANON` — so every existing caller is unchanged; the free functions dispatch to the trait.
`EngineReader::dialect()` keeps returning `Dialect`. `Dialect` does not implement `CanonDialect`
itself: the variant makes that unnecessary. Three exhaustive matches in `migrate-logic/v5.rs` gained
an arm (a registered dialect is not a V5 engine).

**Phase 3.** `CanonDialect`: `canon_body`, `hash_hex_expr`, `prefix60_expr` required; null literal,
identifier quoting, the NULL wrapper, separator, row hash, column digest, exact sum, modulo, byte
length, `digest_columns` defaulted to portable SQL; bucket queries as a grouped subquery with a bare
alias. `ekos_migrate_validate::conformance` (`KIT_VERSION` 1): `Literals` (hand-written by the engine,
`None` + reason per uncovered case), `run(reader, literals) -> Report` (fixture cases, four hash and
prefix texts, a 200-row bucket checksum; an id that digests the report), `coverage`, and the literal
tables of the four built-in dialects (PostgreSQL/ClickHouse from `fixtures`, Trino and Oracle moved
from the lakehouse live suites). Live runs: **PostgreSQL 31/31 and ClickHouse 29/29**
(`migrate-validate/tests/conformance.rs`, sandboxes of `docker-compose.migrate.yml`), **Trino 31/31**
(lakehouse `trino/tests/live.rs`), and a planted check — Trino's literals on PostgreSQL — fails.
**Oracle not run** (testbed not up): the kit test is in `oracle_live.rs`. `reconcile.rs` has no engine
`match`: the source side is the source reader's dialect, `unit_plan`/`evidence_queries` take it, the
key separator comes from a registered dialect. The bronze type-map kit (`bronze/typemap_kit.rs`):
mapped or refused, never dropped; `NarrowingSafe` only with a measurement; a refused column refuses
its table, naming it; no compare rule on a refused column. Both built-in maps pass on their sample
lists; a planted map that drops a column and narrows unmeasured fails with two findings.

**Phase 4.** `ekos_kir::source_capabilities` holds the vocabulary so the fact has one schema for both
producers: `Capability` (ten, `CAPABILITIES_VERSION` 1), `Support`, `Capabilities` (undeclared =
absent "predates capability"), `checked` (the two downgrade rules), `SourceCapabilities` and its
object, keyed by source (`id_of`; registry row `SourceCapabilities`, structurally keyed). `bronze`
now depends on `ekos-kir`. `EngineSpec::capabilities` is required. Declared: PostgreSQL all provided
except `value_lists` partial (enum types; `CHECK … IN` lists are not read); Oracle `column_identity`
partial (`COLUMN_ID` is a position), `usage_stats` absent (the connector reads no workload
statistics). `lakehouse plan` runs the kit on the connected source (read-only `SELECT`s), checks the
declaration against the tables and keys read, and appends the fact (a failure to write is a warning);
`lakehouse doctor` adds an `engine` check with the table; `lakehouse reconcile` turns a green result
`unverified`, with the reason, when `reconcile` is not provided — RFC 0196's `unverified`, so
`lakehouse status` never says healthy. Estate docs: a "Source engines" section on the home page, and a
line under a table's "Keys and references" when the project's one source does not provide declared
keys. Exit: the planted engine that declares `reconcile: provided` without a run is downgraded
(`capabilities.rs` tests, `source_capabilities` tests), the estate test renders an iDempiere-shaped
Oracle source's table. Live: PostgreSQL passes the kit through the provider's `PgSource` reader and
keeps `reconcile` provided (`capabilities_live.rs`). Not done: the iDempiere passport on the published
demo (needs the Oracle testbed and a re-plan); the source passport per table page beyond the keys
line (a table does not yet say which source it came from).

**Phase 5 (part).** `examples/fixture-engine` in ekos-lakehouse — its own Cargo workspace, path
dependencies on the public crates, **no change to a public crate**: an engine named `fixture`
(PostgreSQL-compatible; refuses `money`; declares no keys) with its own `EngineSpec`,
`CanonDialect`, `Source`, `SourceProvider`, literals and binary. Offline it registers, refuses a
foreign key by name and engine, plans, renders bronze, writes a lock the public build refuses by name,
and passes the type-map kit; live, **its own dialect passes the conformance kit on PostgreSQL 18**
(31 cases, hashes, buckets) and keeps `reconcile` provided. `scripts/out-of-tree-engine.sh` runs it
and checks that `docs/writing-a-source-engine.md` names only items the fixture defines; the
lakehouse repository has no CI workflow, so the script is the job.

**Not done.**

- §3.6 in core: `EkosExtension::live_sources` and registered `[[databases]]` kinds. RFC 0252 built no
  `LiveProfiler`; its shared pure plan is the per-`Flavor` SQL builders run through
  `commands/profile.rs` and `sql_check::ProbeWorker`, and `Flavor` is closed. The provider this RFC
  needs is therefore `kind` + `check(&DatabaseConfig)` + a read-only `sql_dialect_sdk::Probe` + its
  capabilities, with profiling `absent` until `Flavor` opens — and `DatabaseKind` would gain a
  `Registered` unit variant plus the entry's kind name, with an arm in every `match` of
  `build.rs`, `database.rs`, `profile.rs`, `relationships.rs` and `sql_check.rs`. Deferred: those five
  files are being changed for SQL Server and Informix in parallel.
- The fixture landing bronze and reconciling end to end: needs a Trino catalog for its source.
- Open question 5 (core recording `SourceCapabilities` for every `[[databases]]` entry): unchanged; the
  vocabulary now lives in `ekos-kir`, so a second producer needs no new schema.

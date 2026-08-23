# RFC 0086 — Real Backend → Database Cross-Tier Edges (Ecto Repo Adapters)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

Phase 6 (the plan's own "stretch" phase) of the "Deep Source Decomposition + Production-Grade
Architecture Diagrams" plan — the last piece needed for `## System Decomposition` (RFC 0083) to
show real *relationships* between layers, not just real layer boxes with no edges between them.
The plan scoped this phase honestly from the start: Backend→Database is "real and buildable" via
Ecto repo configuration; Frontend→Backend is explicitly lower-confidence and separately deferred
(route/fetch-call matching, not attempted here). This RFC ships only the buildable half.

## Design

**Real evidence, confirmed against the source before designing anything**: the real analytics
project's own `lib/plausible/repo.ex`/`clickhouse_repo.ex`/`deletion_repo.ex`/… all declare
`use Ecto.Repo, adapter: Ecto.Adapters.Postgres` / `Ecto.Adapters.ClickHouse` directly inside the
Repo module's own body — a real, in-source, self-contained signal, no separate `config/*.exs`
parsing needed.

**Extended `elixir_analyzer.rs`** (RFC 0081's existing pass, not a new one): tracks which modules
have seen `use Ecto.Repo` anywhere in their own body; when a later `adapter: Ecto.Adapters.X` line
appears in the same open module, emits a real `Custom("Technology")` object (`ecosystem:
"database"`, name normalized to `dependency_analyzer.rs`'s own existing "PostgreSQL" convention
where one exists, an unrecognized adapter keeping its own real name rather than being dropped) +
a real `DependsOn` edge from the Repo module to it. Requires the literal `adapter:` keyword on the
same line as `Ecto.Adapters.` — a real, unrelated call site in this same codebase
(`Ecto.Adapters.SQL.query!(...)` in `lib/plausible/purge.ex`) must not be misread as a config
declaration, and isn't. Reuses the *same* `technology_kir_id` hash scheme every other analyzer's
own local copy uses, so a database detected here resolves to the same real object a
`dependency_analyzer.rs` substring hit elsewhere would produce — not a duplicate.

**Extended `docs-gen`'s `system_decomposition_graph`** (RFC 0083) with two real, minimal additions:
- `layer_membership_for_edges`: a real `Contains`-based one-hop layer inheritance (a Backend
  `File`'s real `ElixirModule` inherits `Layer::Backend` too) — used *only* to resolve a
  cross-tier edge's endpoints, never to inflate the displayed per-layer file/table *counts*, which
  stay exactly the real `File`/`Table` numbers.
- `db_technology_bucket`: routes a real database-adapter `Technology` object into the same
  `layer_sql`/`layer_clickhouse` bucket a matching real `Table` object would use — the reader sees
  "Backend depends on ClickHouse Database" regardless of which real analyzer produced the
  evidence. When a bucket has a real adapter reference but zero real compiled `Table` rows (true
  for this project's ClickHouse side — Ecto is configured, but no ClickHouse schema was ever
  recovered), the node still gets created with an honest `"(config only, no tables compiled)"`
  label rather than a fabricated table count or a silently-dropped edge.

## Scope — what this does and doesn't cover

**Covers**: real Backend→Database edges from a Repo module's own real adapter declaration to the
real database technology it names, correctly bucketed alongside real `Table` evidence when it
exists, honestly labeled when it doesn't.

**Does not cover** (explicitly deferred by the plan itself, not silently cut): Frontend→Backend
edges (API route/fetch-call matching) — the plan's own text already scoped this as a
separately-evaluated, lower-confidence increment; not attempted here. Per-table precision (which
specific real tables a given Repo talks to, vs. "this Repo depends on the ClickHouse family") —
elixir_analyzer.rs runs as an independent `CompilerPass` with no visibility into `sql_analyzer.rs`/
`clickhouse_analyzer.rs`'s own output during `recover`; real per-table linking would need a
compile-time cross-pass resolution step (the same class of mechanism RFC 0075's
`link_transform_nodes_to_tables` already established for a different pair of passes) — a real,
separable follow-on, not attempted in this increment.

## Testing

- 4 new tests in `elixir_analyzer.rs`: a real `use Ecto.Repo` + `adapter: Ecto.Adapters.Postgres`
  producing a real Backend→Database edge with the normalized "PostgreSQL" name; ClickHouse keeping
  its own real name; a real unrelated `Ecto.Adapters.SQL.query!(...)` call site (confirmed to
  exist in the real analytics project) not misread as a config line; a module with no real
  `use Ecto.Repo` never producing a database edge even with a coincidentally adapter-shaped line.
- Extended the cross-file object dedup condition (the same one `ElixirModule` already used) to
  also cover `Custom("Technology")` — found live before shipping: the real analytics project alone
  declares 5 separate ClickHouse-adapter Repo modules, which would have re-pushed 5 duplicate
  "ClickHouse" objects into one artifact without this fix (exactly the avoidable-duplication class
  RFC 0076 Finding 6 already tracks, not one to introduce fresh here).
- 3 new tests in `docs-gen`: a real Backend `ElixirModule` + database-adapter `Technology` fixture
  produces a real `layer_backend`→`layer_sql` edge; a ClickHouse adapter routes to
  `layer_clickhouse`, not `layer_sql`; module-layer inheritance never inflates the displayed
  per-layer file count.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end** against the real analytics project: confirmed via `ekl` that
  `Plausible.ClickhouseRepo` and 5 other real ClickHouse-adapter modules all resolve to the *same*
  real "ClickHouse" `Technology` object (exactly 6 real edges, not 6 duplicate objects — the dedup
  fix confirmed working), and `Plausible.Repo`/2 other real Postgres-adapter modules resolve to
  the same real "PostgreSQL" `Technology` object (3 real edges). `Architecture.md`'s `## System
  Decomposition` now shows real arrows: `Backend → SQL Database (57 tables)` and
  `Backend → ClickHouse Database (config only, no tables compiled)` — the honest label correctly
  distinguishes "real Ecto config exists" from "real Table rows were also compiled," rather than
  fabricating a table count for the ClickHouse side.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0086-backend-to-database-cross-tier-edges.md` | This RFC |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | Ecto Repo adapter detection; cross-file `Technology` dedup fix; 4 tests |
| `ekos/crates/docs-gen/src/lib.rs` | `layer_membership_for_edges`, `db_technology_bucket`, honest config-only node label; 3 tests |
| `TODO.md` | Phase 6 (and the whole decomposition plan) marked done |
| `devlogs/devlog_89.md` | This increment's devlog |

# RFC 0036 — Pentaho → dbt Model Export

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-07

---

## Motivation

RFC 0035 turned the compiled Transformation IR into human-readable documentation — Markdown,
HTML, diagrams. The natural next question: can the same compiled graph become something
*executable*, not just readable? A recovered Pentaho `.ktr`/`.kjb` job is business logic an
organization usually wants to retire onto a modern stack, and dbt is the most common target for
that migration today. `TransformNode` objects (RFC 0027) already carry structured table names,
join keys/kind, and group-by/aggregate data — real SQL-shaped facts, not prose. Rendering them as
dbt models is a second **rendering target** over the same compiled graph `ekos-docs-gen` already
reads, not a new extraction pass.

Investigated directly against the current source before writing this RFC:

- `ekos/crates/semantic/src/transform_ir.rs` — every `TransformNode` variant (`Source`, `Filter`,
  `Join`, `Aggregate`, `Calculate`, `Sink`, `Unmapped`) and exactly what each carries.
  `Source`/`Sink`/`Aggregate` hold real, structured table/column/join-key/group-by/agg-func data.
  `Filter.condition`/`Calculate.expr` are raw, un-parsed source-dialect text — RFC 0027 explicitly
  rejected a cross-format expression AST as out of scope ("a large, separate project with no
  immediate consumer"), so these cannot be safely transpiled into valid dbt SQL.
- `ekos/crates/recovery/src/pentaho_analyzer.rs` — the real Kettle step → `TransformNode` mapping.
  `Join`'s own internal `left`/`right` `NodeId` fields are unreliable self-referential placeholders
  (flagged directly in the analyzer's own code comments); the *graph-level* `FeedsInto` edges (from
  `<order>/<hop>`) are real and already what `ekos-docs-gen`'s Mermaid renderer draws from, so this
  RFC resolves upstream models via `FeedsInto`, never via `Join`'s own fields. Real test data
  (devlog_34): 49% of a real Pentaho repo's steps came back `Unmapped` — zero structured semantics,
  only raw XML + a reason string. `.kjb` job-orchestration entries are always `Unmapped` by design.
- `ekos/crates/docs-gen/src/lib.rs` + `ekos/crates/cli/src/commands/docs.rs` — the structural
  precedent this RFC copies: a pure-rendering crate reading `KirObject`/`KirRelationship` (zero
  I/O), plus a CLI-side command that opens the store via `open_store`, walks the ledger, and
  writes files. `ekos-docs-gen` already renders the Transformation IR's `FeedsInto` DAG "for free"
  as a generic Mermaid graph — proof the IR is a solid rendering substrate for a second format.

## Scope

Render already-compiled `Custom("TransformNode")` KIR objects (from any source currently lowered
through the Transformation IR — Pentaho today, SQL views tomorrow, no new coupling) into a dbt
project skeleton: one `.sql` model per node, `ref()`/`source()`-chained via real `FeedsInto`
edges, plus a generated `schema.yml`.

## Non-goals

- No expression transpilation / no cross-dialect AST (matches RFC 0027/0028's explicit rejection
  of this scope) — `Filter`/`Calculate` render as flagged raw text, not translated SQL.
- Not a new extraction/recovery pass — reads what's already compiled, same as `ekos-docs-gen`.
- Not `.kjb` job-orchestration translation — dbt's own `ref()` graph already encodes DAG order
  from `FeedsInto`; `.kjb` entries are `Unmapped` by design and stay that way here too.
- Not a dbt *inbound* connector (parsing existing dbt projects as a recovery source) — that is the
  unrelated, already-tracked `TODO.md` backlog item; this RFC is the export direction only.

## What already exists and is reused

- `ekos_kir::{KirObject, KirRelationship, KirId, ObjectKind, RelationshipKind}` — read-only, same
  types `ekos-docs-gen` already renders from.
- `Custom("TransformNode")` objects + `Custom("FeedsInto")` relationships (RFC 0027's
  `lower_to_kir`) — the entire input to this feature; nothing new is compiled.
- `open_store` (`crates/cli/src/commands/store.rs`) — the same backend-auto-detecting store opener
  every post-commit CLI command already uses.
- The `ekos-docs-gen` + `commands/docs.rs` + `Commands::Docs{subcommand}` shape — copied 1:1 for a
  new `ekos-dbt-gen` crate + `commands/dbt.rs` + `Commands::Dbt{subcommand}`.

## Design

- **New crate, `ekos-dbt-gen`** (`crates/dbt-gen/`) — pure rendering over `&KirObject` +
  resolved upstream model names, zero I/O, mirroring `ekos-docs-gen`'s shape.
- **`dbt_model_name(object: &KirObject) -> String`** — snake_case slug of the object's name (e.g.
  `fact_sales.ktr:10` → `fact_sales_ktr_10`), dbt's own model-naming convention.
- **Per-`TransformNode` rendering** (`render_dbt_model`), honest passthrough for the untranslatable
  cases (confirmed with the user before implementation):
  - `Source` → `select {columns, or * if none} from {{ source('<group>', '<object_name>') }}`
  - `Sink` → `select * from {{ ref('<upstream>') }}`, aliased to the sink's own model name
  - `Join` → `select * from {{ ref(a) }} as l {kind} join {{ ref(b) }} as r on l.k1 = r.k2 ...` —
    upstream refs resolved via inbound `FeedsInto` edges, never via `Join`'s own `left`/`right`
    fields (per the analyzer's own documented unreliability); a comment flags that side-assignment
    (`a`/`b`) is positional, not guaranteed to match the original Kettle step's left/right when
    there are exactly two inbound edges
  - `Aggregate` → `select {group_by}, {func}({arg}) as {output}, ... from {{ ref(upstream) }}
    group by {group_by}`
  - `Filter` → `select * from {{ ref(upstream) }} where {condition} -- TODO: verify, source
    dialect: Pentaho`
  - `Calculate` → `select *, {expr} as {output} -- TODO: verify, source dialect: Pentaho from
    {{ ref(upstream) }}`
  - `Unmapped` → `-- Unmapped: {reason}` + raw XML as a comment block + `select * from
    {{ ref(upstream) }} -- passthrough stub, not translated` — still `ref()`-chained so the DAG
    stays connected end to end, matching `ekos-docs-gen`'s "Unmapped is a citizen, not a failure"
    contract applied to a second output format
  - A node with no resolvable upstream (a root `Filter`/`Calculate`/`Join`/`Aggregate`/`Sink`/
    `Unmapped` — shouldn't normally happen, but the ledger is arbitrary user-recovered data) gets
    an honest placeholder (`select 1 as placeholder -- no upstream FeedsInto edge found`) rather
    than a panic or a silently invalid `ref()` call.
- **`schema.yml` generation** — one `sources:` entry per distinct `Source` node's `object_name`,
  grouped under one source name; a `models:` entry per generated model. Hand-rolled string
  building (matching `ekos-docs-gen`'s own Markdown/HTML hand-rolling), not a new YAML-serializer
  dependency.
- **New CLI command, `ekos dbt generate`** (`crates/cli/src/commands/dbt.rs`) — opens the store via
  `open_store`, filters to `Custom("TransformNode")` objects, resolves each node's upstream model
  names via inbound `FeedsInto` relationships, renders + writes one `.sql` file per node plus one
  `schema.yml`, grouped by `TransformOrigin.source_path` into a `models/<job>/` subdirectory per
  originating file — the same "post-commit CLI verb reading the committed ledger" shape as
  `ekos docs generate` and `ekos identity scan`, not a `PassManager` pass.

## Alternatives Considered

- **Attempt real expression transpilation** (Kettle pseudo-syntax / SQL text → dbt SQL) — rejected,
  same reasoning RFC 0027/0028 already used to reject a shared expression AST; would silently
  produce wrong SQL for anything non-trivial, violating the project's evidence-first ethos.
- **Skip untranslatable nodes entirely** — rejected (user's explicit choice when this RFC was
  scoped); breaks the `ref()` chain and produces a set of disconnected fragments instead of a
  runnable-shaped project.
- **Fold this into `ekos-docs-gen` as another output format** — rejected; dbt output is executable
  SQL with different structural rules (per-node files, `ref()`/`source()` macros, `schema.yml`)
  from a documentation renderer, closer to a sibling crate than a third `render_*_page` function.

## Open Questions

- [ ] dbt adapter/dialect targeting (Postgres vs Snowflake vs generic ANSI) for generated SQL —
      proposed default: generic ANSI SQL, no adapter-specific functions, since Pentaho's own
      `TableInput`/`TableOutput` steps don't record a target warehouse dialect either.
- [ ] Whether `schema.yml` should also emit dbt `tests:` (`not_null`/`relationships`) — out of
      scope for Phase 1, deferred to Phase 3.
- [ ] Model materialization strategy (`view` vs `table` vs `incremental`) — proposed default:
      `view` for all generated models (safest, no assumptions about volume/freshness needs).

## Testing

- Golden-file tests per `TransformNode` variant asserting exact generated SQL text.
- A `FeedsInto`-chain test asserting `ref()` calls resolve to the correct upstream model names,
  including through an `Unmapped` node (chain must not break).
- CLI-level tests (mirroring `docs.rs`'s style) using a fixture ledger, asserting the right number
  of `.sql` files + one `schema.yml` are written, and that every node type (including `Unmapped`
  and a no-upstream root node) renders without panicking.

## Acceptance Criteria

- [x] All Open Questions resolved or explicitly deferred with rationale.
- [x] Design reviewed against the codebase before implementation (this RFC's own "What already
      exists and is reused" section).
- [x] `ekos dbt generate` runs end-to-end against a fixture workspace and produces a valid-looking
      dbt project structure (`.sql` files + `schema.yml`) with zero panics on every node type
      including `Unmapped` and no-upstream roots. Verified against a real compiled SQL schema
      (real `CREATE VIEW ... JOIN ... WHERE ... GROUP BY`, 6 real Transformation IR nodes, 100%
      mapped) — this real run surfaced and fixed a genuine bug: SQL-sourced join keys already
      carry the original query's own table alias (`o.customer_id`), unlike Pentaho's bare column
      names, so the initial `l.`/`r.` alias-prefixing double-qualified them into invalid SQL.
      Fixed by passing key text through unmodified with a verify comment, matching Filter/
      Calculate's honesty contract; regression test added.
- [x] Golden-file tests pass for every `TransformNode` variant (20 unit tests in `ekos-dbt-gen`,
      5 CLI-level tests in `commands/dbt.rs`).
- [x] Design is consistent with `ekos.md`'s compiler architecture and `CLAUDE.md`'s key invariants
      (evidence-traceable, deterministic, side-effect-free rendering) — `cargo clippy --workspace
      -- -D warnings` and `cargo fmt --check` both clean for every file this RFC touched.

## Implementation Plan

**Phase 1 — Core renderer + CLI command.** `ekos-dbt-gen` crate: per-node-type SQL rendering (all
7 variants, including the honest `Filter`/`Calculate`/`Join`/`Unmapped` caveats), `FeedsInto`-based
`ref()` resolution, `schema.yml` generation. `crates/cli/src/commands/dbt.rs` +
`Commands::Dbt{subcommand}` wiring, same plumbing shape as `docs.rs`. Golden-file + CLI-level
tests.

**Phase 2 — Real Pentaho smoke test.** Run `ekos dbt generate` against the same real cloned
Pentaho repo used for RFC 0035's testing, inspect real output for correctness/readability, likely
surfacing real gaps the way RFC 0035's real-data testing did.

**Phase 3 — dbt `tests:` generation.** Emit `not_null`/`relationships` schema tests from
evidence-backed relationship data, once Phase 1/2 prove the base model generation is sound.

Each phase ships with its own tests before the next starts, matching `CLAUDE.md`'s mandatory
Tests-before-Implementation workflow discipline.

## Files Changed (Phase 1)

| File | Change |
|---|---|
| `ekos/crates/dbt-gen/Cargo.toml` | new |
| `ekos/crates/dbt-gen/src/lib.rs` | new — rendering logic |
| `ekos/crates/cli/src/commands/dbt.rs` | new — CLI plumbing |
| `ekos/crates/cli/src/commands/mod.rs` | `+pub mod dbt;` |
| `ekos/crates/cli/src/bin/ekos.rs` | `+Commands::Dbt{subcommand}` |
| `ekos/crates/cli/Cargo.toml` | `+ekos-dbt-gen.workspace = true` |
| `ekos/Cargo.toml` | `+crates/dbt-gen` member, `+ekos-dbt-gen` workspace dependency |

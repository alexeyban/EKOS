# Devlog 35 — RFC 0036 Phase 1: Pentaho → dbt model export

**Date:** 2026-08-07
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Implemented Phase 1 of RFC 0036 (Pentaho → dbt Model Export): a new `ekos dbt generate` command
that renders every already-compiled `Custom("TransformNode")` object (RFC 0027's Transformation
IR) as a dbt `.sql` model, `ref()`/`source()`-chained via real `FeedsInto` relationships, plus a
generated `schema.yml`. This is a second rendering target over the same compiled graph RFC 0035's
`ekos-docs-gen` already reads — no new extraction pass, pure rendering, zero LLM calls.
Structurally rich node types (`Source`/`Sink`/`Join`/`Aggregate`) render real SQL from real
compiled table/column/join-key/group-by data; `Filter`/`Calculate` inline their raw source-dialect
expression text behind a `-- TODO: verify` comment rather than attempting a transpile RFC 0027
already scoped out; `Unmapped` nodes render a stub carrying the raw source and reason as a comment
while still resolving a real upstream `ref()` — the "Unmapped is a citizen, not a failure"
contract from RFC 0035, applied to a second output format. Verified end-to-end against a real
compiled SQL schema (a real `CREATE VIEW ... JOIN ... WHERE ... GROUP BY`, 6 real Transformation
IR nodes, 100% mapped), which surfaced and fixed a real bug before it shipped.

---

## RFC 0036 Phase 1 — core renderer + CLI command

### Problem / motivation

RFC 0035 turned the compiled Transformation IR into human-readable documentation. The user asked
whether the same compiled graph could become something executable, not just readable — dbt is the
most common target for retiring a legacy Pentaho ETL job onto a modern stack. `TransformNode`
objects already carry structured table names, join keys/kind, and group-by/aggregate data, so
rendering them as dbt models is answerable directly, without any new recovery/extraction work.

Before writing any code, the feasibility ceiling was investigated directly against the source:
`Filter.condition`/`Calculate.expr` are raw, un-parsed Kettle-dialect text (RFC 0027 explicitly
rejected a cross-format expression AST as out of scope), and 49% of a real Pentaho repo's steps
came back `Unmapped` in RFC 0035's own real test. The user was asked directly how to handle these:
the chosen approach was an "honest passthrough stub" — every node gets a real `.sql` file either
way, untranslatable content is flagged rather than guessed at or silently dropped, and the `ref()`
chain always stays connected end to end.

### What was built

| Component | Location |
|---|---|
| Rendering crate | `ekos/crates/dbt-gen/` (new) — pure function, no I/O |
| CLI command | `ekos/crates/cli/src/commands/dbt.rs` (new) — `ekos dbt generate` |
| Clap wiring | `ekos/crates/cli/src/bin/ekos.rs` — `Commands::Dbt` / `DbtCommands::Generate` |
| RFC | `ekos/docs/rfcs/0036-pentaho-to-dbt-export.md` (new) |

`ekos-dbt-gen`'s public surface: `is_transform_node`/`is_feeds_into` (the same filtering role
`ekos_docs_gen::is_significant` plays), `dbt_model_name` (snake_case slug, e.g.
`fact_sales.ktr:10` → `fact_sales_ktr_10`), `upstream_model_names` (resolves a node's upstream
models via inbound `FeedsInto` edges — never via a `Join` node's own `left`/`right` fields, which
`pentaho_analyzer.rs`'s own code comments flag as unreliable placeholders), `render_dbt_model`
(the per-node-type SQL renderer), and `render_schema_yml` (hand-rolled string building, matching
`ekos-docs-gen`'s own Markdown/HTML hand-rolling — no new YAML-serializer dependency).

`commands/dbt.rs` mirrors `commands/docs.rs`'s plumbing shape exactly: `open_store` → filter
`all_objects()` to `TransformNode`s → build a `KirId → model_name` map → per node, resolve
`relationships_for` into upstream model names, render, write.

### Implementation details worth remembering

- **Per-node-type rendering, honest where the IR is opaque**: `Source`/`Sink` wrap `{{ source() }}`
  /`{{ ref() }}` calls with real column lists; `Aggregate` renders real `group by`/agg-func SQL;
  `Filter`/`Calculate` inline their raw compiled text behind `-- TODO: verify, source dialect:
  Pentaho`; `Unmapped` renders `-- Unmapped: {reason}` plus the raw XML as a comment block, then
  still emits `select * from {{ ref(upstream) }}` so the pipeline shape survives even through a
  node with zero structured semantics.
- **A node with no resolvable upstream** (shouldn't normally happen, but the ledger is arbitrary
  user-recovered data) renders `select 1 as placeholder -- no upstream FeedsInto edge found`
  rather than panicking or emitting a broken `ref()` call to nothing.
- **Two-upstream `Join` gets an explicit side-ambiguity comment**: because `FeedsInto` edge order,
  not the `Join` node's own `left`/`right` fields, decides which upstream becomes `l` and which
  becomes `r`, a real two-input join always gets `-- NOTE: l/r assignment is positional ...`
  rather than silently implying the original Kettle step's sidedness was preserved.

### Testing

19 unit tests in `ekos-dbt-gen` (later 20, see the bug below) covering every `TransformNode`
variant's rendering, `upstream_model_names`'s `FeedsInto`-only filtering (a non-`FeedsInto`
relationship touching the same node must never leak in as an upstream — tested explicitly), and a
`FeedsInto`-chain test resolving correctly through an `Unmapped` node (mirroring the real
`dim_customer.ktr` shape from RFC 0035's real Pentaho test: source → unmapped → sink must not
break the `ref()` chain on either side). 5 CLI-level tests in `commands/dbt.rs` mirroring
`docs.rs`'s fixture-ledger test style. `cargo clippy --workspace -- -D warnings` and `cargo fmt
--check` both clean for every file this RFC touched.

### A real bug found by real-data testing (not caught by unit tests)

Unit tests used Pentaho-shaped fixture data (bare column names like `customer_id`/`id`) — every
one passed. Running `ekos dbt generate` against a *real* compiled SQL schema (a `CREATE VIEW ...
JOIN ... GROUP BY` recovered by `sql_transform_analyzer`, not Pentaho) surfaced a real defect: SQL-
sourced join keys already carry the original query's own table alias (`o.customer_id`), unlike
Pentaho's bare column names. The initial renderer unconditionally prefixed join keys with an
invented `l.`/`r.` alias, producing `l.o.customer_id = r.c.id` — invalid SQL, double-qualified.

Fixed by passing key text through exactly as compiled, with a `-- TODO: verify column
qualification, source dialect: Pentaho` comment rather than either alias-prefixing scheme, since
the correct handling is genuinely dialect-dependent and the IR doesn't currently distinguish
"already-qualified" from "bare" key text. A regression test
(`join_node_never_double_qualifies_already_aliased_sql_source_keys`) pins the exact real inputs
that surfaced this. Same pattern as RFC 0035's own real-data bugs (raw-id relationship rendering,
the `ask()` full-sentence-buries-name citation bug) — real testing against real compiled data
keeps finding gaps unit tests with hand-built fixtures miss, because the fixtures encode the
author's own assumptions about the data shape.

### Decisions (alternatives considered, why this choice)

- **Honest passthrough stub over skipping untranslatable nodes** — user's explicit choice.
  Skipping `Filter`/`Calculate`/`Unmapped` nodes would break the `ref()` chain and produce
  disconnected fragments instead of a runnable-shaped project; the chosen approach keeps every
  model file real and the DAG always connected, at the cost of some models needing manual
  verification before they're production-correct.
- **New sibling crate over folding into `ekos-docs-gen`** — dbt output is executable SQL with
  different structural rules (per-node files, `ref()`/`source()` macros, `schema.yml`) from a
  documentation renderer; closer to a sibling crate than a third `render_*_page` function.
- **No expression transpilation** — matches RFC 0027/0028's own explicit rejection of a
  cross-format expression AST; attempting one would risk silently wrong SQL, which conflicts with
  the project's evidence-first ethos more than an honestly-flagged TODO comment does.

---

## Knowledge Captured

- **`Join` node `left`/`right` fields are Pentaho-analyzer placeholders, not real topology** —
  confirmed directly in `pentaho_analyzer.rs`'s own code comments (steps are parsed in document
  order, single pass, no forward lookup table across the whole graph yet). Any future consumer of
  `TransformNode::Join` must resolve upstream/downstream via the graph-level `FeedsInto` edges,
  never via the node's own `left`/`right` `NodeId` fields, for both Pentaho- and (as this session
  found) SQL-sourced graphs.
- **Join key text is dialect-dependent in whether it's already table-qualified** — Pentaho's
  `DatabaseJoin`/`MergeJoin`/`StreamLookup` produce bare column names; `sql_transform_analyzer`
  produces keys that already carry the original query's table alias. Any future renderer over
  `TransformNode::Join.keys` needs to either not assume either shape, or have the IR itself record
  which shape a given graph's keys are in — not solved by this RFC, flagged as a real Open
  Question worth a future RFC 0027 amendment if a third consumer of `Join.keys` needs to
  disambiguate this itself.
- **Real-data testing is still the primary bug-finding mechanism in this project**, third time
  running: RFC 0035's raw-id rendering bug, RFC 0035's citation-emptying bug, and now this
  session's join-key double-qualification bug were all invisible to hand-built unit test fixtures
  and only surfaced once real compiled data (not synthetic, author-authored inputs) was run
  through the new code.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0036-pentaho-to-dbt-export.md` | new — full RFC with Implementation Plan |
| `ekos/crates/dbt-gen/Cargo.toml` | new |
| `ekos/crates/dbt-gen/src/lib.rs` | new — per-node-type SQL rendering, `schema.yml`, 20 tests |
| `ekos/crates/cli/src/commands/dbt.rs` | new — `ekos dbt generate` CLI plumbing, 5 tests |
| `ekos/crates/cli/src/commands/mod.rs` | `+pub mod dbt;` |
| `ekos/crates/cli/src/bin/ekos.rs` | `+Commands::Dbt{subcommand}` / `DbtCommands::Generate` |
| `ekos/crates/cli/Cargo.toml` | `+ekos-dbt-gen.workspace = true` |
| `ekos/Cargo.toml` | `+crates/dbt-gen` member, `+ekos-dbt-gen` workspace dependency |

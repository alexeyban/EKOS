# RFC 0096 — EKL: `AS OF <timestamp>` and `COUNT`/`GROUP BY` aggregation

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

`docs/GAP_ANALYSIS.md` (written this session, synthesized from `TODO.md`'s continuous RFC-Non-Goals
survey) named EKL's missing `AS OF <timestamp>` historical syntax and missing `COUNT`/`GROUP BY`
aggregation as open backlog, restated across RFC 0010 (EKL itself) without ever being closed. Both
gaps had the same shape: the underlying primitive existed and worked (`Runtime::reconstruct_state_at`
→ `KnowledgeStore::object_at`/`relationships_at`, RFC 0047), but only at single-id granularity —
nothing in `Runtime` or `KnowledgeStore` could answer "every object as it existed at time T," and
EKL's grammar had no clause to ask for it at all. `COUNT`/`GROUP BY` had no primitive gap — `execute`
already produces a filtered `Vec<Row>` before projection, aggregation is a pure function over that —
but no grammar or interpreter path existed to reach it.

A real `JOIN` across Object+Relationship in one query (the third item in the same gap-analysis
paragraph) is deliberately **not** attempted here — see Non-goals.

## Design

### New bulk point-in-time primitive: `KnowledgeStore::all_objects_at`/`all_relationships_at`

`object_at`/`relationships_at` (RFC 0047) already reconstruct one entity's state at a point in time,
on both backends. Bulk `AS OF` needs the same reconstruction across every entity at once:

- **SQLite `Ledger`**: a correlated subquery — per distinct `id`, the row with the greatest
  `written_at` (tie-broken by `rowid`, same ordering `object_at`'s own `ORDER BY written_at DESC,
  rowid DESC LIMIT 1` already uses) among rows with `written_at <= at`. One query, no per-id loop.
- **`FactLedger`**: `all_current_payloads` (the existing bulk "current state" reader — one sequential
  pass over the EAVT runs plus the memtable, `fold_state(entity, &entries, None)` per entity) is
  generalized to `all_payloads_at(cut: Option<TxId>)`, threading a real cut through `fold_state`
  instead of always folding to "now." `all_current_payloads` becomes `all_payloads_at(None)`. Same
  one-pass shape, same cost, now parameterized by time.

Both new methods added to the `KnowledgeStore` trait and the `delegate_store!` macro (the seam RFC
0016 established between the two backends), plus `Runtime::list_objects_at`/`list_relationships_at`
wrappers matching `list_objects`/`list_relationships`'s existing shape exactly.

### EKL grammar: `AS OF '<rfc3339>'`, `COUNT`, `GROUP BY <field>`

All three fit the parser's existing flat clause loop (`parser.rs:3-5`'s own design note: "the
grammar is six flat clause types with no recursive expression precedence") without changing its
shape — three more order-independent optional clauses, same as `FROM`/`VIA`/`DEPTH`/`RETURN`/`ORDER
BY`/`LIMIT` already are. `AS OF` reuses the existing string-literal token (already lexed correctly —
`:` inside a quoted string was never treated as operator syntax) and parses it via
`DateTime::parse_from_rfc3339`, erroring with position info like every other parse failure in this
grammar. Two new validations, matching `VIA requires FROM`'s existing style exactly: `GROUP BY
requires COUNT`, and `COUNT` is rejected combined with `RETURN` (aggregate rows have a different
shape than projected entity rows — silently ignoring `RETURN` would be worse than a clear parse
error).

### Interpreter: `AS OF` swaps the read path; aggregation is a pure post-filter step

`candidate_rows` checks `ast.as_of` first: when set, it calls `list_objects_at`/
`list_relationships_at` instead of `list_objects`/`list_relationships`, and returns
`EklError::AsOfWithFromUnsupported` if `FROM` is also present — `FROM`-anchored expansion
(`load_neighborhood`/`trace_impact`) has no time-aware equivalent yet (see Non-goals), and silently
running it against current-state neighborhood data under a nominally-historical query would be a
worse failure than a clear, explicit rejection.

`COUNT`/`GROUP BY` aggregation runs after predicate filtering, before `ORDER BY`/`LIMIT`/projection.
**A design simplification found while implementing, not in the original plan**: aggregate output
doesn't need a new `EklResult` variant. A grouped count is already expressible as an ordinary `Row`
(`{"<field>": key, "count": N}`); a bare count is a one-row table (`{"count": N}`). Reusing the
existing flat `Vec<Row>` contract means `ORDER BY`/`LIMIT` (which operate generically on any `Row`,
not on entity-specific fields) keep working unchanged on aggregate output with zero new code, and
`ekos_ekl`'s MCP tool (`mcp.rs`) needed no changes at all — it already JSON-serializes `result.rows`
directly.

### CLI rendering fix found live, not anticipated in the original plan

`ekl.rs`'s tabular (non-JSON) output picks display columns via `default_returns(&ast.entity)` when
`RETURN` is absent — `["id", "name", "kind"]` for `Object`. For an aggregate query, none of those
keys exist on the result rows at all: the count column would have silently rendered as empty cells
with no visible `count` column anywhere. Found and fixed before this reached the user: aggregate
queries now select `[group_field, "count"]` or `["count"]` as their display columns instead.

## Non-goals

- **A real `JOIN` across Object+Relationship in one query.** This is the one extension that
  actually strains the "six flat clause types" design ethos the parser's own header comment calls
  out — it needs a combined row schema `object_row`/`relationship_row`'s current per-entity split
  can't produce without a real redesign, not another flat clause. Left for its own future RFC once
  real EKL usage shows whether a join is actually needed.
- **`AS OF` combined with `FROM`-anchored expansion.** `load_neighborhood`/`trace_impact` have no
  time-aware equivalents; teaching them one is real, separate work (both would need an `at`
  parameter threaded through `Runtime`, and `expand_from_anchor`'s `KirGraph` reassembly would need
  to reconstruct historical relationship endpoints, not just filter current ones). Rejected
  explicitly (`EklError::AsOfWithFromUnsupported`) rather than silently ignored.
- **`SUM`/`AVG`/other aggregate functions.** `COUNT` was the concrete gap named in the survey;
  numeric aggregates over KIR object/relationship fields are a different, unscoped ask with no
  driving use case yet.

## Verification

18 new unit tests: 6 in `crates/ledger` (3 per backend, both `all_objects_at`/`all_relationships_at`
— empty-before-anything-written, correctly excludes a later update, correctly includes everything
current), 8 new parser tests (`AS OF` parse + malformed-timestamp rejection, bare `COUNT`, `COUNT
GROUP BY`, `GROUP BY` without `COUNT` rejected, `COUNT` + `RETURN` rejected, plus 6 new fuzz seeds),
7 new interpreter tests (`AS OF` before/at/reconstructing-a-past-version, `AS OF` + `FROM` rejected,
bare `COUNT`, `GROUP BY` grouping correctness, `GROUP BY` respecting `WHERE` first, `GROUP BY` +
`LIMIT`). Full workspace gate clean (`cargo fmt`, `build --workspace`, `clippy --workspace -D
warnings`, `test --workspace` — 103/103 test groups), `tests/integration` 3/3.

Live-verified against this repo's own real, already-committed self-analysis ledger (687 files, real
compiled objects — not a scratch fixture): `ekos ekl "FIND Object COUNT GROUP BY kind LIMIT 10"`
renders a real per-kind breakdown (`Claim 472`, `Crate 46`, `Document 267`, `File 687`, ...);
`AS OF '<now>'` produces the identical `Crate` count (46) as a plain current-state query; `AS OF
'2020-01-01T00:00:00Z'` (before this workspace's ledger existed) correctly returns `count: 0`, not
an error or stale data.

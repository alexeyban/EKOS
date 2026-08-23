# Devlog 89 — Real Backend→Database cross-tier edges (RFC 0086), Phase 6 (final phase) of the docs quality plan

**Date:** 2026-08-23
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Sixth and final phase of the source-decomposition plan — the plan's own explicitly-scoped
"stretch" item. Real Backend→Database edges from Ecto Repo adapter declarations, closing out
`## System Decomposition`'s last gap: real layer boxes with real edges between them, not just
boxes sitting next to each other. Frontend→Backend, per the plan's own original scoping, stays
deliberately unattempted — genuinely lower confidence, correctly left alone.

## RFC 0086

Checked the real source before designing anything (this session's now-consistent practice):
`lib/plausible/repo.ex` and four sibling files all declare `use Ecto.Repo, adapter:
Ecto.Adapters.Postgres`/`ClickHouse` directly inside the module body — no separate config file
needed. Extended `elixir_analyzer.rs` (not a new pass) to track "has this module seen `use
Ecto.Repo`" and, on a later `adapter:` line in the same module, emit a real `Technology` object +
`DependsOn` edge, reusing `dependency_analyzer.rs`'s own "PostgreSQL" naming convention so the two
analyzers' output resolves to one real object rather than two.

`docs-gen` needed two small, real additions to `system_decomposition_graph`: a one-hop `Contains`
inheritance so a Backend `File`'s real `ElixirModule` also resolves to `Layer::Backend` for edge
purposes (deliberately *not* used for the displayed file counts, which stay exactly the real
`File` numbers — a module living inside a file must not double-count it), and a bucket that routes
a real database-adapter `Technology` into the same `layer_sql`/`layer_clickhouse` node a matching
`Table` object would use.

## A real gap caught before shipping, again

Same discipline as Phase 5's JSX bug: didn't just ship the happy path. The real analytics project
declares **five** separate ClickHouse-adapter Repo modules — running the naive version would have
re-pushed five duplicate "ClickHouse" `Technology` objects into one `KnowledgeArtifact`. Caught by
re-reading `elixir_analyzer.rs`'s own existing cross-file dedup condition (already special-cased
for `ElixirModule`) and noticing it didn't cover the new `Technology` kind — fixed by extending the
same condition, with a comment naming the real 5-repo count as the concrete reason, not an
abstract "just in case."

A second real, honest finding, this time in the render layer: ClickHouse is really configured in
this project (5 real Repo modules), but **no ClickHouse `Table` schema has ever been recovered**
for it (`clickhouse_analyzer.rs` needs a live ClickHouse connection this workspace doesn't have
configured). Rather than fabricate a table count or silently drop the edge, `docs-gen` now renders
an honest `"ClickHouse Database (config only, no tables compiled)"` label — a real, meaningful
distinction between "we know this dependency exists" and "we've also compiled its schema."

## Live verification

Confirmed via `ekl`: 6 real ClickHouse-adapter modules all resolve to the *same* real "ClickHouse"
object (not 6 duplicates — the dedup fix holds), 3 real Postgres-adapter modules resolve to the
same real "PostgreSQL" object. `Architecture.md`'s `## System Decomposition` now draws real arrows:
`Backend → SQL Database (57 tables)` and `Backend → ClickHouse Database (config only, no tables
compiled)` — the first real cross-tier relationship line this whole plan has produced, and the
honest label distinguishes the two real database dependencies correctly instead of treating them
identically.

## Plan complete

This closes all six phases of the "Deep Source Decomposition + Production-Grade Architecture
Diagrams" plan (`/home/legion/.claude/plans/1-prove-the-core-memoized-wren.md`), started after
finding the generated docs for this same real project unprofessional and unreadable. Real backend
decomposition (Elixir), real frontend decomposition (JS/TS), real database data, a real System
Decomposition view tying them together with real edges, and the diagram-readability bugs that
made the original System Context view unreadable are all fixed and live-verified against real
data. RFCs 0081–0086, devlogs 84–89.

## Knowledge Captured

- **Extending an existing analyzer pass is often the right call over adding a new one** — Ecto
  Repo adapter detection is real Elixir source structure, exactly `elixir_analyzer.rs`'s own
  domain; no new pass, no new plugin, no new observer was needed, just ~40 real lines added to an
  existing, already-tested file.
- **A cross-file object dedup condition written for one object kind doesn't automatically cover a
  second kind added later** — worth explicitly checking existing dedup/merge logic for "does this
  new object kind actually hit this code path" whenever a new object kind is added to a pass that
  already merges multiple files' output, not just assuming a new kind is automatically safe.
- **"No data compiled" and "compiled zero of it" are different real states worth labeling
  differently** — a real Ecto config existing with zero real `Table` rows behind it is not the
  same fact as no database dependency existing at all; the render layer's honest fallback label
  keeps that distinction visible instead of collapsing both into the same silence.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0086-backend-to-database-cross-tier-edges.md` | New RFC |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | Ecto adapter detection + dedup fix; 4 tests |
| `ekos/crates/docs-gen/src/lib.rs` | Cross-tier edge resolution + honest config-only label; 3 tests |
| `TODO.md` | Phase 6 marked done; whole decomposition plan marked complete |
| `devlogs/devlog_89.md` | This file |

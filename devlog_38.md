# Devlog 38 — Pentaho → dbt deck + 3 real recovery-layer bugs found building it

**Date:** 2026-08-07
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Built a new presentation deck (`docs/presentations/dbt-transformation.html`) pairing real Pentaho
`.ktr` source XML against real generated dbt SQL models — a different exercise than Phase 2's
smoke test, which only read the generated `.sql` files in isolation. Pairing source against
target surfaced three further real bugs, all upstream in `pentaho_analyzer.rs` (shared with
`ekos docs generate`, not specific to dbt export): `TableInput` steps never read their real
declared columns, and both real Kettle join step types (`StreamLookup`, `MergeJoin`) never read
their real join keys, because each uses a genuinely different XML shape from the one the analyzer
originally only checked (`DatabaseJoin`'s). All three fixed, with regression tests built from real
verified XML shapes, not assumptions. This is the fourth round of real-data bug-finding this
session (after RFC 0035's Phase 1, RFC 0036/0037's Phase 2) — and the first time the bug was
upstream of both features that consume the Transformation IR, not local to either renderer.

---

## What was found and fixed

### `TableInput` → `Source.columns` was always empty

`pentaho_analyzer.rs`'s `TableInput` mapping set `columns: Vec::new()` unconditionally — nothing
ever populated it, even though `ekos_dbt_gen::render_source` already had correct logic to emit an
explicit column list when `columns` was non-empty. The real column data was sitting in the same
step's XML the whole time, under `<row-meta>/<value-meta>/<name>` — structurally identical in
spirit to `TableOutput`'s already-correct `<fields>/<field>/<stream_name>` read, just a different
tag Kettle uses for input vs. output steps. Fixed by reading it. A real `Sales.SalesPerson` read
now renders `select BusinessEntityID, TerritoryID from {{ source(...) }}` instead of `select *`.

### `StreamLookup` and `MergeJoin` both compiled every real join with empty `keys`

The original code comment for this module was explicit that no real `.ktr`/`.kjb` sample was
available when it was written — `DatabaseJoin`'s documented `<keys><key><value1>/<value2></key>
</keys>` shape was used as the template for all three join-producing step types. Verified against
a real repo, this assumption was wrong for both step types actually present in real jobs:
- `StreamLookup`'s real shape is `<lookup><key><name>/<field></key></lookup>` — `<field>` names
  the column on the upstream (main) stream, `<name>` names the column on the lookup stream.
- `MergeJoin`'s real shape is two separate lists, `<keys_1><key>ColA</key></keys_1>` and
  `<keys_2><key>ColB</key></keys_2>`, paired positionally, not `<value1>`/`<value2>` pairs inside
  one list.

Neither had ever been checked, so `extract_join`'s single `<keys>` lookup silently returned empty
for both — the join *kind* and *topology* were still real and correct, only the key columns were
missing. Fixed with a shared `extract_join_keys` (tries `DatabaseJoin`'s shape first, falls back
to `MergeJoin`'s) and a dedicated `extract_stream_lookup` reading the real `<lookup>` shape
directly instead of delegating to `extract_join`.

### A real ledger-contamination hazard, hit again

Re-running `recover` against the *original* Pentaho scratch workspace (reused from Phase 2) to
verify these fixes produced alarming numbers — "103 SQL files analysed" and "0 Pentaho nodes" —
because that workspace's `ekos.toml` never had `dbt-generated`/`docs-curated` added to
`ignore-patterns`, and a `build` from an earlier, now-compacted session had already re-ingested
those generated output directories as bogus `File`/SQL objects into the ledger. Per this project's
established rule (the ledger is append-only — this can't be un-ingested), the fix was the same one
used earlier this session for the exact same class of problem: throw the contaminated workspace
away, clone the real repo fresh, add every known output directory to `ignore-patterns` *before*
the first `build`, and re-verify there. The original contaminated workspace was left untouched
(it's disposable scratch, not part of the repo) rather than touched further.

---

## The deck

`docs/presentations/dbt-transformation.html` — 10 slides pairing real Pentaho `.ktr` XML excerpts
against real generated dbt `.sql` models: the command, a real `Source` (now with real columns), a
real `Join` (now with real keys, both step types), a real `Unmapped` passthrough, a real `Sink` +
`schema.yml` excerpt, the Filter/Calculate honest-passthrough contract, a "five real bugs" slide
(the two from Phase 2's smoke test plus the three found building this deck), and real summary
numbers. 6 real generated files published under `docs/presentations/examples/dbt/`, linked
directly from the deck. Added to `presentations.html`'s list.

---

## Knowledge Captured

- **Reading generated output in isolation (Phase 2's smoke test) and pairing it against its real
  source (building this deck) are different verification exercises that catch different bugs.**
  Phase 2 read `.sql` files and caught a wording bug in the *rendering* layer. Pairing source XML
  against target SQL side-by-side made an entire *recovery*-layer gap (empty columns, empty keys)
  immediately visually obvious in a way that reading isolated `.sql` output didn't — the deck build
  process itself was a debugging tool, not just a presentation exercise.
- **`pentaho_analyzer.rs`'s three join-producing step types each use a genuinely different Kettle
  XML shape for their keys.** Any future consumer of `TransformNode::Join` should be aware
  `extract_join_keys`/`extract_stream_lookup` already handle all three real shapes found in a real
  repo; `DatabaseJoin`'s shape specifically is still unverified against a real sample (none
  appeared in the test repo) and remains a documented approximation.
- **Ledger contamination from un-ignored output directories is a recurring, not one-off, risk in
  this project** — this is at least the second time in this session's history the same failure
  mode has appeared. Any workspace used for repeated real-data testing across multiple sessions
  needs every generated-output directory name added to `ignore-patterns` *before* the first
  `build`, and if contamination is ever suspected, the answer is always a fresh clone, never an
  attempt to clean an append-only ledger.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/pentaho_analyzer.rs` | Fix: `TableInput` reads real `<row-meta>` columns; new `extract_join_keys` handles `DatabaseJoin`+`MergeJoin` shapes; `extract_stream_lookup` reads the real `<lookup>` shape directly; 2 new regression tests, 1 existing test corrected to a real-verified fixture; module doc comment updated |
| `docs/presentations/dbt-transformation.html` | new — Pentaho → dbt deck with real source/target pairs |
| `docs/presentations/examples/dbt/*` | 6 real generated files published, linked from the deck |
| `docs/presentations.html` | `+` entry for the new deck |
| `ekos/docs/rfcs/0036-pentaho-to-dbt-export.md` | Phase 2b added, documenting the three recovery-layer fixes |

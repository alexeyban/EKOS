# RFC 0075 — Data Architecture Cross-Referencing: Table↔TransformNode Links, Data Domains

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0074 shipped a real Data Architecture view but explicitly documented four gaps rather than
faking them: `TransformNode` Source/Sink nodes weren't linked to the real `Table` object they
name; Data Domains, Ownership, Lifecycle, and Data Quality had no compiled signal at all. Asked
directly to close these follow-ons. This RFC closes the first (Table↔TransformNode linking) and
Data Domains with real shipped code, and turns Ownership/Lifecycle from a vague "not yet computed"
into a precise, correctly-diagnosed blocker — including a real factual correction to RFC 0074's own
text, found during this increment's own investigation.

## Part 1 — Table↔TransformNode linking

### Design

New `ekos-semantic::data_lineage::link_transform_nodes_to_tables(graph: &mut KirGraph)`, run from
`commit.rs` the same way `commit_rollups` (RFC 0044) already runs a whole-graph post-processing
pass right before the final ledger write — the first point in the pipeline where every
`TransformNode` and every `Table`/`Dataset` coexist in one read.

For every `Custom("TransformNode")` Source/Sink node, looks up its
`properties["object_name"]` (case-insensitively — the same normalization `sql_analyzer.rs`'s own
FK-matching pass already applies internally) against every compiled `Table`/`Dataset` name, and
links **only on an unambiguous match** — exactly one table with that normalized name. Two unrelated
schemas both defining a `customers` table is real and common; guessing which one a bare,
unqualified `object_name` means would fabricate a false lineage edge, so a name with zero or 2+
matches is skipped, not guessed at (deliberately not a fuzzy/confidence-scored match — this is exact
string equality after normalization, so RFC 0060's identity-review machinery isn't needed at all).

New relationship kinds `Custom("ReadsFrom")` (Source node → Table) and `Custom("WritesTo")` (Sink
node → Table), each carrying a **deterministic id** (`reads_writes_kir_id`, `Uuid::new_v5` over
`(kind_label, from, to)`) from the start — the same pattern RFC 0072 retrofitted onto
`crate_topology_analyzer.rs`'s `DependsOn` edges after finding they duplicated across repeated
commits. This relationship shape has the identical "boolean fact per pair, no legitimate
multiplicity" property RFC 0072 proved safe to dedupe this way, so there was no reason to ship it
with `KirRelationship::new`'s default random id and wait to find the same bug a third time. New
edges cite the `TransformNode`'s own existing evidence — the same source fragment that already
established its `object_name` — rather than fabricating a new evidence record.

### Live verification

This repo's own real ledger has zero `Table`/`TransformNode` objects (confirmed in RFC 0074), and
neither committed fixture (`ecommerce.sql`, `northwind.sql`) has any view/transformation SQL to
exercise `SqlTransformAnalyzerPass` — only plain DDL. Built a small disposable fixture (2 tables + 2
views, one view selecting from each table) and ran the real pipeline (`init → build → recover →
resolve → compile → commit`) against it. Real output: `Commit complete. ... Data lineage links: 3`
— three real `ReadsFrom` edges (both views read `customers`; the join view also reads `orders`),
zero `WritesTo` (the views themselves aren't `Table` objects, so nothing to link them to — correct,
not a bug). Re-ran `commit` a second time against the unchanged ledger: `Relationships written: 0`
and no `Data lineage links:` line at all (the `if lineage_links_added > 0` guard suppresses it) —
confirmed the deterministic ids make this idempotent, not accumulating duplicates the way RFC 0072
found `DependsOn` did before its fix.

`docs-gen`'s Data Architecture view was updated to surface this: each Data Store's line now shows
real "read by N transformation(s), written by N transformation(s)" counts, and the Transformations
& Lineage note explains whether cross-referencing found anything, honestly, either way.

### A second real bug found via this change

While updating the "has any transformation been compiled" check that gates the Transformations &
Lineage section, found it was keyed on `is_feeds_into` (any `FeedsInto` edge) alone — which is
false for a real, legitimate single-node transformation (a bare `SELECT * FROM x` with no
downstream step has one `TransformNode` and zero `FeedsInto` edges, since `FeedsInto` only connects
two nodes). Fixed to check for any compiled `TransformNode` object directly, not the edge that
happens to connect *multiple* of them. Caught by this increment's own new test fixture (a lone
`Source` node with a `ReadsFrom` edge and no `FeedsInto` edge at all) failing an assertion it should
have passed — exactly the kind of edge case a narrow fixture surfaces that a "normal" multi-step
pipeline fixture never would.

## Part 2 — Data Domains

### Design

`Table`/`Dataset` names already carry a real schema/database qualifier whenever the source DDL
wrote one — `sql_analyzer.rs`'s `ct.name.to_string()` renders exactly what `CREATE TABLE` declared,
schema-qualified or not, and `sql_transform_analyzer.rs` uses the identical `ObjectName::to_string()`
convention. `data_domains_section` groups compiled stores by the portion of their name before the
last `.` (`sales.orders` → domain `sales`), with a real, correct empty-state: unqualified names
(both this repo's own committed fixtures — `ecommerce.sql`, `northwind.sql` — use bare, unqualified
table names) are counted and reported explicitly, not silently dropped or grouped into a fabricated
"default" domain. No new extraction — this reuses structure the compiled name already carries,
matching the whole session's "reuse before extending" discipline.

## Part 3 — Ownership and Lifecycle: a correction, and the real blocker

### RFC 0074's Ownership text was factually wrong

RFC 0074 stated `OwnedBy` edges are "compiled from git history (`git_analyzer.rs`) onto observed
`File` objects." Re-reading `git_analyzer.rs` directly (rather than trusting the earlier summary)
while investigating whether Ownership was closable this increment found this is **not** what the
code does: `git_analyzer.rs` never emits `ObjectKind::File` objects at all, and its one `OwnedBy`
relationship connects a **commit event** (`subject_id`, derived from the commit SHA) to the
**contributor** who authored it — never a file, never a table. The comment directly above the
relationship construction even says "Authorship relationship: contributor → commit event." Two
different, real primitives (`OwnedBy` edges exist; `File` objects exist, from a different connector
entirely) got conflated into one sentence that described something that isn't actually built.
Corrected in the rendered Data Architecture text and in TODO.md — this RFC's own text names the
mistake explicitly rather than quietly fixing it, since a wrong claim about what's compiled is
exactly the kind of thing this whole project's evidence-traceability discipline exists to prevent.

### The real, corrected blocker

Ownership and Lifecycle for data objects both need the same missing link: no relationship connects
a compiled `Table`/`Dataset` to the `File` it was defined in, or to git history at all. That alone
isn't fully sufficient for Ownership either — even with a `Table`→`File` link, `git_analyzer.rs`
would still need a **new**, real per-file ownership derivation (e.g. top contributor by commit
count touching that file — a real, buildable extension of the existing per-file `CoupledWith`
coupling analysis in the same pass, not a redesign), since today's only `OwnedBy` edge is
commit-event-level, not file-level. Two concrete, scoped, real pieces of follow-on work, not one
vague gap — recorded as such in TODO.md rather than left as an unspecified "needs more work."

### Data Quality — confirmed correctly out of reach, not under-investigated

Checked for any hidden signal (DDL-level `NOT NULL`/constraint metadata) that could stand in for a
real data-quality measurement. Deliberately not used: a `NOT NULL` constraint is a structural rule
about the *schema*, not a measurement of the *actual data* (completeness, freshness, validation
pass/fail against real rows) — using it would conflate RFC 0068's own explicit distinction between
"quality requirement" (a stated rule) and "observation" (a measured fact), the same distinction §26
itself draws. Genuinely needs runtime data profiling — explicitly RFC 0068 §63 Phase 3 scope
(runtime telemetry), confirmed out of reach for this increment, not under-investigated.

## Testing

- `ekos-semantic::data_lineage`: 8 unit tests — unambiguous Source/Sink linking, case-insensitive
  matching, ambiguous-name non-linking, no-match non-linking, non-Source/Sink nodes (Filter/Join)
  skipped, determinism across two independent runs, empty-table-set no-op.
- `docs-gen`: read/write counts per data store; the lineage note's two honest branches (linked vs.
  compiled-but-unlinked); `data_domains_section`'s grouping, honest empty-state, and no-stores
  case; the `has_transform_nodes` fix's regression coverage (lone Source node, no `FeedsInto`).
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end**: disposable 2-table/2-view fixture through the real pipeline, three
  times (init once, commit twice) — real `ReadsFrom` edges appear, are stable across a re-commit
  (`Relationships written: 0` the second time), confirmed via `ekos ekl`.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0075-data-architecture-cross-referencing.md` | This RFC |
| `ekos/docs/rfcs/0068-architecture-documentation-standard.md` | Status note for this increment |
| `ekos/crates/semantic/src/data_lineage.rs` | New module: `link_transform_nodes_to_tables`, `reads_writes_kir_id`; 8 tests |
| `ekos/crates/semantic/src/lib.rs` | `pub mod data_lineage;` |
| `ekos/crates/cli/src/commands/commit.rs` | `commit_data_lineage`, wired into `run()` after rollups |
| `ekos/crates/docs-gen/src/lib.rs` | Data Stores read/write counts; Transformations & Lineage honest note (+ `has_transform_nodes` bug fix); `data_domains_section`; corrected Ownership text; Lifecycle/Data Quality text sharpened; `is_reads_from`/`is_writes_to`; 9 new tests |
| `TODO.md` | Table↔TransformNode item marked done; Data Domains marked done; Ownership/Lifecycle re-scoped to the real, corrected blocker; Data Quality confirmed Phase-3-blocked |
| `devlogs/devlog_78.md` | This increment's devlog |

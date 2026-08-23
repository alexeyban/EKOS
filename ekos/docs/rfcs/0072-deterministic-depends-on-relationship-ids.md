# RFC 0072 — Deterministic `DependsOn` Relationship Ids

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-22

---

## Motivation

RFC 0070 and RFC 0071 (Increments 2 and 3 of RFC 0068's build-out) each independently found the
same real bug live: `KirRelationship`'s non-deterministic ids let logically-identical relationships
accumulate as real duplicates across repeated commits, inflating "used by"/dependent counts 3-4×
in this repo's own real ledger. Both increments fixed it locally, at render time, in the one view
each touched — and both times flagged the root cause as real, separate, ledger-layer work. Having
hit the same bug twice independently, TODO.md's own tracking promoted it ahead of further RFC 0068
feature work. This RFC fixes the actual, concretely observed instance at its source, rather than
adding a third render-time workaround.

## Investigation before design

`crates/ledger/src/lib.rs::append_relationship` and `crates/ledger/src/fact_ledger.rs`'s
equivalent both key their `(id, content_signature)` versioning / current-state pointer entirely on
`rel.id` — the exact mechanism that already correctly deduplicates identical `KirObject` re-writes
(RFC 0015), just never engaged for relationships because `KirRelationship::new()` always mints a
fresh random `KirId`, and no relationship-emitting call site in the codebase overrides it with a
deterministic one (unlike `Crate`/`Technology`/`Claim`/`ArchitectureGap`, which already do).

**Considered and rejected: a blanket fix.** `grep -rn "KirRelationship::new("` found 136 call
sites across 32 files — auditing or globally changing all of them in one increment would be a real,
unscoped, risky expansion. More importantly, a blanket "deduplicate by `(from, to, kind)`" default
would be **actively wrong** for at least one real, already-shipped case: `sql_analyzer.rs`'s
`add_fk_relationship` is called once per foreign-key column pair, so a table with two FK columns to
the same target table produces two real, distinct `ForeignKey` relationships sharing the identical
`(from, to, kind)` tuple, distinguished only by `properties["fk_desc"]` (the column names). A
blanket `(from, to, kind)`-based id would silently collapse these into one, losing a real fact —
found by reading `sql_analyzer.rs` directly before assuming a general fix was safe, not by
inspection alone.

## Design

Scoped narrowly to the one relationship shape that both actually caused the observed bug and is
provably safe to dedupe this way: `crate_topology_analyzer.rs`'s `DependsOn` edges (Crate→Crate,
Crate→Technology). A crate depending on another crate or an external technology is a boolean fact
per `(dependent, dependency)` pair — there is no legitimate scenario for two distinct real
`DependsOn` edges between the same two objects, unlike `ForeignKey`'s column-level distinction.

`depends_on_kir_id(from: KirId, to: KirId) -> KirId` — a deterministic UUIDv5 over `(from, to)`,
matching the existing `role_claim_kir_id`/`architecture_gap_kir_id` pattern already in the same
file. Set explicitly via `rel.id = depends_on_kir_id(...)` at both of the file's two `DependsOn`
construction sites (internal crate-to-crate, and external crate-to-technology).

## What this does and doesn't fix

**Fixes, for real, at the source**: every future `recover`/`commit` of this repo's (or any real
workspace's) crate dependency graph now correctly versions/replaces the same logical edge instead
of accumulating a new row — confirmed by live end-to-end verification (below), not just a
unit-level id comparison.

**Does not retroactively clean up existing duplicate rows** already committed to this repo's own
real ledger before this fix — the ledger is genuinely append-only with no delete/tombstone
mechanism anywhere in the codebase (confirmed multiple times this session). RFC 0070/0071's
render-time dedup stays in place and keeps handling the historical duplicates gracefully regardless
of how many exist underneath; this fix stops the count from growing further, it doesn't shrink it.

**Does not fix the other 134 `KirRelationship::new()` call sites** — each relationship kind needs
its own real judgment call about what distinguishes two instances (as `ForeignKey`'s column
distinction shows), not a mechanical global change. Left as explicitly scoped-out, tracked work in
TODO.md, now with a concrete worked example (`ForeignKey`) of why the general case is harder than
it looks.

## Testing

- `crate_topology_analyzer.rs`: two fully independent pass runs (separate `PassContext`s,
  simulating two separate `recover` invocations, not two artifacts from one run) over the same
  manifests must produce identical `DependsOn` relationship ids.
- Full workspace gate: `cargo build/test/clippy/fmt` clean from `ekos/`, plus `cd
  tests/integration && cargo test`.
- **Live, real end-to-end verification, both storage backends**: a small disposable two-crate
  workspace, real `build → recover → resolve → compile → commit` three times in a row (each a
  genuinely independent invocation, cache cleared between runs) against the real, default v3
  `FactLedger` backend (confirmed via the real `Ledger: .../facts` path in the command output, not
  assumed). `ekos ekl "FIND Relationship WHERE kind CONTAINS 'DependsOn'"` returned exactly the
  same 2 real relationship ids after all three runs — not 2, then 4, then 6, which is what the
  pre-fix behavior would have produced.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0072-deterministic-depends-on-relationship-ids.md` | This RFC |
| `ekos/crates/recovery/src/crate_topology_analyzer.rs` | `depends_on_kir_id`; wired into both `DependsOn` construction sites; 1 new regression test |
| `TODO.md` | Relationship-id item updated: narrow real instance fixed at source; broader scope and the `ForeignKey` counter-example recorded |

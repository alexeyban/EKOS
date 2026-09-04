# Devlog 161 — RFC 0135 Part C: `KirRelationship` determinism sweep

**Date:** 2026-09-04
**Branch:** `rfc/0135-part-c-relationship-determinism` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0135-core-provenance-and-determinism-foundations.md` (Part C of 4 — RFC now fully implemented)

---

## Summary

`KirRelationship::new` assigns a random `KirId`. The ledger has no dedup on `(from, to, kind)`,
so a logically-identical relationship re-emitted on a later `recover`/`commit` piles up as a
duplicate row — forever, no tombstone (RFC 0070/0072). RFC 0072 fixed the one observed case
(`crate_topology_analyzer`'s `DependsOn`) and its own note said the other ~134 grep hits "each
need the same kind of case-by-case investigation."

Surveying them: the ~134 collapses to **~24 real producer call sites** (persisted via
`append_relationship` on a `recover`/`compile`/`commit` path), and once surveyed they were **all
the same shape** — one edge of a given kind per ordered pair of already-deterministic endpoints.
So Part C is one sweep, not analyzer-by-analyzer:

- **`KirRelationship::deterministic(kind, from, to, discriminator)`** added to `ekos-kir` —
  `id = uuid_v5("rel:{kind}:{from}:{to}:{discriminator}")`. `discriminator` is `""` for the
  common case; a real key only where more than one edge legitimately exists (the standing
  counter-example is `sql_analyzer`'s two FKs between the same tables via different columns —
  already handled with `fk_desc`, left untouched).
- **~24 bare `::new` sites** across `recovery/` + `semantic/` + `cli/commands/identity.rs`
  converted, all `discriminator = ""`. Full table in the RFC appendix.
- **~7 sites already assigning `rel.id = <helper>`** (RFC 0072/0076/0092: `crate_topology`,
  `sql_analyzer` FK, `python_analyzer` FK/Extends, `dbt_analyzer`, `data_lineage`,
  `package_json`) are **left as-is** — converting them would change their ids and rewrite every
  existing ledger for zero benefit.
- **Guard:** `no_bare_relationship_new_in_production_code` in both `ekos-recovery` and
  `ekos-semantic` — strips `#[cfg(test)]` modules (brace-matched, since `transform_ir.rs` puts
  its test module *first*), then fails if any `KirRelationship::new(` isn't followed within 600
  chars by `.id =`.
- **~175 render/query/simulation `::new` sites** (73 in `docs-gen` alone) confirmed out of scope
  — throwaway objects, never `append_relationship`'d — and untouched.

---

## Knowledge Captured

- **"134 of 136" was a grep count, not an exposure count.** The producer set — relationships
  that actually reach `append_relationship` on a compile path — is ~24 + ~7 already-fixed. The
  rest build throwaway `KirRelationship` values for a Mermaid diagram or a graph-export payload
  and a random id there is completely harmless.
- **They were all one shape.** Every analyzer already mints deterministic endpoint ids
  (`file_kir_id`, `module_kir_id`, `symbol_kir_id`, …), so for every swept site
  `(kind, from, to)` *is* the identity — there was no per-site judgement call to make after all,
  except confirming no legitimate second edge exists (only FK-via-different-columns does, and
  that was already handled).
- **The guard must brace-match, not split on `"mod tests"`.** `transform_ir.rs` has its
  `#[cfg(test)] mod tests` at line 28 and production `lower_to_kir` at 619 — a naive
  "everything after `mod tests` is a test" (which Part D's Python inventory used) would have
  missed a real production `FeedsInto` edge.
- **`cli/commands/identity.rs` was a live exposure** — `ekos identity scan` writes `SameAs`
  candidates with `KirRelationship::new`, so a re-scan accumulated duplicate candidate rows.
  Now `::deterministic` — one candidate per `(a, b)` pair.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/lib.rs` | `KirRelationship::deterministic(kind, from, to, discriminator)` + 1 test |
| `ekos/crates/recovery/src/{dependency,crypto,confluence,document_semantics,git,github,elixir,rust,javascript,python,local_docs}_analyzer.rs` | `::new` → `::deterministic(_, _, _, "")` at each producer site |
| `ekos/crates/recovery/src/lib.rs` | `relationship_determinism_guard` test module |
| `ekos/crates/semantic/src/{lib,rollup,transform_ir}.rs` | `::new` → `::deterministic` at each producer site + guard test module |
| `ekos/crates/cli/src/commands/identity.rs` | `SameAs` candidate → `::deterministic` |
| `ekos/docs/rfcs/0135-…md` | Part C marked implemented; per-call-site appendix; RFC status → Accepted |
| `TODO.md` | Part C ticked; RFC 0135 done |

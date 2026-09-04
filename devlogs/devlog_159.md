# Devlog 159 — RFC 0135 Part D: identity kind-exclusion registry + CI guard

**Date:** 2026-09-04
**Branch:** `rfc/0135-part-d-identity-exclusion-registry` → `main` (local merge, `[skip ci]`)
**RFC:** `ekos/docs/rfcs/0135-core-provenance-and-determinism-foundations.md` (Part D of 4)

---

## Summary

`DefaultResolver`'s "which `Custom(_)` kinds are self-identified by a structural key and must
never be merge candidates" list was a literal `matches!(k == "Section" || k == "TransformNode" ||
…)` with a ~100-line comment recording that it had been rediscovered **live, roughly a dozen
times** — each time a new analyzer shipped without touching it, an over-merge collapsed a whole
book / crate graph / module tree into one object, found weeks later by reading a real generated
entity page.

Part D replaces the literal with `ekos_kir::custom_kinds::REGISTRY` — one row per `Custom` kind
the compiler pipeline emits, `structurally_keyed: bool` — and a test that walks
`crates/{recovery,semantic}/src` and **fails CI if any `ObjectKind::Custom("…")` an analyzer
emits is missing a row**. The guard the list always needed.

Enumerating for the registry turned up **4 kinds that were structurally keyed but never
excluded** — `Page` (Confluence, `(space, page id)`), `Risk` (RFC 0094, one per source object),
`Rollup` (RFC 0044, one per directory), `ProjectSummary` (RFC 0088, one per project). All four
had the exact Section/Crate failure shape (shared name prefix + `structural_score`'s same-kind
1.0 fallback) and are now excluded — 4 latent over-merges fixed.

---

## PR — Part D

| File | Change |
|---|---|
| `ekos/crates/kir/src/custom_kinds.rs` | **New.** `CustomKind { name, structurally_keyed, note }`, `REGISTRY` (22 rows: 18 keyed, 4 mergeable — `Concept`/`Technology`/`Issue`/`PullRequest`), `lookup` / `is_structurally_keyed`, 3 unit tests |
| `ekos/crates/kir/src/lib.rs` | `pub mod custom_kinds;` |
| `ekos/crates/identity/src/lib.rs` | the ~100-line comment + literal `matches!` → `is_structurally_keyed(k)`; condensed history comment; 2 residual-pair test cases re-keyed off `Page`→`Pipeline`; `other_custom_kinds_still_resolve_normally` → `a_non_keyed_custom_kind_still_resolves_normally` (uses `Issue`); **new** `every_pipeline_custom_kind_is_registered` |
| `ekos/crates/identity/Cargo.toml` | `walkdir` dev-dependency (the coverage-test source walk) |
| `CLAUDE.md` | `identity` crate-map entry rewritten around the registry + the CI guard |

### The coverage test

```
walk crates/{recovery,semantic}/src/*.rs
  → for each file, take the slice before the first "mod tests"
  → split on `ObjectKind::Custom(` and pull the "…" string literal
  → assert custom_kinds::lookup(name).is_some() for every one
```

Crude `mod tests` split rather than a real parser — good enough (a `Custom` string that only
appears in a test module is out of scope by definition). Passed on first run, which is the
evidence the 22-row registry is complete against today's source.

---

## Knowledge Captured

- **The registry lives in `ekos-kir`, not `ekos-identity`.** `kir` owns `ObjectKind` and is a
  dependency of both the analyzers (`recovery`) and the resolver (`identity`) — the only place a
  single source of truth can sit without a dependency cycle.
- **`Page`, `Risk`, `Rollup`, `ProjectSummary` were latent over-merges nobody had hit yet** —
  `Page` because no one had resolved a multi-page Confluence space; `Risk`/`Rollup` because in a
  single-project workspace there is usually ≤1 per directory/object so no pair to merge;
  `ProjectSummary` because it's genuinely one-per-project until multi-project. The registry
  forced the audit that found them.
- **`Technology` is `structurally_keyed: false` but for a different reason than the other
  mergeables** — its id is `Uuid::new_v5(name)`, so two `Technology("react")` objects from
  different analyzers are literally the *same object* (deduped at `append`), never a merge. It
  never reaches `DefaultResolver`. Documented in its registry `note` so a future reader doesn't
  "helpfully" add it to the keyed set.
- **Two existing identity tests used `Custom("Page")` as a stand-in for "a kind that reaches
  comparison"** — a reasonable choice when Page wasn't excluded, now wrong. Re-pointed at
  `Custom("Pipeline")` (only ever a test-ism, never a real pipeline `Custom` kind — the built-in
  `ObjectKind::Pipeline` is the real one).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/custom_kinds.rs` | New — the registry |
| `ekos/crates/kir/src/lib.rs` | module decl |
| `ekos/crates/identity/src/lib.rs` | resolver derives exclusion from the registry; test updates + new coverage test |
| `ekos/crates/identity/Cargo.toml` | `walkdir` dev-dep |
| `CLAUDE.md` | `identity` crate-map entry |
| `ekos/docs/rfcs/0135-…md` | Part D marked implemented |
| `TODO.md` | Part D ticked |

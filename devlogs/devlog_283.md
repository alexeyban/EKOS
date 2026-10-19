# Devlog 283 — External review: redaction gaps, deterministic evidence, a read-only Runtime by type

**Date:** 2026-10-19
**PRs:** none (branch `fix/emberian-review`, merged locally to `main`)
**Branch:** fix/emberian-review → main — 8fee43a, 87eab36, 61bac92, 123c839, 9b1a272, this commit
**Design:** RFC 0043 amendment, RFC 0135 Part E (E1–E4), RFC 0056 amendment, RFC 0007 amendment,
RFC 0149 amendment

---

## Summary

An outside reviewer (emberian), modelling EKOS for a formally verified version ("VerEKOS"),
filed eleven issues (#10–#20) against `8e3c178`. Every line reference was checked against the
code and held. Three issues were secrets leaking past RFC 0043. One was ledger churn: random
evidence ids made every re-run of a pass a new version of every object it touched. The rest were
invariants kept only by convention: evidence on compiled edges, hypotheses walked as facts, an
MCP tool that writes, a read-only Runtime holding a writable store, and review guards that an
alias gets around. All eleven are addressed; on #16 the maintainer decided that confirming a treasury
`AuthorizedBy` is human-only, so it moved from the MCP tool to `ekos treasury confirm|reject`.

---

## Redaction (#18, #19, #20) — 8fee43a

### Problem / motivation
Readers added after RFC 0043 re-read workspace files outside its two entry points.
`read_crate_doc_comment` sent raw `//!` comments to the LLM (neither exclusion nor redaction).
`llm_description` redacted but read excluded files. `GitBlame` stored commit subjects verbatim in
the append-only ledger.

### What was built
| Component | Change |
|---|---|
| `ekos_common::redaction::read_redacted` | refuses an excluded path, then returns the whole file redacted |
| `llm_description::read_symbol_source`, `read_crate_doc_comment`, `migrate_logic::definition` | all go through it |
| `GitBlame` | holds the `RedactionConfig`; skips excluded paths; redacts each summary |

### Implementation details worth remembering
The whole file is redacted **before** slicing. The spans and line numbers recorded at `recover`
index text that had already been redacted, so slicing raw text and redacting the slice could put
`migrate_logic`'s byte spans in the wrong place.

---

## Determinism and evidence (#10, #12, #17) — 87eab36

### What was built
| Component | Change |
|---|---|
| `KirEvidence::new` | id = v5 of `(path, line, column, confidence bits, fragment)`; `with_confidence` re-derives unless the id was caller-assigned |
| `FeedsInto` / `Disputes` / risk `References` / business-semantics links | cite evidence (`cite_endpoint_evidence` for the semantics layer) |
| `Rollup` | cites ≤5 sorted membership facts; membership evidence located at the member file |
| `KirObject::is_unconfirmed_claim` + `UNCONFIRMED_OBJECT_STATUSES` | `trace_impact`, `load_neighborhood_bounded`, `ekos_dependents` skip such endpoints |

### Decisions
- **`new` itself became deterministic** instead of adding `KirEvidence::deterministic` plus a
  guard test (the issue's proposal). That fixes all ~90 call sites at once, and there is no
  random constructor left to forget. Across the whole workspace suite the only failure was an
  unrelated flaky test, which passed 3/3 on rerun.
- **#17 filters by the endpoint's status, not the edge's.** Stamping `status` on edges would
  need re-deriving after every human confirmation. The endpoint check also covers
  `MigrationProposal` and needs no edge rewrite. `open`/`resolved` conflicts stay walkable:
  "the sources disagree" is an observed fact.

---

## MCP writers and review evidence (#11, #16) — 61bac92

- ClickHouse audit event id = v5 of `(database, sql, result hash)`; documented as the one
  non-review MCP write.
- `only_listed_functions_can_write_the_ledger`: the `mcp.rs` functions that open a writable
  store, call `append_*` or run a live query are an explicit list. It caught the new
  `record_review_decision` on its first run.
- Both review tools take optional `reviewer`/`note`; the decision event cites an evidence record
  naming them (note redacted) and has a deterministic id.
- **`AuthorizedBy` is human-only** (maintainer's decision): `ekos treasury confirm|reject`
  (Review class; reject needs `--note`) → `treasury::review_authorization`, guarded by
  `review_guard.rs`. `ekos_identity_review` now refuses an `AuthorizedBy` and names the command.
  RFC 0032 amendment.

---

## RFC 0007 amendment (#15) — 123c839
Restates candidates (structurally keyed kinds excluded), structural score (column/row Jaccard,
1.0 fallback), thresholds (0.90 / 0.95 Concept) and exact-only merges (fuzzy → `SameAs` review).

---

## Read-only by type, review guard (#13, #14) — 9b1a272

### What was built
| Component | Change |
|---|---|
| `ekos_ledger::KnowledgeReader` | every read; `KnowledgeStore: KnowledgeReader` adds `append_*`, `set_write_context`, `vacuum_into` |
| `Runtime`, `export_graph` | take `&dyn KnowledgeReader` |
| `every_store_method_has_a_decision_here` | reads both traits; each wrapper body must consult the scope or be a listed passthrough |
| `commands/review_guard.rs` | parses `mcp.rs` with syn, resolves `use` renames/groups/`self`/globs and macro-argument paths; checks every entry point still exists |

### Implementation details worth remembering
- The trait and impl blocks were split by a script that relies on rustfmt layout (methods at
  indent+4, closing `}` on its own line). Use the same approach for any further split.

---

## Knowledge Captured

- **Supertrait methods are callable on `dyn Sub` without importing the supertrait**, and
  `&dyn Sub` → `&dyn Super` coerces (trait upcasting, stable since 1.86). So splitting a trait
  into `Reader` + `Store: Reader` leaves `&dyn Store` users compiling. What breaks: read calls on
  a **concrete type** with only `Store` imported, and `Store::read_method(&x)` written in full.
  Here that was 4 files plus 2 written-out calls; both extension repos compiled unchanged.
- **Checking an extension repo against a local EKOS:** the commented `[patch]` block in their
  Cargo.toml has no row for the `cli` package (named `ekos`). Without one, cargo mixes the old git
  `cli` with the new libraries, and the errors appear inside EKOS's own code. Add
  `ekos = { path = ".../crates/cli" }`, use a scratch `CARGO_TARGET_DIR`, and restore their
  `Cargo.lock` afterwards.
- **A literal-text guard catches omissions, not aliases.** `use x::{resolve as settle}` and
  `use x as c; c::resolve()` get past `contains("x::resolve")`. Resolve the imports, or use a
  capability type. Also pin that the guarded name still exists; otherwise a rename makes the
  guard pass forever.
- **Making `KirEvidence::new` deterministic means the first commit after the upgrade writes one
  new version of each object the passes touch.** After that, unchanged sources write nothing.
- `rebuilding_after_a_ledger_clear_reproduces_file_objects_despite_a_fingerprint_hit` can fail
  with `DirectoryNotEmpty` under parallel load. This was not caused by these changes.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/common/src/redaction.rs` | `read_redacted`, `SourceReadError` |
| `ekos/crates/recovery/src/{llm_description,architecture_reasoning,sql_analyzer}.rs` | re-readers via `read_redacted`; reparse determinism test |
| `ekos/crates/cli/src/commands/{semantics,architecture,migrate_logic}.rs` | blame redaction; call-site config |
| `ekos/crates/kir/src/lib.rs` | content-derived evidence ids; `is_unconfirmed_claim` |
| `ekos/crates/semantic/src/{transform_ir,conflicts,lib,rollup,business_semantics}.rs` | evidence on edges and rollups |
| `ekos/crates/runtime/src/{lib,graph_export}.rs` | hypothesis-aware walks; `KnowledgeReader` |
| `ekos/crates/clickhouse-query/src/audit.rs` | deterministic audit event id |
| `ekos/crates/cli/src/commands/mcp.rs` | dependents status, review evidence, writer allowlist test |
| `ekos/crates/ledger/src/{lib,scope,retrieval,partitioned/knowledge_store}.rs`, `distributed/src/gateway.rs` | trait split |
| `ekos/crates/cli/src/commands/review_guard.rs` (new), `extension.rs`, `Cargo.toml` | import-resolving guard; extension obligation |
| RFCs 0007, 0043, 0056, 0135, 0149 | amendments |
| `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | docs |

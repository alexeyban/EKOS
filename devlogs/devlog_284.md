# Devlog 284 — Evidence checked at the commit boundary; parsed writer guard

**Date:** 2026-10-19
**PRs:** none (local `main`, pushed)
**Branch:** EKOS main — this commit
**Design:** RFC 0135 E5, RFC 0056 amendment (clarified)

---

## Summary

A follow-up review of devlog_283 suggested three things: a global "every committed item has
evidence" test, a parsed (not grepped) MCP writer guard, and a precise statement of what the
ClickHouse audit's deterministic id means for repeated queries. The global test found five more
gaps on its first run, and all are fixed.

---

## Global evidence test

### What was built
| Component | Change |
|---|---|
| `crates/cli/tests/evidence_everywhere.rs` | full pipeline over a mixed fixture; fails on any object/edge kind with empty evidence unless in `EXEMPT` (empty) |
| `recovery/src/rust_analyzer.rs` (v6) | `Contains` shares the symbol's evidence; `Calls` cites `call_site_line` in the caller's file; `DependsOn` cites the `use` item (new `edge_spans`) |
| `recovery/src/view_analyzer.rs` (`view-analyzer/5`) | `File → View` `Contains` cites the definition |
| `recovery/src/crate_topology_analyzer.rs` | `Crate` cites its `Cargo.toml` |
| `recovery/src/source_evidence.rs::cite_incoming` | an object that exists only because something references it (`PythonModule`, `RustModule`, `Technology`) cites ≤3 incoming evidence ids, sorted |

### Decisions
- **`cite_incoming` sorts by id and caps at three.** Content-derived evidence ids (devlog_283) make
  "smallest three" independent of file order, so the object's signature is stable across runs.
  Citing every reference would make a popular module's object grow with the codebase.
- The test prints the compiled kinds, so a fixture that silently stops producing a kind shows up.
  It found 24 kinds on this fixture.

## Parsed writer guard
`review_guard::functions_reaching(source, predicate)` parses the file, drops `#[cfg(test)]` items,
resolves imports, and returns the functions whose bodies contain a matching path. A method call
`x.append_event(..)` counts as the one-segment path `append_event`. `mcp.rs`'s
`only_listed_functions_can_write_the_ledger` uses it, and `writer_scan_resolves_aliases` covers an
aliased `open_store`, a method call, a macro argument, an impl method, and comments/strings that
must not match.

## ClickHouse audit identity
Checked on both backends (`a_repeated_query_is_a_new_version_not_a_lost_row`): a repeat is a new
version under the same event id, not a lost row. `get_event` shows the latest run, and the
individual runs appear in raw entry readers (`ekos_diff`, `ekos ledger audit`). RFC 0056 now says
exactly that, and that the choice is intentional.

---

## Knowledge Captured
- A per-producer test only covers producers someone remembered. One pipeline-level test over a
  varied fixture found five gaps that RFC 0135 E2's per-producer tests missed. Prefer checking
  at the boundary where data is written.
- `syn` + `proc_macro2` token walking turns `x.method(..)` into the path `method` once paths
  are split at `.`. That is enough to see method-call writes without type information.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/cli/tests/evidence_everywhere.rs` | new pipeline-level evidence test |
| `ekos/crates/recovery/src/{rust_analyzer,view_analyzer,crate_topology_analyzer,python_analyzer,dependency_analyzer,source_evidence}.rs` | evidence for the five gaps; pass versions |
| `ekos/crates/cli/src/commands/{review_guard,mcp}.rs` | `functions_reaching`; parsed writer guard |
| `ekos/crates/clickhouse-query/src/audit.rs` | repeat-is-a-version test |
| RFC 0056, RFC 0135, `CHANGELOG.md`, `TODO.md`, `CLAUDE.md` | docs |

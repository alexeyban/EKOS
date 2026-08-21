# Devlog 65 — Closing seven small, real gaps found across the whole devlog history

**Date:** 2026-08-20
**PRs:** (uncommitted at time of writing — see Files Changed)
**Branch:** main (direct)

---

## Summary

Reread all 64 prior devlogs in full (via a background research pass) to compile a complete,
deduplicated list of every stated-but-unfixed gap across the project's history — not just the most
recent ones. Sorted the results into small/well-scoped code fixes, large/systemic design work
needing its own RFC, and work blocked on external credentials/access this environment doesn't
have. Fixed seven of the eight small/well-scoped items; the eighth turned out, on closer
inspection, to be substantially bigger than its one-line description suggested, and got
re-scoped down to "reported, not fixed" instead of being forced through.

---

## Fix 1 — `OllamaProvider::from_env()` ignored `[llm].model`

`ekos.toml`'s `[llm] model = "..."` was read for Anthropic/OpenAI but silently ignored for the
Ollama provider path — `build_llm_provider` always called `OllamaProvider::from_env()`, which only
ever consulted the `OLLAMA_MODEL` env var. Added `OllamaProvider::from_env_with_model(Option<&str>)`
(config override → env var → built-in default, in that priority order); `from_env()` now delegates
to it with `None`. `build_llm_provider` passes `config.llm.model.as_deref()`.

---

## Fix 2 — `extract_citations` couldn't distinguish "cited nothing" from "nothing to cite"

Found live-testing RFC 0046 against real `gpt-4o-mini` responses (devlog_46): a trailing
`{"cited_evidence": [...]}` block that parses as valid JSON but is empty produced the exact same
empty-diagnostics shape as a genuinely well-cited answer. Added a second diagnostic code, `AI002`,
distinct from the existing `AI001` (missing/malformed block): emitted whenever the block parses
cleanly but zero citations survive filtering — either an empty array, or every id unknown/
malformed. Two existing tests (`ask_sends_object_context_in_prompt`, `ask_drops_unknown_cited_ids`)
encoded the old (buggy) expectation of empty diagnostics for exactly this case; updated both to
assert the new `AI002` diagnostic instead.

---

## Fix 3 — `gather_context` had no size/token budget

Found live-testing RFC 0046 (devlog_46): broad/hub search terms against EKOS-self's ~7,500-object
ledger produced real `context_length_exceeded`/`rate_limit_exceeded` provider errors — one request
alone asked for 209,852 tokens against a 200,000 TPM limit. `max_matches`/`neighborhood_depth`
bound seed count and hop *depth*, never what a single hop actually pulls in. Added
`AiRuntimeConfig::max_context_chars` (default 200,000, wired through `[ai].max-context-chars` in
`ekos.toml`, same pattern as the other `[ai]` tunables) — `gather_context` now stops admitting
`ObjectState`s once their cumulative serialized size crosses the budget, always admitting at least
the first object so a single oversized object can never make `ask` answer from zero context.
Truncation surfaces as a new `AI003` diagnostic, merged into `AiAnswer.diagnostics` alongside any
citation diagnostic.

---

## Fix 4 — `ekos ask`'s ranking picked the wrong same-basename file

Real bug (devlog_60): asking `"README.md"` against a real, cold-compiled `analytics/` ledger
answered from an 83-byte `test/priv/README.md` GeoLite2 test fixture instead of the real project
`README.md`, which ranked 13th of 25 matches. Root cause: `Ledger::find_objects`'s bm25 ranking
blends a `content` excerpt column whose raw size varies by orders of magnitude across objects —
the real README's much larger excerpt penalized its bm25 score relative to the tiny fixture's,
despite the fixture only matching on a nested path, not an exact name. Added
`promote_exact_name_matches`: a stable partition (not a re-sort) that moves any object whose name
is an exact case-insensitive match for the query ahead of every other already-ranked result.
Regression test reproduces the real shape (a large real-README excerpt vs. a tiny fixture
excerpt, both named `README.md` at different path depths).

---

## Fix 5 — GitHub connector missed full-URL issue/PR references

RFC 0062 fixed bare `#N` mentions but not full URLs. Real example: `plausible/analytics` PR
#6597's actual body is `"Extracted from https://github.com/plausible/analytics/pull/6591"` — no
bare `#N` anywhere, invisible to `find_bare_issue_numbers`. Added
`find_full_url_issue_numbers(body, owner, repo)`, scoped to the same owner/repo this pass is
already processing (a cross-repo URL would need a different KIR item namespace entirely — out of
scope). Merged into the existing bare-mention loop with dedup, so a body mentioning both `#1` and
its full-URL form doesn't double-emit an edge.

---

## Fix 6 — PDF-derived `Table` objects over-merged within one document

The same over-merge root cause RFC 0024/0060 already fixed for `Section`/`Table` schema-qualifiers
had one more instance, previously only documented as an `#[ignore]`d failing test (devlog_60):
PDF/DOCX-extracted tables (`local_docs_analyzer.rs`, named `"{path}: table {n}"`) have no
`columns` property, so `structural_score` fell back to its blanket `1.0` "no structural signal"
floor — live repro was 9 distinct tables from one real PDF collapsing into one canonical object at
confidence 0.99. The real fix, not a workaround: these tables *do* carry real structural
content — `local_docs_analyzer.rs` already stores each table's actual extracted cell text under
`properties["rows"]`, just never consulted by identity resolution. Added
`similarity::row_cell_tokens` (a lowercased, deduplicated cell-text set) and a second
`structural_score` branch that Jaccard-compares it, tried only after the existing `columns`
check and before the `1.0` fallback — so SQL-derived tables (which do have `columns`) are
completely unaffected. Un-ignored the original test (rewritten to seed real, distinct row content
per table, since the old version used the property-less `make_graph` helper and could never have
exercised the fix); added a companion test proving two tables with genuinely *identical* row
content still merge, so the fix doesn't overcorrect into "every PDF table looks unique regardless
of content."

---

## Fix 7 — dbt/Transformation-diff false positives on reordered join keys

Found deriving `devlog_29`'s Phase 7 benchmark's own expected values: the same real join,
recovered by two different producers, records its key pair in opposite tuple order — Pentaho's
`MergeJoin` reads `<key><value1>/<value2>` as `("id", "customer_id")`;
`sql_transform_analyzer.rs`'s `collect_equi_keys` reads `ON customer_id = id` left-to-right as
`("customer_id", "id")`. Same columns, same join, reversed order — `ekos_transformation_diff`'s
text-level comparison would report this unchanged join as both added and removed. Added
`canonical_join_keys` in `mcp.rs` (sorts each pair, then sorts the pair list, before rendering the
diff-comparable string) — used only by `node_comparable` (the diff path); `node_summary`'s
human-readable display keeps the producer's own real key order untouched, per RFC 0028's own
"presentation/comparison fix, not a data-model change" framing. The `Calculate` node's
cross-producer text mismatch (`"total_with_tax := MULTIPLY(amount, tax_rate)"` vs.
`"total_with_tax=amount * tax_rate"`) is a different, harder problem — real symbolic-expression
equivalence, not tuple-order canonicalization — and was **not** attempted here; the devlog_29
follow-up itself only committed to "consider" it, not scope it as done.

---

## Not fixed: multi-project ID-collision extension — re-scoped after investigation

The devlog audit characterized this as small/well-scoped: RFC 0044's file-object id-collision fix
(`build.rs` hashes `"{project_key}:{rel_path}"` instead of the bare path when `[observe] paths`
has more than one entry) was never extended to `github_analyzer.rs`, `local_docs_analyzer.rs`,
`rust_analyzer.rs`/`python_analyzer.rs`, or `git_analyzer.rs`'s `CoupledWith` pairs. On actually
reading the code, this isn't a per-file tweak: `project_key` only exists as a transient local
inside `build.rs`'s own connector loop, computed once per `observe_paths` entry and used solely to
hash `File` object ids — it is never persisted onto the `ObservationArtifact`s those recovery
passes later read back from `ctx.artifact_store`. Every one of those passes runs later, in
`ekos recover`, over already-collected artifacts with no project context available at all. Fixing
this properly means plumbing a project identity through the artifact schema itself (a new field or
equivalent, threaded through *every* observer/analyzer that derives an id from a path or symbol
name) — a real cross-cutting data-model change, not a same-shape copy-paste of the `build.rs`
pattern. Re-classified as Category 2 (needs its own RFC) rather than forced through as a quick fix.
This is the same "verify before trusting a prior characterization" discipline this session applied
to the Postgres ENUM non-issue in `devlog_64` — the audit's one-line summary was a reasonable
starting hypothesis, not a substitute for reading the actual code.

---

## Also found, not part of the gap list: `analytics/`'s local ledger has a corrupted FTS index

While trying to verify Fix 4 against the real `analytics/` ledger (the exact repo/scenario
devlog_60's bug was found in), `ekos query find` failed with `database disk image is malformed` /
`Error code 267: Content in the virtual table is corrupt`. `PRAGMA integrity_check` on the
underlying SQLite file reports `ok` — the corruption is isolated to the FTS5 virtual table, not
general page-level damage. Not touched destructively (no rebuild/repair attempted) — this is real
user data in a ledger outside this git repo, and diagnosing/repairing it wasn't in scope for this
pass. Most likely cause: concurrent writes from multiple `ekos` processes against the same ledger
file with no write-barrier — this is direct, physical evidence for the "write-barrier/concurrency
spec" gap already on the roadmap as unimplemented (`TODO.md` Priority 4), not a new, separate
problem. Verification of Fix 4 instead relies on its unit test, which faithfully reproduces the
real bug shape (a large real-README excerpt vs. a tiny same-basename fixture excerpt).

---

## Knowledge Captured

- **`sort_by_key` is a stable sort in Rust's std** — used for `promote_exact_name_matches`'s
  partition; relative bm25 order is preserved within both the "exact match" and "everything else"
  groups, so the fix only ever promotes, never re-scores.
- **Rust env-var test isolation**: `cargo test` runs test functions in parallel threads by
  default, and `std::env::set_var`/`remove_var` are process-global and `unsafe` (2024 edition) —
  two `#[test]` functions independently mutating the same var will race. The existing
  `ollama.rs` test already carried a "no concurrent access" comment implicitly assuming this;
  adding a second env-mutating test broke that assumption immediately (caught live, by the test
  actually failing) — the fix was merging both scenarios into one test function, not adding
  more isolated ones.
- **A structural signal was already being collected and just never consulted.** The PDF-table
  fix didn't need any new data — `local_docs_analyzer.rs` had been writing real `rows` content to
  every table object's properties all along; `structural_score` simply never looked at it. Worth
  checking "is the signal already there, just unused" before assuming a fix needs new
  instrumentation.
- **A prior devlog's one-line characterization of a gap's size can be wrong** — the multi-project
  ID-collision item looked identical in shape to the `File`-object fix it was compared against,
  but the actual blocker (no project context surviving into recovery-pass artifacts at all) only
  shows up once you read the code, not the summary. Re-scoping mid-task, honestly, beats forcing a
  fix that doesn't actually address the root cause.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/ollama.rs` | `from_env_with_model`, wired to `[llm].model` |
| `ekos/crates/cli/src/commands/recover.rs` | `build_llm_provider` passes the config model override |
| `ekos/crates/runtime/src/ai.rs` | `AI002` diagnostic; `max_context_chars` budget + `AI003` diagnostic |
| `ekos/crates/compiler-core/src/config.rs` | `AiConfig::max_context_chars` |
| `ekos/crates/cli/src/commands/ask.rs` | wires `max_context_chars` into `AiRuntimeConfig` |
| `ekos/crates/ledger/src/lib.rs` | `promote_exact_name_matches` in `find_objects` |
| `ekos/crates/recovery/src/github_analyzer.rs` | `find_full_url_issue_numbers`, merged into the mention loop |
| `ekos/crates/identity/src/similarity.rs` | `row_cell_tokens` |
| `ekos/crates/identity/src/lib.rs` | `structural_score`'s second branch; un-ignored + rewrote the PDF-table test |
| `ekos/crates/cli/src/commands/mcp.rs` | `canonical_join_keys`, used only by `node_comparable` |
| `devlog_65.md` | This file |

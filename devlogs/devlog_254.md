# Devlog 254 — Eval follow-up: what search reads, how partial matches rank, and three grader defects

**Date:** 2026-10-11
**PRs:** none (local main, `[skip ci]`)
**Branch:** main

---

## Summary

A full eval on `deepseek-v4.1-flash` scored 88 of 101 and named three problems: retrieval, refusals and uncited answers. Every failing scenario was traced to its cause from the saved transcript. Two of the three headings were mostly grader defects. Retrieval was real, and the largest cause was not a ranking detail: a symbol's doc comment and signature had never been searchable.

The same suite on the same ledger now scores 95 of 101, and no failure is attributed to retrieval. RFC 0177.

---

## The run that started it

`deepseek-v4-flash`, the model in `ekos.toml`, is no longer served by opencode Zen: 90 of 91 calls returned `403: Model access is disabled`. The report still printed scores, and a run where every call errors could print `PASS`. `ekos.toml` now names `deepseek-v4.1-flash`.

| | Before (88/101) | After (95/101) |
|---|---|---|
| Answer correctness | 86.2% | 95.8% |
| Evidence groundedness | 96.7% | 95.6% |
| Recall@10 | 55.9% | 70.6% |
| Fabrication (of 20 refusal scenarios) | 15% | 10% |
| Answered but uncited | 11 | 2 |
| Failures attributed to retrieval | 6 | 0 |

Both runs use the self-workspace ledger from 2026-09-23. It was not re-recovered: `recover` and `commit` call the configured model, which is metered here.

---

## Retrieval

### Problem / motivation

Six scenarios failed because the fact never reached the model, and Recall@10 was 55.9%.

### What was built

| Cause | Change | Where |
|---|---|---|
| A symbol's doc comment and signature were not indexed | `description`, `signature`, `fields` join the indexed text | `kir/src/lib.rs::indexed_content` |
| An existing index never gains newly indexed text | The index records a content version; a writable open under another version wipes and replays it | `ledger/src/search.rs` (`CONTENT_VERSION`) |
| Partial matches were ordered by BM25 alone | The band is fetched 4× wider and ordered by BM25 rank fused with coverage rank | `ledger/src/search.rs::fuse_bm25_with_coverage` |
| A hit showed the opening of its text | The claim shows the window holding the most query words | `runtime/src/reason.rs::matched_window` |
| "Full source text" attached nothing | Falls back to the symbol's own evidence fragment | `runtime/src/reason.rs::source_text_item` |
| "What depends on M" answered for one import path | Dependents of every `M::…` object of the same kind are added | `runtime/src/reason.rs::module_members` |
| A file was not found by its bare name | The exact-name signal matches a file by name or stem | `ledger/src/retrieval.rs::exact_name_matches` |
| Struct fields were nowhere | Rust struct and enum symbols record `fields` | `recovery/src/rust_analyzer.rs` (pass `v5`) |

### Measured without a model

`ekos eval run --evidence-only` is new. It runs retrieval and evidence assembly, calls no model, and prints Recall@10 and how many scenarios have every expected fact in the evidence.

| Step | Recall@10 | Facts in evidence |
|---|---|---|
| Before | 55.9% | 52 / 63 |
| Matched window, source-text fallback, module dependents | 55.9% | 56 / 63 |
| File stem, recall counted per name | 61.8% | 56 / 63 |
| Fused order for partial matches | 61.8% | 60 / 63 |
| Doc comments and signatures indexed | 70.6% | 60 / 63 |

### Implementation details worth remembering

- **Coverage alone was worse than BM25 alone on one axis.** Ordering the band by coverage reached 60 of 63 facts and dropped Recall@10 back to 55.9%: a long file that mentions every word once buried the symbol the question describes. Reciprocal-rank fusion of the two orders kept both gains.
- **The fusion is a pure function** (`fuse_bm25_with_coverage(&[coverage]) -> order`). A test built on a synthetic index could not reproduce the failure: it passed with and without the change, because the effect comes from the real corpus's name-field statistics. The function is tested directly instead.
- **Coverage is counted from one lookup per term**, now over 2,000 documents instead of 200. A hit a lookup misses is counted one term short.
- **The content version is separate from the schema.** Nothing about the tantivy schema changes when `indexed_content` returns more text. An index without a recorded version is treated as version 1.
- **A read-only open never rebuilds.** It warns once and searches the older text.

### Decisions

- **The `fields` property is a list of strings** (`name: Type — doc`), like `symbols`. It is indexed and shown without a second schema.
- **A file stem shared by more than three files matches none of them.** `lib` and `mod` name no file.
- **Module members only for a seed named `M::self`.** `M::redact` is one item and its dependents are its own.

---

## The dead source-text path

RFC 0140 §3 attaches the full source of the top entities to the evidence. Over 101 scenarios it attached nothing.

The lookup takes the `source_artifact_id` of the entity's latest write and expects an observation artifact with `data.source`. A commit records `ka:<id>`: the knowledge artifact, which holds compiled KIR. The Rust analyzer writes one knowledge artifact for all files, with no input ids, so there is no path back to a file's text. The unit test seeded a matching id by hand and passed.

The symbol's own evidence fragment is its source, sliced at recover time from text that had passed redaction. The lookup now falls back to it. Text beyond the fragment's 40 lines is still unreachable; recording the observation artifact id on the symbol is an open item.

---

## Refusals and uncited answers

| Reported | Actual | Change |
|---|---|---|
| 11 answers uncited | All 11 were correct refusals | `answered_uncited` excludes refusals; refusals are counted on their own line |
| 3 fabrications | All 3 rejected the false premise, each in different words | The prompt fixes a second opening, `Premise not supported.`; the grader accepts it |
| 2 answers uncited after the fixes | One quoted code with an open brace, which hid a well-formed citation block from the brace scanner | The block is also looked for by its key |

### Decisions

- **No loose phrases in the grader.** "does not" reads like a denial and appears in invented answers too. A test keeps one such answer failing.
- **A test asserts the prompt states both openings the grader accepts.** A model graded against words it was never shown is graded on luck.

### What the new opening cost

The three false-premise scenarios now pass. Three others changed:

- `arch-015`, a real question with a convoluted premise, was answered `Premise not supported.`
- `adv-001` and `sec-003` are correct denials in the model's own words (`adv-001` found a devlog that names the fake analyzer as a test fixture and said so). They passed before with `Insufficient evidence.`

Net on the 20 refusal scenarios: 17 → 18. The prompt was not tuned further, because each try is a metered run.

---

## Harness

| Change | Why |
|---|---|
| A run with an errored scenario fails | With every scenario errored each mean is `None` and every gate held vacuously |
| Recall@10 counted per expected name | The name-to-id map kept one of two objects sharing a name; a query that ranked the file first scored 0 when the map held the document read from it |
| Question echoes left out of search | An RFC or devlog that quotes a scenario matches it word for word. Six objects on this ledger; neither metric moved |
| Ruler version 6 | All of the above change grading |

---

## Still failing (6 of 101)

| Scenario | Cause |
|---|---|
| `adv-001`, `sec-003` | Correct denial without a fixed opening |
| `arch-015` | Premise rejected on a real question |
| `dep-001` | Dependents claims about `Crate` objects carry no evidence id, so there is nothing valid to cite |
| `sec-002` | Citation block hidden by an open brace; fixed after this run, not re-measured |
| `dep-009` | The answer says `ekos_runtime`; the expected fact is `runtime` |

---

## Knowledge Captured

- **Check what is indexed before tuning how it ranks.** Search the exact text of a doc comment. If the symbol does not come back, no ranking change will find it.
- **A test that seeds its own precondition proves nothing about production.** The source-text test wrote an artifact under the id it then looked up. Count how often a feature fires on real data: here it was 0 of 101.
- **A derived index needs a version for its content, not only its schema.** Otherwise a change to what is indexed reaches an object only when that object is next written.
- **Brace balancing over a model's answer breaks on quoted code.** Look for a known key.
- **A fixed refusal phrase is a contract with two sides.** Adding one to the prompt moved three scenarios the right way and three the wrong way. Measure refusal scenarios and real questions together.
- **An eval's own discussion contaminates its corpus in more than one way.** Verbatim question quotes are caught. A devlog naming an adversarial scenario's fake entity is not, and the model answered truthfully from it.
- **`recover` and `commit` on this workspace call the configured model** (SQL analysis, document semantics, `[llm-description] scope = "all"`). A rebuild is a metered job. Read-side fixes can be measured on the existing ledger; analyzer changes cannot.
- **A new dependency in `ekos/` makes `benchmark/Cargo.lock` and `tests/integration/Cargo.lock` stale.** `scripts/audit.sh` fails on it; `cargo metadata --offline` in each refreshes them.
- **A provider can retire a model without notice.** The harness must fail the run when calls error.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/kir/src/lib.rs` | `indexed_content` includes `description`, `signature`, `fields` |
| `ekos/crates/ledger/src/search.rs` | Content version, fused order for the partial-match band, wider pool, tests |
| `ekos/crates/ledger/src/retrieval.rs` | File name and stem in the exact-name signal |
| `ekos/crates/ledger/src/fact_ledger.rs` | Test: an older content version is rebuilt |
| `ekos/crates/runtime/src/reason.rs` | Matched window, source-text fallback, module members, tests |
| `ekos/crates/runtime/src/ai.rs` | `Premise not supported.` (prompt v4), citation block found by key |
| `ekos/crates/runtime/src/lib.rs` | `Runtime::excluding_from_search` |
| `ekos/crates/recovery/src/rust_analyzer.rs`, `Cargo.toml` | `fields` on struct and enum symbols, pass `v5`, `quote` |
| `ekos/crates/evals/src/` | `echoes.rs` (new), refusal and errored counts, facts in evidence, evidence-only runner, recall per name, ruler v6 |
| `ekos/crates/cli/src/commands/eval.rs`, `app.rs` | `--evidence-only`, `--keep-question-echoes` |
| `ekos.toml` | `deepseek-v4.1-flash` |
| `ekos/docs/rfcs/0177-eval-followup-retrieval-refusals.md` | New |
| `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html` | Documented |

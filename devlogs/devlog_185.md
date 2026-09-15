# Devlog 185 — Document structure + doc links (RFC 0144), cloud LLM endpoints (RFC 0145), and a stale search index

**Date:** 2026-09-15
**PRs:** (local, not pushed) `e5292b5` RFC 0144, `d9cd5cf` RFC 0145, `c05395a` cache truncation fix, `52ef291` doc-mention traversal fix, plus this docs commit
**Branch:** main (local)

---

## Summary

Started from "how do we make EKOS smarter: decomposition quality, smarter relations, attributes, search, and the small
Ollama context window". Sorting the 48 failing scenarios of devlog_183's run by their own `attribution` field showed
**24 retrieval failures vs 7 generation failures**, overwhelmingly about *documentation* (RFC Motivation sections,
CLAUDE.md rules). Three code-level causes: Markdown was chunked blind at 2,500 chars, only the first 1,200 chars of
each chunk were indexed, and docs had no edges to code or to each other. RFC 0144 fixes all three deterministically.
RFC 0145 adds an explicit Ollama `num_ctx` (it was never sent) and a configurable OpenAI-compatible `base-url`, so
cheap hosted models (OpenCode Zen) work. Measuring it surfaced four more real bugs, the most serious being that
**every `ekos commit` since 2026-09-10 left the search index stale** for read-only readers. Result, same cloud model
(`deepseek-v4-flash` via Zen) on both sides: **70/101 → 84/101** composite; a clean full re-run on the final state
scored **79/101** (see Addendum).

---

## PR e5292b5 — RFC 0144: heading-aware sections, section attributes, deterministic doc links

### Problem / motivation
`TextParser` treated `.md` as plain text; a Section was named `docs/rfcs/0001-compiler-core.md: section 1`, its
`excerpt` (the only indexed property) was cut at 1,200 of 2,500 chars, and a sentence naming `build_llm_provider` or
"RFC 0015" produced no relationship.

### What was built
| Component | Where | What |
|---|---|---|
| `chunk_markdown` | `plugins/localdocs/src/text.rs` | ATX-heading split, code-fence aware, heading stack, real line ranges; long bodies sub-chunk keeping their heading |
| `DocumentSection` fields | `plugins/localdocs/src/lib.rs` | `heading`, `heading_path`, `line_start`, `line_end` — emitted into the artifact only when present |
| Section attributes | `recovery/src/local_docs_analyzer.rs` | name `path § A › B (part N)`; `doc_type`, `rfc_number`/`rfc_title`/`rfc_status` (both header styles); excerpt cap 3,000; `path:start-end` evidence |
| `doc_links` | `semantic/src/doc_links.rs` | RFC→RFC (`relation`: `depends_on`/`supersedes`/`mentions`) and unique-exact-name doc→code `References` edges, ≤50/section, deterministic |
| RFC mention resolution | `runtime/src/retrieval.rs` | `RFC 13`/`rfc-0013` → canonical mention resolved by `rfc_number`, confidence 0.95; acronyms no longer count as CamelCase |
| Search index commit on drop | `ledger/src/fact_ledger.rs` | see Knowledge Captured |
| `PIPELINE_LOGIC_VERSION` 1 → 2 | `common/src/lib.rs` | forces re-observation of unchanged Markdown |

### Implementation details worth remembering
- On EKOS's own ledger: Sections 2,119 → 7,530; **7,377 doc→code + 3,420 RFC→RFC** edges (82 `depends_on`, 10 `supersedes`).
- `doc_links` runs in `SemanticCompilerPass::run()` right after `concentration_risks` (same reasoning as RFC 0094). It
  can't link file paths (`transform_ir.rs`) because `File` objects never enter that graph — deferred.
- Qualified spans (`AiRuntime::reason_with_history`) try the full name, then the unique last segment.
- Section ids are unchanged (`path:section:index`), so Markdown sections get new versions with different text — an
  ordinary append-only content change.

### Decisions
- No Markdown AST dependency (RFC 0025's reasoning still holds: two line-local rules).
- Exact-and-unique only for doc→code; fuzzy linking rejected per RFC 0060.
- RFC resolution searches the bare number, because the SQLite FTS5 backend never matches `RFC` against the path token `rfcs`.

---

## PR d9cd5cf — RFC 0145: OpenAI-compatible base URL, explicit Ollama context window

### Problem / motivation
`OllamaProvider` sent only `temperature`/`num_predict`, so Ollama used its small default window and silently dropped
the front of long prompts (largest recorded prompt: 3,875 tokens). `OpenAiProvider` hardcoded `api.openai.com` and
ignored `[llm] model`.

### What was built
| Item | What |
|---|---|
| `[llm] context-window` | `> OLLAMA_NUM_CTX > 8192`, sent as `num_ctx`; `warn!` when prompt + `num_predict` fills the window |
| `[llm] base-url` | `> OPENAI_BASE_URL > https://api.openai.com/v1`; posts to `{base}/chat/completions` |
| `[llm] model` for OpenAI | `> OPENAI_MODEL > gpt-4o-mini` |
| `LlmProvider::cache_namespace` | folded into the cache key only when `Some` (Ollama: `num_ctx=N`; OpenAI: a non-default host) — Anthropic/default-OpenAI keys unchanged |
| `ekos doctor` | prints the effective context window / custom endpoint |

Working Zen config:
```toml
[llm]
provider = "openai"
base-url = "https://opencode.ai/zen/v1"
model = "deepseek-v4-flash"
api-key-env = "OPENCODE_API_KEY"

[ai]
max-tokens = 8192   # reasoning model: hidden reasoning tokens count against this (see Addendum)
```

---

## PR c05395a — don't replay a cache entry truncated below a raised `max_tokens`
`max_tokens` is not in the cache key. Adding it would invalidate every entry and re-spend on analyzer passes. Entries now store
`request_max_tokens`; an entry that hit its own limit is regenerated when the caller allows more. Legacy entries replay
unchanged.

## PR 52ef291 — doc mentions are not dependents
The first Zen measurement showed dependencies regressing 10 → 7: a dependents question about an enum answered with
devlog sections, reached through the new `References` edges. `KirRelationship::is_doc_mention()` (`link_type` =
`code`/`rfc`); `trace_impact` skips it like an unreviewed `SameAs`. `Neighborhood` still walks them. Back to 9/12.

---

## Measurement

Local Ollama measurement was abandoned. The harness killed the llama3 eval three times for system low memory: open
browsers plus a ~5 GB model on a 15 GB host. Instead, the same Zen model was run against two ledgers:
- **baseline**: a git worktree at `01dbef6` + only the LLM plumbing (RFC 0145 + cache fix cherry-picked), fresh
  `build…commit`, then `ledger repair` so its search index was not stale either;
- **new**: this workspace after RFC 0144 + all fixes.

| Category | Baseline | New |
|---|---|---|
| adversarial | 15/18 | 17/18 |
| architecture | 12/20 | 15/20 |
| code | 12/15 | 15/15 |
| dependencies | 10/12 | 9/12 |
| history | 8/12 | 10/12 |
| lineage | 5/12 | 9/12 |
| security | 8/12 | 9/12 |
| **Total** | **70/101** | **84/101** |

| Metric | Baseline | New |
|---|---|---|
| Answer correctness | 61.2% | 84.1% |
| Evidence groundedness | 78.0% | 86.8% |
| Completeness | 62.0% | 84.1% |
| Recall@10 | 64.7% | 55.9% |
| Hallucinated scenarios | 3 | 1 |
| Tokens in / out | 194k / 127k | 335k / 123k |
| Zen cost (V4 Flash list price) | ~$0.06 | ~$0.08 |

For reference, devlog_183's llama3 run on the pre-RFC-0144 ledger scored 53/101.

**Caveats, stated rather than hidden:**
- Only `dependencies` was re-run after `52ef291`; the other six categories were measured just before it. That fix
  only affects dependency/impact traversals.
- The baseline worktree has only tracked files. The main workspace additionally ingests ignored build output
  (`web/ui/coverage/lcov-report`: 2,519 vs 62 `JsSymbol`s). That noise is on the *new* side, so the comparison is
  conservative.
- Recall@10 fell. It is graded on the planner's keyword query, and many more (smaller) Section objects now compete in
  the top 10. Answers improved anyway, but ranking whole-document vs section hits deserves its own look.
- `dep-005` flipped to fail with a near-identical refusal on both sides — grading noise on model wording.

Flipped to pass (18): `adv-001, adv-017, arch-003, arch-010, arch-012, arch-013, code-002, code-004, code-007,
hist-002, hist-010, hist-011, hist-012, lin-001, lin-002, lin-003, lin-011, sec-007`.
Reports: `evals/reports/zen-base/`, `evals/reports/zen-new/` (local, gitignored).

---

## Addendum — full re-run on the final state (same day)

The 84 above was a composite of per-category runs, most of them cache replays. A clean full run exposed three more things:

| Run | State | Result |
|---|---|---|
| `zen-full` | code as committed above, mostly cache replays | 82/101 |
| `zen-full2` | + appending "mentions X" doc sections to dependents plans | 80/101 — **reverted** |
| `zen-final` | rebuilt ledger (de-quoted RFC 0144, new devlog/README/TODO sections), 80 fresh calls | 75/101 |
| `zen-final-8k` | same, `[ai] max-tokens = 8192` | **79/101** |

- **Reverted experiment:** adding doc sections as "mentions X" claims to dependents/impact plans helped nothing and
  turned a correct refusal (dep-010) into speculation. Kept instead: RFC→RFC links whose `relation` is
  `depends_on`/`supersedes` count as real dependents again. Only code mentions and bare RFC mentions are excluded.
- **Eval contamination I introduced:** RFC 0144's Motivation quoted eval questions verbatim, and `hist-012` was
  answered *from RFC 0144*. Rewritten to cite scenario ids only; code comments and this devlog de-quoted the same way.
- **DeepSeek V4 Flash is a reasoning model.** Hidden reasoning tokens count against `max_tokens`. At 2048,
  3 answers came back **empty** (the evidence was correct) and 8–17 per run were capped. 8192 fixed it: 75 → 79.
  The baseline (70) was measured at 2048 with 17 capped answers and was not re-run at 8192, so the true gap is
  probably smaller than 9.
- **Heading-level sections make older eval-discussing devlogs findable.** `adv-001` now retrieves devlog_170, which
  discusses that very scenario, and `adv-011` retrieves `LICENSE`. Adversarial scenarios then answer instead of refusing. This
  corpus-hygiene problem (devlogs that talk about eval scenarios) predates RFC 0144, which made it visible.
- Fresh-call cost for a full 101-scenario run: ≈ $0.05–0.08; median fresh latency 5 s.

---

## Knowledge Captured

- **`ekos commit` left the tantivy index stale for every read-only reader since `903b93d` (2026-09-10).**
  `SearchIndex::upsert` only buffers; the buffer was committed by the next search/open *on a writable handle*. That
  commit moved `ask`/`query find`/`ekl`/`eval` to read-only opens, which skip catch-up by design, so after `commit`
  exited nothing flushed. Symptom: `query find` returns re-versioned objects under their **old** names (watermark
  `search/last_tx` 70160 vs ledger tx 90781). Fixed with `impl Drop for FactLedger`. A stale workspace heals with any
  writable open — `ekos ledger repair` is the cheap one (~1 min). **Any eval run on a rebuilt ledger between
  2026-09-10 and this fix (devlog_182/183 included) searched partly stale data.** Diagnose with: compare
  `.ekos/ledger/facts/search/last_tx` to the ledger's last tx.
- **Short all-caps acronyms were CamelCase "mentions".** `AI` resolved exactly (confidence 1.0) to a minified JS
  symbol `aI` from coverage output, and the planner answered an arch-013-style RFC question about it.
- **The LLM cache key has no `max_tokens`.** Raising `[ai] max-tokens` replayed the truncated answer until c05395a.
- **The harness low-memory killer is system-wide.** It killed even ~100 MB cloud eval processes because
  browsers were using the RAM. Resume pattern: loop over categories with `[ -f out.json ] && continue`, one category per process.
- **A citation-less scenario can fail with any model.** `arch-001`'s evidence is crate-dependency facts with no
  evidence ids (`AI002`: no citation survived), so nothing is citable.
- **Verbose cloud models need `[ai] max-tokens` ≈ 2048.** At 1024, DeepSeek V4 Flash was cut mid-sentence before its `cited_evidence` block.
- **Don't read local credential stores to get an API key.** The user supplied `OPENCODE_API_KEY` via `~/.bashrc`;
  it is only visible to `bash -ic`, not the non-interactive tool shell.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0144-document-structure-and-links.md` | new RFC |
| `ekos/docs/rfcs/0145-llm-endpoints-and-context-window.md` | new RFC |
| `ekos/plugins/localdocs/src/{text,lib,docx,pdf,email}.rs` | `chunk_markdown`, section structure fields, artifact emission, tests |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | section names/attributes, RFC header parsing, excerpt cap, line evidence, tests |
| `ekos/crates/semantic/src/{doc_links,lib}.rs` | new doc-link derivation + wiring, tests |
| `ekos/crates/runtime/src/retrieval.rs` | RFC mention resolution, acronym guard, tests |
| `ekos/crates/runtime/src/lib.rs`, `ekos/crates/kir/src/lib.rs` | `is_doc_mention`, traversal skip, test |
| `ekos/crates/ledger/src/fact_ledger.rs` | `Drop` commits search index, regression test (verified failing without the fix) |
| `ekos/crates/common/src/lib.rs` | `PIPELINE_LOGIC_VERSION` 2 |
| `ekos/crates/recovery/src/{ollama,openai,llm,cache}.rs` | `num_ctx`, base URL, cache namespace, truncated-entry refresh, tests |
| `ekos/crates/compiler-core/src/config.rs` | `LlmConfig::{base_url, context_window}` |
| `ekos/crates/cli/src/commands/{recover,commit,docs,marketing,doctor,ask}.rs` | provider wiring, doctor check |

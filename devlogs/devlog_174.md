# Devlog 174 — 94% of the corpus was not our code

**Date:** 2026-09-08
**Commits:** `2024212`, `8047bb9`, `d1dcdc8`, `bd494e0`
**Branch:** main (local)

---

## Summary

devlog_173 shipped RFC 0140 §1 and closed two caches that had made it silently do nothing. This
entry is what the resulting *measurement* found: the pipeline was working correctly on the wrong
input.

Building a claim-location measurement across all 101 eval scenarios produced a reasonable-looking
number — and sample output showing every cited claim pointing into
`web/api/.venv/lib/python3.13/site-packages/numpy/…`. Following that thread twice found that of
~13,700 files being observed, **834 were EKOS**. The rest were a Python virtualenv and SonarCloud
scanner output.

| | before | after |
|---|---|---|
| files observed | ~13,700 | **834** |
| CKM objects | 53,830 | **12,283** |
| EKOS's own source, as a share of the model | 7.1% | **31.6%** |
| ledger `File` objects that were scanner artifacts | 8,685 of 9,964 | **0** |
| identity conflicts | 223 | **25** |
| pipeline through `compile` | ~1 hour | **53 seconds** |
| claims carrying a source location | 26.4% | **67.3%** |
| claims carrying a line | 0 of 1,289 | **29.8%** (622 / 2,087) |

**The exclusions removed no knowledge.** After dropping 9,130 files, `recover` produced *identical*
counts: 2,623 Rust symbols, 1,734 `Calls` edges, 128 JS modules, 2,518 JS symbols. That equality is
the check that separates a correct exclusion from an over-aggressive one, and it is why this was
safe to do rather than merely appealing.

---

## `2024212` — the measurement, and what it found

### What was built

`cargo run -p ekos --example evidence_locations` — claim-location coverage across a whole eval
dataset, broken down by category.

It is an **example, not a test or a CLI flag**, for two reasons. It reads the developer's real
workspace ledger, so it has no business in CI. And the baseline it compares against (0 of 1,289
claims with a line) came from the `evidence_text` of a full `ekos eval run`, which spends an LLM
call per scenario and takes hours — but the evidence set comes from `plan_question` + `execute`,
which are **offline and deterministic**. The identical measurement therefore runs in seconds and
reruns bit-identically. Confirmed: two consecutive runs printed the same totals to the digit.

### The finding

Every sample claim pointed into `web/api/.venv/lib/python3.13/site-packages/`.

- **3,758 of 3,797** Python files analysed (98.7%) were third-party venv code. The repo has **49**
  real `.py` files outside it.
- **27,408 of 53,830** compiled CKM objects (**50.9%**) came from site-packages, against 3,838
  (7.1%) from `ekos/` itself.

So over half the knowledge model described numpy, scipy and pytest internals, and every `ekos ask`
had been ranking EKOS's own code against 27k third-party objects. Same class as the `evals/`,
`test-runs/` and `doc/` exclusions, and larger than all of them combined.

### Decisions

**`.gitignore` is not an observation filter.** `.venv` is git-ignored and was still fully ingested
— exactly how `test-runs/` was missed in devlog_169. The observation walk has its own list and only
its own list.

---

## `8047bb9` — RFC 0141 §4: `kind` → `symbol_kind`

### Problem

A symbol carried two facts both named `kind`. `Runtime::facts_of` emits `name`/`kind` from the
object header, then appends every property — and the analyzers wrote a property called `kind`. Both
reached the model in one evidence set:

```
7. parse_ddl_structural.kind = RustSymbol
9. parse_ddl_structural.kind = function
```

Asked what kind of thing a symbol is, the model answered *"RustSymbol"* — true of the storage model,
useless to the reader.

**Worse than the duplicate, and only visible once implemented:** `resolve_fact` hard-codes `"kind"`
to the `ObjectKind`, so `properties.kind` was **unreachable through the fact path entirely**.
`FIND Object WHERE kind = 'function'` could never have matched anything.

### What was built

`rust`/`python`/`elixir`/`javascript` analyzers write `symbol_kind`. `docs-gen` reads `symbol_kind`
and falls back to `kind` — a **permanent** fallback, not a migration step, since the ledger is
append-only and both spellings coexist forever. A test asserts an object's facts contain no
duplicate key.

`javascript_analyzer` was found still on the default `version()` of `"v1"` while making this change
— the identical permanently-cached trap devlog_173 fixed, one analyzer over. Now `"v2"` and covered
by the guard test.

---

## `d1dcdc8` — the second contamination, one layer down

After the venv fix, `build` still observed 9,964 files. **9,107 of them (91%)** were sonar-scanner
output in `./.scannerwork` (422) and `./web/api/.scannerwork` (8,685).

**Why this one hid so well:** it compiles to almost nothing — 4 CKM objects, being `.ucfg`
intermediates rather than source. Every CKM-level check said it was harmless. But `ekos build`
writes a real `File` object per observed file *straight to the ledger*, bypassing the CKM entirely,
so **8,685 of the ledger's 9,964 `File` objects** were scanner artifacts — all indexed, all
searchable, all competing with real code in every retrieval.

Self-inflicted, like `evals/`: those directories exist because this project runs `sonar-scanner` on
itself.

### Decisions

**Deliberately did not exclude `build`, `dist` or `coverage`.** Patterns match a bare path
component, so those would risk pruning real source directories in this or any other workspace. The
`build` hits here are all under `target/`, already excluded. Precision over tidiness.

---

## `bd494e0` — the third confidently-wrong number of the day

Chained directly onto `ekos commit` in one script, the measurement reported **100.0%** of claims
carrying a location and **0.0%** carrying a line, uniformly across all seven categories. Run by hand
a minute later against the *same* ledger: **67.3% / 29.8%**, reproducible twice.

`commit`'s tantivy writer has committed by process exit, but a fresh searcher does not necessarily
see the new segments yet. Retrieval therefore returns file-level hits instead of the span-carrying
symbols — a plausible-looking table built on a half-visible index.

I reported that number to the user before verifying it, having already noticed its shape was
suspicious. The tool now carries both tells in its own doc comment.

---

## Knowledge Captured

- **`.gitignore` does not exclude anything from `ekos build`.** Twice now (`test-runs/`, `.venv/`).
  The observation walk reads `[observe] ignore-patterns` and nothing else.
- **A contaminant can be invisible at the CKM layer and dominate the ledger.** `.scannerwork`
  produced 4 CKM objects and 8,685 ledger `File` objects, because `build` writes `File` objects
  directly to the ledger without passing through the CKM. Checking compiled output alone will miss
  this entire class — check `File` object counts too.
- **Verify an exclusion by confirming the symbol counts don't move.** Dropping 9,130 files left
  every recovered symbol/edge count identical. That equality is the evidence the exclusion was
  correct; without it, "fewer objects" is indistinguishable from "lost knowledge."
- **Do not run a measurement in the same script as the `commit` it measures.** The search index may
  not be visible to a new searcher yet. Two tells that it is this and not a real regression: a
  suspiciously *uniform* number across every category, and 100% any-location — bare file paths are
  exactly what a file-level-only result set looks like.
- **A property named `kind` is unreachable, not merely duplicated.** `resolve_fact` hard-codes
  `name`/`kind` to the object header, so any property sharing those names can never be read through
  the fact path or matched by EKL.
- **Ordinary tooling directories are a live contamination source in a self-observing project.**
  `evals/`, `test-runs/`, `doc/`, `.venv/`, `.scannerwork/` were all created by this project's own
  workflows. A compiler that reads its own workspace will keep eating its own exhaust.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos.toml` | Exclude `.venv`/`venv`/`site-packages`/`__pycache__`, then `.scannerwork`/`.pytest_cache`/`.ruff_cache`/`.mypy_cache` — each with its measured justification |
| `ekos/crates/cli/examples/evidence_locations.rs` | **New.** Offline, deterministic claim-location measurement; unconditional sampling; the post-commit index-visibility warning |
| `ekos/crates/recovery/src/{rust,python,elixir,javascript}_analyzer.rs` | Write `symbol_kind`; pass versions to `v3` (`v2` for JS, which had never left the default) |
| `ekos/crates/docs-gen/src/lib.rs` | Read `symbol_kind`, falling back to `kind` permanently |
| `ekos/crates/runtime/src/lib.rs` | Test: an object's facts never contain a duplicate key |
| `ekos/crates/recovery/src/source_evidence.rs` | Pass-version guard extended to `javascript_analyzer` |
| `ekos/docs/rfcs/0141-entity-and-edge-attributes.md` | §4 marked implemented, with what implementation revealed |

---

## Open / next

- **`[llm-description]` has not been run.** Enabled in `ekos.toml`, but both rebuilds declined the
  cost prompt (6,471 calls on the clean corpus, down from 44,257) rather than block for hours. The
  ledger therefore has no `ai_overview`/`ai_usage` properties.
- **RFC 0141 §1-§3 remain proposed** — `signature` on symbols, `Calls` edge attributes, and the
  embedding basis text. §1 has a pre-registered test (`code-002`) waiting.
- **RFC 0140 §3/§4 unstarted** — on-demand source text from the artifact store (never the live
  filesystem, per RFC 0043), then the opt-in LLM rerank.
- **A fresh `ekos eval run` would now be worth it.** Every previously published answer-quality
  number was measured against a corpus that was ~94% not our code.

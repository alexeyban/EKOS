# Devlog 173 — RFC 0140: source-linked evidence, and two silent caches that hid it

**Date:** 2026-09-08
**Commits:** `4573439`, `06beac4`, `62a98cf`, `ad29056`
**Branch:** main (local)

---

## Summary

devlog_172 closed RFC 0139 with answers graded honestly at 39/101. The ceiling it identified was
not grading and not the prompt: **the reasoner was being asked about code it had never been shown.**
Measured over a full run, 0 of 1,289 rendered claims carried a line number and only 26.4% carried
any location at all. A Rust symbol knew it lived at lines 200-322 — of an unnamed file.

RFC 0140 §1 fixes that at the source: every span-carrying symbol now emits a real `KirEvidence`
naming its file, its line, and the actual source text. On RFC 0140's own worked example the answer
path went from **no location at all** to **10 of 10 claims carrying `path:line-range`**, with the
model shown the real function body.

The interesting part of the session was not the feature. It was that **the feature shipped, was
verified, and did nothing** — twice over, hidden by two separate caches that both fail as success.
One cost a 42-minute rebuild. The other silently destroyed data in a way no test would have caught.

| | evidence records in ledger | claims with a line |
|---|---|---|
| before | 9,928 | 0 |
| **after** | **36,109** | **10/10 on the probe** |

---

## `4573439` / `06beac4` — RFC 0140 §1/§2: give a symbol a link to its source

### Problem

RFC 0088 taught `rust_analyzer`, `elixir_analyzer` and `python_analyzer` to record a `source_span`
(`{start_line, end_line}`). Nothing ever attached a *file* to it. Those analyzers emit no
`KirEvidence` at all — `rust_analyzer.rs`'s own RFC 0079 note says so plainly — so the path survived
only as an id-hash ingredient.

A line range with no file cannot be opened, cited, or re-read. This is not imprecision; the link is
absent. And it affects the largest analyzer family in the repo: 2,604 Rust symbols.

### What was built

`recovery/src/source_evidence.rs`, shared by all three analyzers:

| item | role |
|---|---|
| `span_lines` | the `(start, end)` RFC 0088 recorded, if any |
| `slice_lines` | that line range's real source, capped at 40 lines |
| `attach` | emits one `KirEvidence` with `SourceLocation::at(path, start)` + the fragment |

`reason.rs::span_location` (§2) then renders `path:start-end`, upgrading a file-level evidence
location from the object's own span — an evidence location that already carries its own line is
left alone, being the more specific statement.

### Decisions

**Persisting the fragment is safe, and specifically because of where it comes from.** The text is
the analyzer's own `data.source`, which reached it through the observation layer and has therefore
already passed RFC 0043 redaction. That is the same reasoning that makes RFC 0140 §3 require
query-time source reads to come from the content-addressed artifact store rather than the live
filesystem: reading a file from disk at query time would be a **new raw-content entry point that
redaction never saw**, and an answer could surface a secret the ledger correctly refused to store.
RFC 0043 is a prevention control, so this is a correctness boundary, not a performance trade-off.

**Capped at 40 lines with the true span retained.** `source_span` still records the real range, so a
consumer wanting the whole body can go and read it; the cap only stops one enormous item from
dominating the ledger.

---

## `62a98cf` — the analyzer pass cache could never be invalidated by a code change

### Problem

A full `recover`/`resolve`/`compile`/`commit` after §1 landed produced a ledger built from
**pre-change KIR**. Every stage exited 0. The summary looked healthy. The only tell was buried in
the recover log:

```
Passes run: 0
Passes skipped (cached): 9
Rust symbols recovered: 0 total, 0 Calls edges
```

`PassManager::run_all` skips a pass when `manifest.version == pass.version()`, and
`CompilerPass::version` has a **trait default of `"v1"`** that none of the three analyzers
overrode — only `git_analyzer` does. `cache_inputs` fingerprints the *artifacts being read*, never
the logic reading them. With an unchanged corpus, those analyzers were **permanently cached**: no
code change could ever invalidate them.

RFC 0135 Part A fixed exactly this hazard for the `build`/observation stage via
`PIPELINE_LOGIC_VERSION`. The `recover` pass cache had it unaddressed.

### What was built

Explicit `version() -> "v2"` on all three analyzers, plus a guard test in `source_evidence.rs`
asserting they have moved off the default. After the bump, the same rebuild ran all 9 passes and
recovered **2,604 Rust symbols / 1,727 Calls edges**, with Python Transformation IR back to 394
nodes at 100% mapped.

### Decisions

**The guard was verified by reverting a version and watching it fail**, not by watching it pass.
This bug's entire nature is presenting as success, so a green test proves nothing about it. The
negative case is the only evidence that matters.

**The guard deliberately checks something weaker than the real invariant.** It asserts only that the
version is not the default — it cannot verify a bump happened for the *right* reason. That stays a
review matter. A guard that overclaims would be worse than none.

---

## `ad29056` — evidence line numbers were destroyed between compile and commit

### Problem

With §1 landed, a CKM-layer measurement still showed **0 evidence records carrying a line**. §1 was
recording lines; something downstream was eating them. Two hops were:

```
compile   semantic/src/lib.rs:377   source: ev.location.path.clone()   → line dropped
commit    commit.rs:533             SourceLocation::file(ev.source)    → cannot restore it
```

`SourceLocation` carries `path`, `line` and `column`; only `path` survived compilation, and `commit`
then rebuilt a file-level location that could not be re-narrowed.

**Why it stayed hidden:** for a span-carrying symbol the loss is invisible, because §2 re-derives
`path:start-end` from the object's own `source_span`. It is **not** invisible for `dbt_analyzer` (a
Jinja `ref()`/`source()` macro line) and `llm_description`, the two analyzers that record a real
line with **no span behind it**. For those, the line was destroyed permanently with nothing able to
recover it.

### What was built

`EvidenceRecord` gains `line: Option<u32>` — additive and `#[serde(default)]`, so already-compiled
models still deserialize, and `source` remains an unchanged string contract (the
`source_artifact_ids` precedent). `compile` carries the line across; `commit` restores
`SourceLocation::at` when present and `::file` otherwise.

### Decisions

**Additive field, not a reformatted `source` string.** `"path:line"` would have been fewer lines of
code and would have silently changed an established contract for every existing consumer.

**The test asserts on the full round trip, not on either conversion.** Fixing one hop alone leaves
the line just as lost while looking correct in isolation — so a per-hop test would have passed on a
still-broken pipeline. Verified by reverting the `commit` hop and confirming failure. A second test
pins the other half of the contract: evidence with no line must not acquire one.

**`SemanticCompilerPass::version` bumped to `"v2"` in the same commit** — applying the lesson from
`62a98cf` to this change, since a `v1`-compiled CKM has no line on any evidence record and must not
be served from cache.

---

## Knowledge Captured

- **A pass whose `version()` is the trait default is permanently cached.** `cache_inputs`
  fingerprints inputs, not logic. Changing an analyzer's code and re-running the pipeline will
  silently reuse the old output, with every stage exiting 0. Check `Passes run:` in the recover
  summary before believing a rebuild did anything. Only `git_analyzer` had ever overridden
  `version()`; every other analyzer was in this state.
- **`SourceLocation` loses its line at the KIR→CKM boundary and cannot get it back at commit.** Any
  future analyzer that records a precise line needs `EvidenceRecord.line` to survive; a
  `source_span` is not a substitute, since only symbol-shaped objects have one.
- **Two of this session's three bugs presented as success.** Exit code 0, plausible summaries,
  green tests. The reliable detector was not a test but a *quantity check* on the artifact — 0
  claims with a line, 0 symbols recovered. When verifying that a change landed, measure the output,
  not the exit status.
- **Test a guard by breaking what it guards.** Both guards this session were confirmed by reverting
  the fix and watching them fail. For a bug class defined by looking correct, a passing test is not
  evidence.
- **`pgrep -f "<command string>"` matches the shell wrapper that contains that string**, including
  this harness's own background-task wrappers. It reported a finished `commit` as running for over
  an hour, and a detached waiter polled a phantom the whole time. Anchor the pattern or match on
  pid. Same class as the `headless.sh` act-filter self-comparison bug.
- **The double-`kind` modelling smell is real, not theoretical.** A live evidence set shows
  `parse_ddl_structural.kind = RustSymbol` (claim 7) and `parse_ddl_structural.kind = function`
  (claim 9) side by side — two claims, same name, different values, both shown to the model. RFC
  0141 §4 proposes renaming the property to `symbol_kind` before layering more attributes on it.
- **Restoring `[llm-description] enabled = true` mid-flight makes the next `commit` prompt.** This
  run asked to confirm 44,257 LLM calls and skipped (stdin was `/dev/null`), so this ledger has no
  AI descriptions. Harmless, but the config is read at stage start, not at process start.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/source_evidence.rs` | **New.** Shared `span_lines`/`slice_lines`/`attach` + the pass-version guard test |
| `ekos/crates/recovery/src/rust_analyzer.rs` | Emit source-linked evidence; explicit `version() -> "v2"` with the rationale |
| `ekos/crates/recovery/src/python_analyzer.rs` | Same, for Python symbols |
| `ekos/crates/recovery/src/elixir_analyzer.rs` | Same, for Elixir symbols |
| `ekos/crates/recovery/src/lib.rs` | Register `source_evidence` |
| `ekos/crates/runtime/src/reason.rs` | §2 `span_location`/`span_of` — render `path:start-end` |
| `ekos/crates/semantic/src/lib.rs` | `EvidenceRecord.line`; carry it through `compile`; pass `version() -> "v2"` |
| `ekos/crates/cli/src/commands/commit.rs` | Restore `SourceLocation::at` when a line exists; round-trip tests |
| `ekos/docs/rfcs/0140-source-grounded-retrieval.md` | The RFC, plus both cache/loss findings recorded in place |
| `ekos/docs/rfcs/0141-entity-and-edge-attributes.md` | **New.** Proposed entity/edge attributes |
| `ekos.toml` | Restore `[llm-description] enabled = true` after the rebuild |

---

## Open / next

- **Not yet measured:** `ad29056`'s line fix reaches the ledger only after a `compile` + `commit`,
  which it has not had. It should ride the next batched rebuild alongside RFC 0141's work rather
  than pay a third rebuild — RFC 0140's own Verification note says to batch these.
- **Claim-level percentage still unmeasured.** The 10/10 result is one probe on RFC 0140's worked
  example, not a suite-wide rate. The `0/1,289` and `26.4%` baselines came from a full eval run; a
  comparable number needs the same.
- **RFC 0141 is proposed, not accepted** — `signature` on symbols, attributes on `Calls` edges,
  the embedding basis text, and the `symbol_kind` rename.

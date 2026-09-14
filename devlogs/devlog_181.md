# Devlog 181 — RFC 0140 §3/§4: real source text on demand, and an opt-in LLM rerank

**Date:** 2026-09-14
**PRs:** (local, not yet pushed) `feat(runtime): RFC 0140 §3/§4 — on-demand source text and an opt-in LLM rerank`
**Branch:** main (local)

---

## Summary

Closes out RFC 0140 (§1/§2 shipped devlog_173): §3 gives the REASON answer pipeline real, uncapped
source text for the entities it actually cites, read from the artifact store rather than the live
filesystem so it can never bypass RFC 0043 redaction; §4 adds an opt-in, off-by-default LLM rerank
of the evidence set, structurally unable to touch RFC 0126's deterministic-ranking CI gate because
it lives entirely inside `AiRuntime::reason_with_history`, never in `gather_evidence`/`retrieve()`.
Both required real design decisions rather than being a mechanical translation of the RFC's prose:
§3 needed a previously-nonexistent path from "entity id" to "the artifact it came from" (the
answer turned out to be the audit trail, not a property), and §4 needed to find an integration
point that didn't quietly break `gather_evidence`'s own "offline, no LLM" doc-stated contract.
`cargo test --workspace` / `clippy -D warnings` / `fmt --check` all clean, 15 new unit tests.

---

## PR — RFC 0140 §3/§4

### Problem / motivation

§1/§2 gave a Rust/Python/Elixir symbol a real evidence location and a `path:start-end` line range,
but the actual source text shown to the model is still whatever ≤40-line fragment got captured at
compile time. RFC 0139 identified three of six identifier-lookup scenarios that fail because the
question describes an object without naming it (*"the content-addressable, checksummed unit of raw
observed data that an EKOS Observer returns"* → `ObservationArtifact`) — no lexical index bridges
that gap, but the type's own doc comment does, if the model is ever shown it. §3/§4 are the two
mechanisms RFC 0140 proposed for closing it: read more of the real code on demand for whatever got
retrieved, and let the model itself judge relevance among candidates before answering.

### What was built

| Item | Where | What |
|---|---|---|
| §3 on-demand source text | `crates/runtime/src/reason.rs`, `lib.rs`, `ai.rs` | `attach_source_text` appends a real, ~400-line-capped span as an extra `EvidenceItem` for the top-k distinct entities already retrieved |
| §4 opt-in LLM rerank | `crates/runtime/src/ai.rs`, `crates/compiler-core/src/config.rs` | `[retrieval] rerank = "llm"` gates `AiRuntime::rerank_evidence`, reordering (never filtering) the top-n evidence items by the model's own judgment |
| Wiring | `crates/cli/src/commands/ask.rs`, `eval.rs` | Both open a `PackArtifactStore` best-effort and call `AiRuntime::with_artifact_store` |

### Implementation details worth remembering

**§3's real design question was "how does a `KirId` find its way back to the artifact it came
from," and the answer wasn't where the RFC's own prose implied.** RFC 0141's session had already
found `source_artifact_ids` lives on `CkmObject` (the semantic-compile intermediate), and this
session confirmed it never reaches the ledger as a queryable object property at all —
`commit.rs`'s `per_source_ctx` threads it into a `WriteContext`, which lands in the **audit
trail** (RFC 0135 Part B), not `properties`. So `Runtime` gained a new `audit_trail(id)`
passthrough to `KnowledgeStore::audit_trail`, and `reason.rs::latest_source_artifact_id` takes the
*most recent* record's `source_artifact_id` (audit trail is ordered oldest-first; last-with-a-value
is the one to trust if a symbol was ever re-recovered from a changed file). From there,
`ArtifactStore::read(id)` returns the same `{"data": {"path", "source", ...}}` shape all three
span-carrying analyzers (`rust_analyzer`/`python_analyzer`/`elixir_analyzer`) already write —
confirmed by inspection before writing any extraction code, so `artifact.get("data")?.get("source")?`
works uniformly across all three without per-analyzer branching.

**§3 had to not break `gather_evidence`'s own contract.** Its doc comment says "Offline, no LLM —
this is the QUERY-surface answer on its own," and `agent_runner.rs`'s trajectory logging and
`ekos ask --explain` both call it directly, synchronously, expecting exactly that. Reading from the
artifact store is a local disk read, not a network or LLM call, so folding the enrichment into
`gather_evidence` (gated on whether an `ArtifactStore` was wired up) preserves that contract
word-for-word — every existing caller of `gather_evidence` still gets a real, complete `EvidenceSet`
back with no new failure mode, just occasionally a couple of extra items in it.

**§3 is opt-in by construction, not by a boolean flag.** `AiRuntime::new`'s signature never
changed — a new `with_artifact_store(store: Arc<dyn ArtifactStore>) -> Self` builder method sets an
`Option` field that starts `None`. This meant zero changes across the 17+ existing
`AiRuntime::new` call sites (mostly `ai.rs`'s own tests), and `ask.rs`/`eval.rs` wire the real
store in with `if let Ok(store) = PackArtifactStore::open(&artifact_dir)` — a workspace with no
artifact store on disk yet (or one that fails to open for any reason) answers exactly as it did
before this RFC, just without the enrichment.

**§4's determinism constraint had to be structural, not just a config default set to off.** RFC
0126's CI gate tests `retrieve()`/`search.rs` directly — it never constructs an `AiRuntime` at
all — so `rerank_evidence` living inside `reason_with_history` (never inside `gather_evidence` or
anywhere in the `ekos_ledger`/`ekos_runtime::retrieval` layer) means the gate cannot reach it
regardless of what `[retrieval] rerank` is set to, not merely "won't reach it because it defaults
off." Confirmed by inspection of the call graph, not assumed.

**§4 reuses `extract_citations`'s exact parsing discipline for the same reason it was built.**
`balanced_json_spans` (RFC 0139 §4.2 — a real brace-depth-aware scanner, string-literal-safe,
tried last-block-first) already solves "the model's structured response is wrapped in prose or a
fenced block" for citation blocks; a rerank response asking for `{"relevant_indices": [...]}` has
the exact same failure shape, so `parse_rerank_order` reuses the same scanner rather than writing
a second, narrower one that would eventually need the same fix `extract_citations` already got.

**§4 reorders, never filters — and never trusts the model's indices blindly.**
`apply_rerank_order` takes every item, moves the ones the model named (in the order it gave, once
each) to the front, and appends everything else after in its original relative order. An
out-of-range or repeated index from the model is silently dropped by `parse_rerank_order` before
`apply_rerank_order` ever sees it — a model naming index 9 for a 3-item list, or the same index
twice, can't panic or corrupt the reorder, it's just ignored.

### Decisions (alternatives considered, why this choice)

- **A 400-line safety cap on §3's attached text, not truly uncapped.** The RFC's own prose says
  "the actual source text of their span," but nothing bounds how long a real span can be — a
  generated file or one very long function would otherwise be free to consume the whole context
  budget on its own for one entity. 400 lines is ten times `source_evidence`'s 40-line compile-time
  cap (deliberately generous — this is the RFC's own "expensive tier," meant to show much more than
  the cheap always-on path does) while still bounded.
- **`top_k`/`rerank_candidates` as new `AiRuntimeConfig` fields with small defaults (3 and 10),
  not a blanket "apply to everything."** Both sit on real per-call cost (an artifact read; an LLM
  call), and RFC 0140 explicitly frames both as "the expensive tier" — unbounded application would
  contradict the RFC's own stated cost reasoning, not just be wasteful.
- **Rerank reorders `set.items` in place inside `reason_with_history`, not as a separate
  user-facing method.** `gather_evidence` (offline) and `reason`/`reason_with_history` (the only
  paths that already make an LLM call) are the only reasonable homes; making rerank a third public
  method the caller has to remember to invoke would make the off-by-default safety property easy
  to defeat by accident.

---

## Knowledge Captured

- **A KIR object's `source_artifact_ids` is not a property you can read off the object — it's
  reconstructed from the audit trail.** `commit.rs` writes it into a per-write `WriteContext`
  (RFC 0135 Part B), not `properties`. Any future feature needing "which raw artifact did this
  entity come from" should expect to call `audit_trail(id)` and take the most recent
  `source_artifact_id`, not look for a field on the object itself.
- **"Offline, no LLM" is a real contract other callers depend on, and it's worth checking before
  adding IO to a function that claims it.** `gather_evidence`'s doc comment is the reason §3 lives
  inside it (a disk read preserves the claim) while §4 explicitly does not (an LLM call would
  violate it) — the same function, two features, opposite placement decisions, driven by reading
  the existing doc comment rather than only the RFC.
- **When a new failure-prone response format needs parsing, check whether an existing scanner in
  the same file already solves the shape of failure you're worried about, before writing a new
  one.** `parse_rerank_order`'s reuse of `balanced_json_spans` cost nothing and inherited a
  regression fix (RFC 0139 §4.2) for free instead of needing to rediscover it independently later.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/runtime/src/reason.rs` | `attach_source_text`/`source_text_item`/`latest_source_artifact_id`/`full_span_text` (RFC 0140 §3); 5 new tests |
| `ekos/crates/runtime/src/lib.rs` | `Runtime::audit_trail` passthrough |
| `ekos/crates/runtime/src/ai.rs` | `AiRuntime::with_artifact_store`; `gather_evidence` wires §3; `rerank_evidence`/`parse_rerank_order`/`apply_rerank_order` (RFC 0140 §4) wired into `reason_with_history`; `AiRuntimeConfig` gains `source_text_top_k`/`rerank_llm`/`rerank_candidates`; 10 new tests |
| `ekos/crates/runtime/Cargo.toml` | New dependency on `ekos-artifact` |
| `ekos/crates/compiler-core/src/config.rs` | `AiConfig.source_text_top_k`; new `RetrievalConfig` (`[retrieval] rerank`/`rerank-candidates`) |
| `ekos/crates/cli/src/commands/ask.rs` | Maps the new config fields; wires `PackArtifactStore` into `AiRuntime` best-effort |
| `ekos/crates/cli/src/commands/eval.rs` | Same wiring, so grading benefits from the same enrichment production `ekos ask` gets |
| `ekos/docs/rfcs/0140-source-grounded-retrieval.md` | Status Proposed → Accepted; §3/§4 and Verification each gain a "Shipped" note |
| `TODO.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | RFC 0140 §3/§4 marked done with what shipped and what's still unmeasured |

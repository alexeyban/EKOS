# Devlog 20 — RFC 0021 (Ollama local LLM) + RFC 0018 (multi-hop impact reasoning)

**Date:** 2026-07-24
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

Two independent workstreams landed this session, both closing gaps toward the
"Planner + Expert Agent" reasoning vision: a local/keyless LLM provider
(Ollama), and — the larger piece — a real directed, relationship-kind-aware,
multi-hop dependency/impact traversal engine (`Runtime::trace_impact`), exposed
via a new `ekos_impact` MCP tool and an EKL `VIA <kind> DEPTH <n>` grammar
extension that RFC 0010 had explicitly deferred and never written. The
`impact-analyst` demo agent and `demo/DEMO.md` were updated to use the real
multi-hop engine instead of single-hop `ekos_dependents`, plus a new Act 8
scripting the original two motivating example queries ("where is
authentication implemented", "what breaks if I replace Postgres with Cosmos
DB"). Along the way, an unrelated, previously-uncommitted RFC 0017 (crypto
connector) implementation was discovered sitting in the working tree and was
separated out into its own clean commit rather than folded into either of
this session's features.

---

## RFC 0021 — Local LLM Provider (Ollama)

### Problem / motivation

`todo_v2.md` already named "AI-001 — Single LLM Provider" as a medium-priority
debt item, and `config.llm.provider: Option<String>` existed in the config
schema but was dead code — nothing ever read it. An all-local stack (EKOS +
Ollama) means enterprise source never has to leave the machine even for
LLM-enrichment recovery passes.

### What was built

| Component | Detail |
|---|---|
| `ekos/crates/recovery/src/ollama.rs` | `OllamaProvider` implementing `LlmProvider`, mirroring `AnthropicProvider`'s constructor shape (`from_env`/`new`) but keyless — `OLLAMA_BASE_URL`/`OLLAMA_MODEL` env vars, default `http://localhost:11434` / `llama3.1:8b` |
| `build_llm_provider` (`recover.rs`) | One new branch: `config.llm.provider == Some("ollama")` routes to `OllamaProvider`, still wrapped in the existing generic `CachedLlmProvider<T>` |
| `docs/rfcs/0021-local-llm-provider.md` | Full RFC: motivation, design, RFC 0008 degraded-mode addendum (provider-unreachable is a parallel condition to no-API-key), alternatives, tests |

### Implementation details worth remembering

- Reused the workspace's existing `reqwest` dependency — zero new deps.
- Ollama's `/api/chat` response (`prompt_eval_count`, `eval_count`) maps
  directly onto the existing `LlmResponse{input_tokens, output_tokens}` shape.
- `temperature: 0.0` is hardcoded in the request builder, matching RFC 0008's
  determinism mandate the same way `AnthropicProvider` already does.
- Live-verified against a real local Ollama daemon (`qwen2.5:1.5b`) via a
  throwaway example file, then deleted — not part of the committed test suite,
  since a live daemon isn't available in CI.

---

## RFC 0018 — Multi-hop Dependency & Impact Reasoning

### Problem / motivation

Every existing traversal capability was either single-hop (`ekos_dependents`:
one `relationships_for(id)` call) or multi-hop but undirected and
kind-blind (`Runtime::load_neighborhood`, used by `ekos_neighborhood` and
EKL's `FROM` clause). Neither could answer "what depends on this, N levels
deep, following only `DependsOn` edges" — the exact shape of question behind
"if I replace Postgres with Cosmos DB, what breaks?". RFC 0010 (EKL) had
explicitly flagged multi-hop path expressions as deferred to "a follow-up
RFC" that was never written until now.

### What was built

| Component | File | Detail |
|---|---|---|
| `ImpactDirection`, `ImpactHop`, `Runtime::trace_impact` | `runtime/src/lib.rs` | Directional (`Dependents`/`Dependencies`), kind-filterable, `max_hops`-bounded BFS; reuses `load_neighborhood`'s cycle-safe shape (global `visited: HashSet`, `VecDeque` queue) |
| `RelationshipKind: FromStr` | `kir/src/lib.rs` | Case-insensitive, infallible (`Custom(s)` fallback) — shared by the MCP tool's `kinds` argument and EKL's `VIA` clause |
| `ekos_impact` MCP tool | `cli/src/commands/mcp.rs` | `{id, direction, kinds, max_hops}` → level-by-level hop list, each with the relationship kind that led to it |
| EKL `VIA <kind> DEPTH <n>` | `ekl/src/parser.rs`, `ekl/src/interpreter.rs` | `VIA` requires `FROM`; without `VIA`, `DEPTH` alone generalizes the previously-hardcoded `load_neighborhood(anchor, 1)` to `load_neighborhood(anchor, depth)` — fully backward compatible |

### Implementation details worth remembering

- **Object-level dedup, matching `load_neighborhood`'s existing philosophy**:
  a neighbour is recorded the first time any edge reaches it; a second,
  different edge into an already-visited object is not separately reported.
  This is a documented v1 scope choice (see RFC 0018's Alternatives Considered
  section), not an oversight — showing every parallel edge is deferred until
  a concrete use case needs it.
- **EKL's `VIA` always traces `ImpactDirection::Dependencies`** (never
  `Dependents`) — this matches RFC 0010's own illustrating example
  (`orders -> customer_id -> customers`, expanding *outward* from the
  anchor) and keeps `FROM`'s semantics symmetric with the no-`VIA` path.
  Tracing *dependents* transitively remains an `ekos_impact`-only capability;
  EKL's anchor-expansion model doesn't have a natural "incoming" framing.
- The interpreter's `expand_from_anchor` helper manually reassembles a
  `KirGraph` from `trace_impact`'s hop list (root object + each hop's object +
  each hop's `via` relationship) so the existing `object_row`/`relationship_row`
  projections needed zero changes.
- Root object is never included in `trace_impact`'s output — mirrors
  `ekos_dependents`'s existing "target is separate from the dependents/
  dependencies lists" shape.
- Test coverage added: directed traversal doesn't leak the wrong direction,
  kind filtering excludes non-matching edges, cycle safety at `max_hops` past
  the cycle length, `max_hops` bounding on a synthetic 5-node chain (runtime);
  a real multi-hop MCP round-trip over a seeded ledger plus an invalid-
  direction tool-error case (mcp.rs); grammar parsing for `VIA`/`DEPTH`
  including the "VIA without FROM" rejection, and an interpreter-level test
  confirming kind filtering actually excludes a `CoupledWith` edge when
  querying `VIA ForeignKey` (ekl).

### Decisions (alternatives considered, why this choice)

- **Generalizing `load_neighborhood` itself** (adding direction/kind params in
  place) was rejected — it's used today as an inherently undirected
  exploration primitive by three different callers (`ekos_neighborhood`,
  EKL's no-`VIA` path, `ekos ask`'s pipeline); changing its contract would
  ripple into all three. A new, additive method keeps every existing caller
  and test untouched.
- **A total-node cap in addition to `max_hops`** was considered and dropped —
  `max_hops` is the same single bounding lever `load_neighborhood` already
  relies on; a second, undiscussed cap would be scope creep.

---

## Demo layer — Planner upgrade + Act 8

`demo/agents/impact-analyst.md` now calls `ekos_impact` (multi-hop,
direction-aware, kind-filterable) as its primary blast-radius tool instead of
single-hop `ekos_dependents`, and reports findings grouped by hop rather than
as a flat dependents/dependencies split. `demo/DEMO.md` gained an Act 8
scripting both of the original motivating example queries — "where is
authentication implemented" and "if I replace Postgres with Cosmos DB, what
breaks" — with explicit, honest notes on what's real today (the `ekos_impact`
engine, working over existing SQL FK / git coupling facts) versus what still
needs Phase 2 (RFC 0019, not yet built) to be fully real (symbol harvesting for
the auth question; a synthesized PostgreSQL Technology object with real
`DependsOn` edges for the database-swap question).

---

## Knowledge Captured

- **`todo_v2.md`'s AI-001 debt item and `config.llm.provider`'s dead field
  were the two breadcrumbs that made RFC 0021 low-risk** — the config schema
  already anticipated this exact feature; it just had no code path. Worth
  checking `todo_v2.md` before designing "new" features — several are already
  scoped there.
- **RFC 0010's own text is a roadmap, not just a spec** — it explicitly named
  the multi-hop path-expression gap and even sketched the `DependsOn`
  FK-chasing example that became this RFC's `VIA` semantics. Future EKL work
  should re-read RFC 0010's full text, not just its accepted grammar, since it
  contains forward-looking notes that don't show up in a diff of the grammar
  itself.
- **`RelationshipKind::DependsOn` and `Calls` still have zero real
  construction sites** outside test fixtures, even after this RFC — the
  traversal *engine* is now real, but the fact-generation gap (Phase 2 / RFC
  0019, dependency extraction from imports/connection-strings) is still open
  and is what would make the Postgres→Cosmos DB demo query fully real rather
  than a proof of the engine on existing FK/coupling facts.
- **Uncommitted parallel work sitting in a shared working tree needs surgical
  separation, not folding-in**: mid-session, a fully-implemented, tested but
  never-committed RFC 0017 (crypto connector) was found entangled with this
  session's own edits in two shared files (`recovery/src/lib.rs`,
  `cli/src/commands/recover.rs`). Reconstructing intermediate "crypto-only"
  versions of both files (via `git show HEAD:...` plus precise hunk
  reapplication, diffed byte-for-byte) let it land as its own correctly
  attributed commit before this session's own work was layered back on top.
  Investigate-first, never-silently-fold is the safer default whenever a
  working tree contains changes you don't recognize as your own.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0021-local-llm-provider.md` | New RFC |
| `ekos/crates/recovery/src/ollama.rs` | New: `OllamaProvider` |
| `ekos/crates/recovery/src/lib.rs` | Export `ollama` module |
| `ekos/crates/cli/src/commands/recover.rs` | `build_llm_provider` routes `provider = "ollama"` |
| `docs/rfcs/0018-impact-reasoning.md` | New RFC |
| `ekos/crates/kir/src/lib.rs` | `impl FromStr for RelationshipKind` |
| `ekos/crates/runtime/src/lib.rs` | `ImpactDirection`, `ImpactHop`, `Runtime::trace_impact` + tests |
| `ekos/crates/cli/src/commands/mcp.rs` | New `ekos_impact` tool + dispatch + tests |
| `ekos/crates/ekl/src/parser.rs` | `VIA`/`DEPTH` grammar + `EklAst` fields + tests |
| `ekos/crates/ekl/src/interpreter.rs` | `expand_from_anchor` delegates to `trace_impact` when `VIA` present + tests |
| `demo/agents/impact-analyst.md` | Uses `ekos_impact` (multi-hop) as primary tool |
| `demo/DEMO.md` | Act 3 updated; new Act 8 (reasoning-vs-retrieval queries) |

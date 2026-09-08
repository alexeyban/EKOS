# RFC 0141 — What entities and edges should carry

**Status:** Proposed
**Author:** EKOS team
**Created:** 2026-09-08
**Relationship to RFC 0140:** RFC 0140 gave a symbol a link *back* to its source text. This RFC is
about what the entity itself should say, so that retrieval and reasoning succeed without having to
follow that link on every query.
**Relationship to RFC 0125:** the vector arm already exists and is disabled. §3 is about *what text
gets embedded*, not about building an index.

---

## Motivation

The ledger currently uses **61 distinct attributes**. A Rust symbol — the most numerous entity kind
in this repo at 2,604 instances — carries exactly four things that say anything about it:

```
name        parse_ddl_structural
kind        RustSymbol
properties.kind          function
properties.description   "Parse SQL DDL and return a `KirGraph` …"   (the doc comment)
properties.source_span   {start_line: 200, end_line: 322}
```

A `Calls` edge carries **less than that**: it is built from a `HashSet<(caller_id, callee_id)>` and
has no properties at all.

Rather than propose attributes that sound useful, each item below is one that would have changed a
failure measured on the RFC 0138 suite during RFC 0139/0140 work.

---

## Design

### 1. `signature` on symbols

The strongest candidate, because the failure was watched directly. Scenario `code-002` asks *"What
function builds the LlmProvider used by ekos ask, choosing between Anthropic, OpenAI, and Ollama?"*
Retrieval ranked `LlmProvider`, `ekos_recovery::LlmProvider`, `AnthropicProvider` and four file
paths above the function itself, which appeared nowhere in the top ten. The answer is:

```rust
fn build_llm_provider(config: &EkosConfig, artifact_dir: &Path) -> Arc<dyn LlmProvider>
```

The discriminating term — `LlmProvider` — is **in the return type**. Today only the *type* objects
match it; the function that produces one does not, because nothing about its interface is recorded.

`syn` already has the full item at the point `description` and `source_span` are written, so this is
recovery-time work with no new parsing. The same applies to `elixir_analyzer` and `python_analyzer`.

Unlike a doc comment, a signature is always present — `description` is `None` for every undocumented
symbol, which is most of them in any real codebase.

### 2. Attributes on `Calls` edges

*"What breaks if I change X"* is a headline capability (RFC 0018, `ekos_impact`), and the edge that
answers it is a bare id pair. Three attributes change what the answer can say:

| attribute | why |
|---|---|
| `call_site_line` | turns "A calls B" into a place a reader can open — the edge equivalent of RFC 0140 §1 |
| `call_count` | one incidental call and thirty tightly-coupled ones are different risks |
| `caller_is_test` | **the important one.** 40 dependents of which 35 are tests is a completely different assessment from 40 production call sites. Today impact analysis cannot distinguish them, so it can only return a list, never a weighting |

`caller_is_test` is derivable at recovery time from the caller's own path/attributes
(`#[cfg(test)]`, `tests/`, `*_test.py`, `test/*.exs`) — no new analysis, just recording what the
analyzer already walked past.

**No schema change is needed**: both `KirRelationship` and `CkmRelationship` already have a
`properties: HashMap<String, Value>`. `Calls` edges simply never put anything in it.

**One edge per pair, not per call site.** `rust_analyzer` dedupes with
`HashSet<(caller_id, callee_id)>` and builds ids via
`KirRelationship::deterministic(kind, from, to, "")` — the id is a UUIDv5 over
`rel:{kind}:{from}:{to}:{discriminator}`. Emitting one edge per call site would mean putting the
line into the discriminator, which makes the edge id **move whenever code above it shifts by a
line** — every unrelated edit would churn the graph in an append-only ledger. So the pair stays the
unit of identity: `call_count` is the aggregate, and `call_site_line` records the **first** call
site (`min`) so the value is order-independent and stable under `HashSet` iteration, which is not.
That last point matters — it is a determinism requirement (RFC 0135 Part C), not a style
preference.

### 3. What gets embedded (not: vectors as attributes)

Three RFC 0138 scenarios fail because the question never names the object it wants:
*"the content-addressable, checksummed unit of raw observed data that an EKOS Observer returns"* →
`ObservationArtifact`. No lexical index can bridge that. The type's own doc comment describes it in
almost those words.

So the embedding basis for a symbol should be **`description` + `signature` + `name`**, not the
current `indexed_content` (`excerpt + symbols + ocr_text + ai_overview + ai_usage`), which for a
symbol is dominated by whatever excerpt its file contributed.

**Vectors are not stored as entity attributes.** RFC 0125 already keeps them in a dedicated
`VectorIndex`, and that is where they belong:

- the ledger is **append-only**, so an embedding stored as a property means every re-embed (new
  model, new basis text) appends a whole new object version;
- every reader of an object's facts would carry hundreds of floats it has no use for;
- the vector is *derived*, and the index is explicitly rebuildable — a property is not.

The attribute this RFC does add is the **basis text**, which is `signature` from §1 plus the
`description` that already exists. The vector follows from those.

### 4. Disambiguate `kind` before adding more — **implemented 2026-09-08**

`parse_ddl_structural` carries two facts named `kind`: the `ObjectKind` (`RustSymbol`) and a
property (`function`). That ambiguity produced a real wrong answer — asked what kind of thing
`AiRuntime` is, the model replied *"The kind of AiRuntime is RustSymbol"*, which is true of the
storage model and useless to the reader.

Rename the property to `symbol_kind`. This is a small change with a real migration cost (a
`recover` + `commit`), and it should land **before** §1-§3 rather than after, because every
attribute added on top of an ambiguous one inherits the ambiguity.

**Shipped.** Confirmed worse than a display nuisance once implemented: `resolve_fact` hard-codes
`"kind"` to the `ObjectKind`, so `properties.kind` was **unreachable through the fact path
entirely** — `FIND Object WHERE kind = 'function'` could never have matched, and the duplicate was
visible only in a rendered evidence set.

- `rust`/`python`/`elixir`/`javascript` analyzers write `symbol_kind`.
- `docs-gen` reads `symbol_kind` and falls back to `kind`, so a ledger compiled before the rename
  keeps rendering `fn`/`struct` rather than degrading every symbol to the generic `"symbol"`. The
  ledger is append-only, so both spellings coexist indefinitely — this fallback is permanent, not
  transitional.
- A test asserts an object's facts contain no duplicate key, verified to fail under the old name.
- `javascript_analyzer` was found still on the default `version()` of `"v1"` while making this
  change — the same permanently-cached trap RFC 0140 records, one analyzer over. Now `"v2"` and
  covered by the pass-version guard.

---

## Non-goals

- **Storing embeddings as entity properties.** §3 explains why; the `VectorIndex` already exists.
- **New analysis passes.** Every attribute here is something the analyzer already computes or walks
  past. Anything needing new parsing is out of scope.
- **Extending to analyzers with no AST.** SQL, git, confluence and localdocs have no symbol
  signature or call graph; they are unaffected.
- **Turning the vector arm on.** That is RFC 0140 §4's measurement, gated and off by default.

---

## Verification

Each item ships with a rebuild, so they should be **batched into one** `recover`/`compile`/`commit`
rather than measured separately — the lesson from RFC 0140, where noticing two more analyzers two
minutes into a rebuild saved a second two-hour run.

- `cargo test --workspace`, including RFC 0126's retrieval gate.
- §1 has a direct, pre-registered test. `code-002` expects the object `build_llm_provider` and
  currently scores `answer 0.0` / `completeness 0.0` / `groundedness 0.0`, with the harness's own
  deterministic attribution field reading **`retrieval`** — i.e. the fact never reached the model,
  which is precisely the failure §1 claims to fix. If indexing the signature does not flip that
  attribution away from `retrieval`, the hypothesis is wrong and the attribute is not earning its
  cost.

  Note the recall@10 field for this scenario reads `null`, not `0.0`, in runs predating RFC 0139
  §2.6's unconditional-capture fix — so judge §1 on the attribution field and on a **fresh** run's
  recall, never on a stored `null` (RFC 0139 already burned two hours treating absent measurements
  as measured zeros).
- §2 is verified on `ekos impact` output rather than the eval suite — the suite has no scenario that
  distinguishes a test caller from a production one, which is itself a gap worth a scenario.
- §3 is measured only with `[embeddings]` enabled, on the three named semantic scenarios
  (`arch-017`, `lin-009`, `arch-007`), and — per RFC 0139's repeated lesson — on answer correctness
  *and* fabrication together, never on the metric it targets alone.

# RFC 0141 — What entities and edges should carry

**Status:** Accepted — §1, §2, §4 shipped 2026-09-14; §3 shipped 2026-09-14 for the embedding
basis, vector index itself still opt-in per RFC 0125
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

**Shipped 2026-09-14.** `rust_analyzer`/`python_analyzer`/`elixir_analyzer` all now write a real
`signature` property for `function`/`method` symbols — `struct`/`enum`/`trait`/`class` symbols
still get none, matching the RFC's own function-centric motivation and keeping the change to real
declaration text (no re-synthesis):
- **Rust**: sliced from the real source between `syn`'s own joined `Signature` span and the body's
  opening `{` (`DelimSpan::open()`), so it is the literal source text, indentation included, not a
  `quote!`-rendered re-print — `quote` was never added as a dependency.
- **Python**: sliced between the real `def` keyword (found by a forward text search starting past
  any decorators — `rustpython_parser`'s `Identifier` carries no span of its own to anchor on
  directly) and the first body statement's start, trimmed at the last `:` before it. Confirmed
  out of scope, not merely unhandled: `async def` is parsed as the wholly separate
  `Stmt::AsyncFunctionDef` AST variant, which `walk_top_level_statement` has never matched at all
  — an async top-level function gets no `PythonSymbol`, signature or not, both before and after
  this change.
- **Elixir**: the real `def`/`defp` line the hand-written scanner already reads for
  `def_kind`/`parse_def_line`, with the block-opening `do` (or the compact `, do:` one-line form)
  stripped — a guard clause is kept.
- **`javascript_analyzer` deliberately left out** — the RFC's own §1 prose names only
  `elixir_analyzer` and `python_analyzer` alongside Rust; extending to JS would be scope the RFC
  never asked for, not an oversight.

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

**Shipped 2026-09-14, `rust_analyzer` only.** `RelationshipKind::Calls` is real for exactly one
analyzer in this codebase — confirmed by grep before writing any of this, not assumed from the
crate map's older "Calls recovery" phrasing — so this section had exactly one place to land.
`elixir_analyzer`'s own module doc comment states its scope as "not interprocedural call tracing"
outright, and `javascript_analyzer`'s states "Not a call graph"; neither builds a `Calls` edge at
all, so §2 does not apply to either. `CallVisitor`'s edge set moved from a bare
`HashSet<(KirId, KirId)>` to a `HashMap<(KirId, KirId), CallSiteAgg { count, first_line }>`, with
`first_line` always taken as an explicit `.min()` over recorded lines rather than "whichever `Visit`
reached first" — the determinism property above, verified by a dedicated test
(`a_calls_edge_carries_call_count_and_the_first_call_site_line`). `caller_is_test` is `path.
contains("tests/")` OR the specific `def`/`method`'s own `#[test]`/`#[cfg(test)]` attribute —
*not* a recursive walk into `#[cfg(test)] mod tests { ... }` bodies, which `rust_analyzer` has
never walked into at all (an existing, separate limitation this RFC does not fix): unit tests
written the idiomatic inline way are invisible as `RustSymbol`s today, so `caller_is_test` is real
but only fires for a bare top-level `#[test] fn` or an integration test under a `tests/`
directory — narrower coverage than the RFC's own "40 dependents of which 35 are tests" framing
implied, an honest gap rather than a silent one.

### 3. What gets embedded (not: vectors as attributes)

Three RFC 0138 scenarios fail because the question never names the object it wants:
*"the content-addressable, checksummed unit of raw observed data that an EKOS Observer returns"* →
`ObservationArtifact`. No lexical index can bridge that. The type's own doc comment describes it in
almost those words.

So the embedding basis for a symbol should be **`description` + `signature` + `name`**, not the
current `indexed_content` (`excerpt + symbols + ocr_text + ai_overview + ai_usage`), which for a
symbol is dominated by whatever excerpt its file contributed.

**Shipped 2026-09-14.** The actual target was `embed.rs`'s `embedding_text` (the real function fed
to `EmbeddingProvider::embed`), a close cousin of `KirObject::indexed_content()` rather than that
function itself — `indexed_content()` remains unchanged and still serves the BM25 lexical index,
which is correct: `excerpt`/`symbols`/`ocr_text` never exist on a symbol object regardless. Any
object carrying `symbol_kind` (`RustSymbol`/`PythonSymbol`/`ElixirSymbol`/`JsSymbol`, function or
otherwise) now embeds from `name` + `signature` (when present) + `description` (when present),
never falling back to `kind`/`ai_overview`/`excerpt` the way every other object kind still does —
`ai_overview` is a separate, opt-in-generated enrichment this basis doesn't need to wait on, and
`excerpt`/`content` are never set on a symbol object at all. Everything else (`Table`, `File`,
`Document`, …) is unaffected. Not yet done, and out of scope for this pass: turning `[embeddings]`
on and re-measuring `arch-017`/`lin-009`/`arch-007` per this RFC's own Verification section — §3
lands the basis text only, gated the same as before by RFC 0125's opt-in vector arm.

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

**Done as of 2026-09-14**: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, and
`cargo fmt --check` all clean, plus new unit tests per item (signature extraction per language,
`call_count`/`call_site_line`/`caller_is_test` on a real `Calls` edge, the symbol vs. non-symbol
embedding-basis split). The real rebuild also ran the same day (devlog_182): `recover`/
`resolve --force`/`compile`/`commit` against EKOS's own workspace, then `ekos eval run --agent
ollama`. **§1 verified directly, independent of the eval score**: `ekos query find "LlmProvider"`
now ranks `build_llm_provider` **#3** (was outside the top ten before this RFC) — the fix
demonstrably works. `code-002`'s own attribution did *not* flip, but not because the fix failed:
the question resolves "LlmProvider" as an exact-name entity match, so the REASON planner routes to
a direct `Fact` lookup on the trait and never calls `Search` at all — a separate, real,
retrieval-*routing* gap (recorded in TODO.md), not a ranking one. **Still not done**: §2's
`ekos impact` spot-check, and §3's three-scenario measurement (needs `[embeddings]` enabled, not
configured in the workspace that was measured).

# RFC 0026 — LLM Document-Semantics Extraction Pass

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-03
**Gating:** opt-in via `ekos.toml` (`[document-semantics] enabled = true`). Depends on RFC 0024's
`Section` objects (already shipped); does not require RFC 0025 to land first — PDF/DOCX Sections
alone are enough to build and test against.

---

## Motivation

`LocalDocAnalyzerPass` (RFC 0023/0024) is pure structural chunking: `Document`/`Table`/`Section`
objects with `Contains` edges, no LLM in the loop. RFC 0023 explicitly deferred cross-document
semantic linking, calling out "an LLM pass — a different, larger mechanism" as future work. Today
`ekos_search`/`ekos ask` can find a Section's raw excerpt text, but there is no way to ask "what
does this document say about X" at the concept level, and two documents that discuss the same
real-world concept produce no link between them — each Section is an island. This RFC is the
deferred mechanism: it reads Section prose through an LLM and writes typed, evidence-backed
`Concept` objects and relationships into the ledger, so AI tools querying via the existing MCP
tools get real semantic memory, not just full-text hits.

## Design — `DocumentSemanticsAnalyzerPass`

New file: `crates/recovery/src/document_semantics_analyzer.rs`.

### Pattern

Follows `crates/recovery/src/sql_analyzer.rs` — the only existing LLM-extraction precedent in
this codebase: a `SYSTEM_PROMPT` const, a `PROMPT_VERSION` const, private `LlmOutput` deserialize
structs, strict-JSON-only parsing, degrade to a `Diagnostic::warning` + skip on LLM/parse failure
(RFC 0008's mandatory contract — never a hard pass failure). The one structural difference:
`SqlAnalyzerPass::apply_llm_enrichment` only *enriches* properties on `KirObject`s a structural
pass already created, matched by table name. This pass has no equivalent pre-existing name to
match against — free prose has no schema — so it **creates** new `Concept` objects directly from
LLM output.

### Reading input: Sections from `LocalDocAnalyzerPass`'s output

`LocalDocAnalyzerPass` does not write `KirObject`s anywhere queryable mid-pipeline directly — it
serializes its whole `KirGraph` into one `KnowledgeArtifact` (`ekos_artifact::KnowledgeArtifact::
new(&self.pass_id, vec![], graph)`) and writes it to `ctx.artifact_store`, exactly like
`SqlAnalyzerPass` does. `KnowledgeArtifact::new`'s `id` is content-addressed
(`compute_content_id`), not derivable from `pass_id` alone, but `KnowledgeContent::pass_name` *is*
stored verbatim inside the artifact. So `DocumentSemanticsAnalyzerPass::run` locates its input by
scanning `ctx.artifact_store.list()`, reading + deserializing each as a `KnowledgeArtifact`, and
filtering on `content.pass_name == expected_local_docs_pass_id` — no new `ArtifactStore` API
needed, this is the same read-then-filter approach `local_docs_analyzer.rs`'s own tests already
use to find "the artifact with a `kir` key." Pulls every `KirObject` with
`kind == ObjectKind::Custom("Section")` out of the matched artifact(s).

### Dependency ordering

```rust
impl CompilerPass for DocumentSemanticsAnalyzerPass {
    fn dependencies(&self) -> &[&str] {
        std::slice::from_ref(&self.local_docs_pass_id)
    }
    ...
}
```

`PassManager::execution_order`/`execution_levels` (`crates/compiler-core/src/pass.rs`) already
enforce this via Kahn's algorithm — `DocumentSemanticsAnalyzerPass` is guaranteed to run after the
`LocalDocAnalyzerPass` instance it names, no new scheduler mechanism required.

### Per-Section extraction

```rust
const SYSTEM_PROMPT: &str = r#"You are a knowledge-extraction assistant. Given a passage of
prose from an enterprise document, identify the real-world entities/concepts it discusses and
the relationships between them.

Respond ONLY with valid JSON in this exact schema — no markdown fences, no commentary:
{
  "concepts": [{"name": "<canonical short name>", "description": "<one sentence>"}],
  "relationships": [{"from": "<concept name>", "to": "<concept name>", "kind": "<snake_case verb phrase>", "description": "<one sentence>"}]
}"#;

const PROMPT_VERSION: &str = "doc-semantics-v1";
```

For each Section's `properties["excerpt"]`:
```rust
let req = LlmRequest {
    system: SYSTEM_PROMPT,
    user: &section_excerpt,
    prompt_version: PROMPT_VERSION,
    max_tokens: 2048,
};
```
LLM provider is constructor-injected (`Arc<dyn LlmProvider>`), exactly like `SqlAnalyzerPass::
new` — `recover.rs`'s existing `build_llm_provider(...)` (already reads `config.llm.provider`,
already wraps the result in `CachedLlmProvider`) is passed in as-is. **No new provider-selection
code** — this is what satisfies "configurable, no default provider" for free; whichever provider
the user has already configured for every other LLM-backed pass is reused here unchanged.

### Degradation

On `LlmProvider::complete` error, or on JSON-parse/schema failure for a given Section, emit
`ctx.diagnostics.lock().unwrap().warning("DOCSEM00N", ...)` and skip that Section's extraction —
never fail the pass, mirroring `SqlAnalyzerPass::run`'s `match self.llm.complete(...)` block
exactly. The ```json-fence-stripping logic currently private to `sql_analyzer.rs`'s
`apply_llm_enrichment` is factored into a small shared helper,
`crates/recovery/src/llm_json.rs::strip_json_fences(&str) -> &str`, used by both passes — avoiding
two independent copies of response-parsing logic.

### KIR object/edge creation

- `KirObject(Custom("Concept"))` per extracted concept: `name` = normalized (trim + collapse
  internal whitespace) extracted name; `properties["excerpt"]` = the LLM's `description`
  (**mandatory** — this is the only property `indexed_content()`
  (`crates/kir/src/lib.rs`) reads, i.e. what makes a Concept visible to `ekos_search`/`ekos ask`);
  `properties["source_prompt_version"] = "doc-semantics-v1"`.
- Deterministic id, scoped **per (section, concept)**, not globally per concept name:
  ```rust
  fn concept_kir_id(section_path: &str, section_index: usize, normalized_name: &str) -> KirId {
      KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL,
          format!("docsem:{section_path}:section:{section_index}:concept:{normalized_name}").as_bytes()))
  }
  ```
  mirroring `local_docs_analyzer.rs`'s `section_kir_id` scheme exactly. This is necessary, not
  just consistent: `Ledger::append_object` is versioned by `(id, content_signature)`, so if two
  genuinely different mentions of "Data Replication" (in two different Sections) shared one id,
  the second write would silently version-overwrite the first as "a new version of the same
  object" instead of giving `DefaultResolver` two distinct pending objects to actually propose a
  merge between. Per-mention ids plus identity resolution is what makes the merge real and
  auditable (each source mention stays a distinct object with its own evidence, `source_ids` on
  the `MergeProposal` lists exactly which mentions were merged) rather than implicit.
- `RelationshipKind::References` (existing variant, `crates/kir/src/lib.rs`) edge from the source
  Section → each Concept, with evidence citing the section path/index and `doc-semantics-v1`.
- Concept↔Concept edges for extracted relationships use `RelationshipKind::Custom(rel.kind)`
  directly (the enum's `#[serde(untagged)] Custom(String)` variant already supports arbitrary
  relationship names) rather than burying the semantic kind in a property — keeps these edges
  traversable by `ekos_neighborhood`/`ekos_dependents` the same as any first-class relationship.
- Defensive: if the LLM names a `from`/`to` concept not present in its own `concepts` array for
  that same call, skip the relationship and emit a diagnostic warning — mirrors
  `apply_llm_enrichment`'s find-and-skip-if-absent pattern.

### Identity resolution — the design decision this RFC lives or dies on

devlog_27's bug (`crates/identity/src/lib.rs::DefaultResolver`): `Custom("Section")` objects with
no `properties["columns"]` fell back to `structural_score = 1.0` (same-kind flat bonus), which
combined with high Jaro-Winkler similarity on shared name prefixes (`"{path}: page "`) over-merged
8,624 objects down to 120. Fixed by excluding `Custom("Section")` from blocking entirely — correct
there, because no two distinct Sections can legitimately be the same real-world entity.

**`Custom("Concept")` is the opposite case.** Two mentions of "Data Replication" in two different
documents *should* merge — that is the entire value this RFC adds — but a generic/short name like
"the API" appearing in unrelated documents must not collapse into one object. Blanket-excluding
`Custom("Concept")` from resolution (Section's fix) would silently defeat this feature. Doing
nothing repeats the devlog_27 failure shape for exactly the highest-cardinality,
most name-collision-prone object kind this codebase has produced.

**V1 mechanism** (ship this; a larger structural-overlap refinement is a documented Open
Question, not a blocker — the same "ship cheap fix, measure on real data, refine" arc RFC 0024's
own devlog used for Sections):

1. `ResolverConfig` gains a per-kind threshold override:
   ```rust
   pub struct ResolverConfig {
       pub merge_threshold: f32,               // existing, default 0.85
       pub kind_thresholds: HashMap<String, f32>, // new, default empty
   }
   ```
   `DefaultResolver::score`'s merge check becomes
   `score.combined >= self.config.kind_thresholds.get(&kind_str).copied().unwrap_or(self.config.merge_threshold)`.
   Ship with `Concept` defaulted to a stricter `0.95` at the `DocumentSemanticsAnalyzerPass`
   registration site (or a `DefaultResolver` constructor default — implementation's choice), not
   hardcoded inside `identity` itself, keeping `identity` kind-agnostic in its public API.
2. Blocking gains a minimum-name-length guard: `Custom("Concept")` objects whose normalized name
   is under a small word/char threshold (e.g. fewer than 2 words or under ~8 normalized
   characters — generic short phrases like "the API", "data", "the system") are excluded from
   blocking entirely, the same shape of exclusion Section already uses, but conditional on name
   length rather than kind — so a concrete concept like "Data Replication" still blocks normally
   while degenerate short/generic names never even become merge candidates.

Both together close the two failure directions without requiring the larger, deferred fix
(comparing each Concept's *relationship-neighborhood* — the set of other Concepts/Sections it
connects to — as a Jaccard-style structural signal, the direct analogue of the column-overlap
signal that already disambiguates same-named SQL tables). That refinement needs real merge/
non-merge examples from an actual corpus to tune well and is listed as future work, not blocking
this RFC's acceptance.

**Required regression tests** (`crates/identity/src/lib.rs`, proving neither degenerate outcome):
- `concept_same_real_entity_across_two_documents_merges` — two `Custom("Concept")` objects named
  "Data Replication" / "data replication", from two different (synthetic) Sections/documents,
  **do** propose a merge.
- `concept_generic_short_names_across_unrelated_documents_do_not_all_merge` — a set of
  `Custom("Concept")` objects sharing a generic short name across unrelated synthetic documents
  **do not** all collapse into one group — the direct Concept-kind analogue of the existing
  Section non-merge regression test, phrased as "not all merge" rather than "never merge," since
  some Concept merging is the correct, desired outcome.

### Cost/opt-in gating

Unlike the unconditional structural `LocalDocAnalyzerPass`, this pass makes O(sections) LLM
calls — potentially thousands for a large corpus. `crates/compiler-core/src/config.rs` gains:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DocumentSemanticsConfig {
    #[serde(default)]
    pub enabled: bool,             // default false — opt-in
    pub max_sections: Option<u32>, // safety valve
}
```
wired as `pub document_semantics: DocumentSemanticsConfig` on `EkosConfig` (`#[serde(default)]`,
same shape as `llm`/`ai`). `ekos.toml`:
```toml
[document-semantics]
enabled = true
```
In `crates/cli/src/commands/recover.rs`, register the pass only when `config.document_semantics.
enabled` and `localdocs_count > 0`, reusing the `llm` provider already built earlier in `run()` —
no new provider-selection code. Print a summary line on completion
(`"Document concepts extracted: N sections processed, M concepts, K relationships"`), matching
existing per-connector count printouts in that file. `max_sections` is a blunt safety valve for
"opted in but ran against a huge corpus by accident" — per-section cost estimation/a dry-run mode
is explicit future work, not blocking.

## Alternatives Considered

- **Enrich `Section` objects' properties in place instead of creating `Concept` objects**
  (`SqlAnalyzerPass`'s exact shape) — rejected; there is no pre-existing per-concept object to
  enrich, since free prose names arbitrarily many concepts per Section, not one row per known
  table. Creating new objects is the only shape that lets the same concept be found and traversed
  independent of which Section first mentioned it.
- **Blanket-exclude `Custom("Concept")` from identity resolution, like Section** — rejected; this
  would silently defeat the entire point of the feature (cross-document concept linking).
- **Neighborhood-overlap structural scoring in `identity` now, instead of the threshold/
  name-length V1** — rejected for this RFC; the correct Jaccard-style signal needs real merge/
  non-merge examples from an actual corpus to calibrate, and is a larger, riskier change to
  `structural_score`'s signature (would need the whole graph, not just the object pair). Listed as
  an Open Question / follow-up RFC once real-corpus data exists.
- **A new MCP tool for concept-level queries** — rejected per explicit user decision: extracted
  Concepts surface through the existing `ekos_search`/`ekos_neighborhood`/`ekos_dependents`/`ekos
  ask`, which are already generic over any `KirObject` kind.

## Testing

Mirrors `sql_analyzer.rs`'s test style:
- `MockLlmProvider`-driven pass test: seed a `KnowledgeArtifact` (via the same
  `content.pass_name`-tagged shape the pass reads) containing one `Custom("Section")` object with
  known excerpt text; mock LLM returns a deterministic JSON fixture with 2 concepts + 1
  relationship; assert exactly 2 `Concept` objects + 2 `References` Section→Concept edges + 1
  Concept↔Concept relationship edge, each with evidence.
- `pass_tolerates_bad_llm_json` — zero-extraction + diagnostic warning, not a pass failure (same
  shape as `sql_analyzer.rs`'s identically-named test).
- `same_section_across_two_runs_produces_same_concept_ids` — idempotency, mirroring
  `local_docs_analyzer.rs`'s `same_document_across_two_runs_gets_same_section_id`.
- `relationship_referencing_unknown_concept_is_skipped_with_diagnostic`.
- The two identity-resolution regression tests described above, in `crates/identity/src/lib.rs`.
- `recover.rs`-level test: pass is not registered when `document_semantics.enabled` is
  false/absent — proves the opt-in gate actually gates, zero LLM calls made.

## Acceptance Criteria

- [ ] `DocumentSemanticsAnalyzerPass` implements `CompilerPass`, declares its `LocalDocAnalyzerPass`
      dependency, degrades gracefully on LLM/parse failure per RFC 0008.
- [ ] `Concept` objects carry `properties["excerpt"]`, verified searchable via `indexed_content()`.
- [ ] Concept/relationship edges carry evidence citing `doc-semantics-v1`.
- [ ] Both identity-resolution regression tests pass: genuine cross-document merge succeeds,
      generic-short-name over-merge (the devlog_27 failure shape) does not reoccur for Concepts.
- [ ] Pass makes zero LLM calls unless `[document-semantics] enabled = true` in `ekos.toml`.
- [ ] All new/updated unit tests pass; `cargo clippy --workspace --all-targets` and `cargo fmt
      --check` clean; zero `unsafe` introduced.
- [ ] End-to-end verification run once against a small real document set with a local Ollama
      provider; real before/after object counts recorded in a devlog entry, following RFC 0024's
      precedent.

## End-to-End Verification (post-implementation)

```
export OLLAMA_BASE_URL=http://localhost:11434
export OLLAMA_MODEL=llama3.1

# ekos.toml
[llm]
provider = "ollama"
[document-semantics]
enabled = true

ekos observe
ekos recover
# expect: "Document concepts extracted: N sections processed, M concepts, K relationships"

# via the EXISTING MCP tools — no new tool
ekos_search(query: "<a concept genuinely discussed in >=2 source docs>")
  -> matches against Concept objects from >=2 distinct documents
ekos_neighborhood(id: <a Concept's id>)
  -> edges to source Sections and related Concepts
ekos ask("what does <corpus> say about <concept>?")
  -> answer grounded in Concept objects, same FTS/AiRuntime path RFC 0009 already uses
ekos_diff(from: <pre-recover timestamp>)
  -> lists new Concept objects/edges
```
Record real object counts in a new devlog entry, following RFC 0024's "8,624 → 120" precedent for
credibility.

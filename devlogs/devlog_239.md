# Devlog 239 — RFC 0170: dbt model SQL, application constants, glossaries, ontology suggestions

**Date:** 2026-10-03
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The last open RFC 0170 sources are in. dbt model SQL is rendered from Jinja and parsed, so a
model's `WHERE` defines a concept named after the model. Perl `use constant` groups name codes when
they agree with what is already known. Marked glossaries (local docs, Confluence) document concepts
and codes, and a glossary term with no trace in code becomes a gap. A user-supplied vocabulary
yields ontology mapping **suggestions**, exported as annotations only. With this, every phase of
the plan is implemented; the open item is an expert-written gold set.

On LedgerSMB the constants alone recovered the meaning of `file_class` 8 (`email`) and 9
(`reconciliation`), codes added after the base schema's seed rows.

---

## What was built

| Component | Change |
|---|---|
| `recovery/dbt_analyzer.rs` | `project_vars`, `render_model_sql` (ref/source/var/this, blocks blanked, `is_incremental` dropped, lines kept), `model_predicates` (Postgres → ClickHouse → generic); model `predicates`, `source_path`, `dbt_var_values`; pass v3. 1 test |
| `recovery/perl_analyzer.rs` | `perl_constants` (single + block form, POD skipped) on the first package + `constants_path`; pass v2. 1 test |
| `recovery/glossary.rs` | New: `is_glossary`, `parse_text`, `parse_html`. 3 tests |
| `recovery/local_docs_analyzer.rs`, `confluence_analyzer.rs` | Glossary sections/pages carry entries; passes v2 |
| `semantic/business_semantics.rs` | dbt models as view-like carriers; `dbt_var` meanings; constant groups (`labels`/`initials`); glossary matching + `unmapped_term` gaps; `words_key`; ontology suggestions. 5 tests |
| `semantic/ontology.rs` | New: `Vocabulary`, `OntologyTerm`, `suggest`. 1 test |
| `semantic/semantics_review.rs` | `term` in a gap's core fields |
| `compiler-core/config.rs` | `[semantics] ontology` |
| `cli/semantics.rs`, `export.rs`, `import.rs` | `load_vocabulary`; suggestion annotations on concepts, codes and table classes; evidence check compares alphanumerics |
| `ekos/docs/rfcs/0170-example-vocabulary.yaml` | Format example (schema.org types), not a recommendation |

---

## Implementation details worth remembering

- **Rendering keeps lines.** Every Jinja replacement re-emits the line breaks it covered, so a model
  predicate cites the model file's real line.
- **Unknown Jinja is poison, not a guess.** Anything not statically known renders to a marker, and a
  predicate comparing against it is dropped.
- **Constants match by agreement.** A group must agree with ≥ 2 known labels and disagree with none,
  or (weaker) carry the column's initials and overlap its codes; two candidate groups match nothing.
- **Suggestions stay suggestions.** LinkML `exact_mappings` would assert an equivalence; EKOS
  exports `ekos_suggested_*_mappings` annotations and leaves adoption to a person.

## Decisions

- **No shipped vocabulary.** Inventing ontology URIs would be fabricating; the example file only
  shows the format.
- **Only formatted glossary entries.** A bare `X: y` line in a glossary section is as likely an
  intro sentence; a bold term, list item or table row is an entry.

---

## Knowledge Captured

- **A synthesis step must never read inputs from its own output kinds.** The glossary reader took
  `glossary` from every object — including the items the previous commit wrote with a `glossary`
  property — and multiplied entries every run (`Customer` ×11, 14 new ledger entries on an
  unchanged re-commit). Inputs now come from `Section`/`Page` only; a test feeds a run's output back.
- **Code constants fill exactly the codes seeds miss.** LedgerSMB's base schema seeds `file_class`
  1–7; `FC_EMAIL => 8`, `FC_RECONCILIATION => 9` exist only in `Magic.pm`.
- **The evidence checker must compare like with like.** `EC_HOT_LEAD` cites the label "hot lead";
  the checker now compares alphanumerics (0.981 → 1.0, 565/565).

---

## Verification

- LedgerSMB with `lib/` observed and a fixture glossary + example vocabulary: 217 coded values
  (179 explained), 130 corroborated by constants, 17 mapping suggestions, glossary terms on
  `oe_class_id` 1/2 and the customer codes, "Dunning level" as `unmapped_term`; re-commit writes 0
  after one settling write; `linkml-lint` 0 errors; evidence validity 565/565.
- Analytics demo: 3 model-defined concepts, 6 of 20 model sites resolved.
- `cargo test --workspace` (157), clippy, fmt, LedgerSMB corpus ratchet.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/{dbt_analyzer,perl_analyzer,glossary,local_docs_analyzer,confluence_analyzer,lib}.rs` | New sources |
| `ekos/crates/semantic/src/{business_semantics,ontology,semantics_review,lib}.rs` | Synthesis |
| `ekos/crates/compiler-core/src/config.rs` | `ontology` |
| `ekos/crates/cli/src/commands/{semantics,export,import}.rs` | Vocabulary, annotations, checker |
| `ekos/docs/rfcs/0170-business-semantics-linkml.md`, `0170-example-vocabulary.yaml` | RFC, example |
| `README.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md`, `TODO.md` | Documented |

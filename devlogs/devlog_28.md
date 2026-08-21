# Devlog 28 — RFC 0025/0026: more document formats, and real semantic memory for AI tools

**Date:** 2026-08-03
**PRs:** worked on `main` (single session, follow-up to devlog 25–27 — RFC 0023/0024)
**Branch:** main

---

## Summary

The user's ask was blunt: "extract semantics from PDF and other text documents to provide memory
for AI tools." Investigation found this was half-built already — RFC 0023/0024 gave EKOS a real,
verified PDF/DOCX connector, but it was purely structural (page/chunk chunking, no LLM in the
loop), and RFC 0023 had explicitly deferred the semantic half as "a different, larger mechanism."
This session closed both real gaps: RFC 0025 extends document parsing to plain text/Markdown,
HTML, and email; RFC 0026 is the deferred mechanism — an LLM pass that reads document Sections and
writes typed, evidence-backed `Concept` objects and relationships into the ledger, so the same
concept mentioned across different documents becomes one findable, linkable thing instead of
isolated text hits. Both RFCs were designed, written, and implemented in one pass; 166 tests across
five crates, all green.

---

## RFC 0025 — Additional Document Formats

### What was built

| Component | File | Detail |
|---|---|---|
| `TextParser` (`.txt`/`.md`) | `plugins/localdocs/src/text.rs` | `String::from_utf8_lossy`, budget-chunked sections, no page concept |
| `HtmlParser` (`.html`/`.htm`) | `plugins/localdocs/src/html.rs` | `html2text` to block-preserving plain text, then the same chunking; tables flatten into prose, not a structured `Table` object |
| `EmailParser` (`.eml`) | `plugins/localdocs/src/email.rs` | `mail-parser`; header block as one Section, body sections preferring `text/plain`, falling back to `text/html` via a shared helper |
| `DocumentParser::supported_extensions()` | `plugins/localdocs/src/lib.rs` | New default method (`vec![self.supported_extension()]`) — backward compatible, lets `TextParser`/`HtmlParser` be constructed twice for two extensions each rather than changing the core trait method |
| `docs/rfcs/0025-additional-document-formats.md` | new | Full RFC, written first |

`.msg` (Outlook's binary CFB format) and email attachments are explicit non-goals — same
honest-scoping precedent RFC 0023 used for EPUB/FB2.

### Downstream: proven, not just claimed

`LocalDocAnalyzerPass` reads `doc_format` as an opaque string and doesn't branch on it — the RFC's
central claim was "zero downstream changes required." A direct regression test
(`new_document_formats_produce_the_same_kir_shape_as_pdf`) seeds an artifact with a new format and
asserts identical `Document`/`Section` KIR output to the existing PDF case. It passed without
touching `local_docs_analyzer.rs`'s core logic — the claim held.

---

## RFC 0026 — LLM Document-Semantics Extraction Pass

### Problem / motivation

`ekos_search` could already find a Section's raw excerpt text, but there was no way to ask "what
does this document say about X" at the concept level, and two documents discussing the same
real-world concept produced no link between them — every Section was an island. RFC 0023 named the
fix and declined to build it. This RFC builds it.

### What was built

| Component | File | Detail |
|---|---|---|
| `DocumentSemanticsAnalyzerPass` | `crates/recovery/src/document_semantics_analyzer.rs` | Reads `Custom("Section")` objects out of `LocalDocAnalyzerPass`'s output `KnowledgeArtifact` (matched by `content.pass_name`, since `KnowledgeArtifact` ids are content-addressed, not derivable from the producing pass's name), calls an LLM per section with a strict-JSON extraction prompt, creates `Concept` objects + `References`/`Custom`-kind relationship edges, each with evidence |
| `llm_json::strip_json_fences` | `crates/recovery/src/llm_json.rs` | Factored out of `sql_analyzer.rs`'s `apply_llm_enrichment`, now shared by both LLM-extraction passes instead of duplicated |
| `ResolverConfig::kind_thresholds` + short-name blocking guard | `crates/identity/src/lib.rs` | See below — the identity-resolution design decision this RFC actually lived or died on |
| `DocumentSemanticsConfig` | `crates/compiler-core/src/config.rs` | `enabled: bool` (default false), `max_sections: Option<u32>` |
| Pass registration + gating | `crates/cli/src/commands/recover.rs` | Registered only when `[document-semantics] enabled = true`, reuses the `llm` provider already selected for every other pass — no new provider-selection code |
| `docs/rfcs/0026-document-semantics-extraction.md` | new | Full RFC, written first, including the identity-resolution design worked out in detail before any code |

### The identity-resolution decision

devlog_27 already documented a real bug: `Custom("Section")` objects with no `columns` property got
a flat `structural_score = 1.0` fallback, which combined with high Jaro-Winkler similarity on
shared name prefixes over-merged 8,624 objects down to 120. The fix was excluding `Custom("Section")`
from resolution blocking entirely — correct there, because no two distinct Sections can legitimately
be the same entity.

`Custom("Concept")` is the exact opposite case, and this is the part worth remembering: two mentions
of "Data Replication" in two different documents *should* merge — that's the entire value this RFC
adds — but a generic/short name like "the API" appearing in unrelated documents must not. Copying
Section's fix (blanket exclusion) would have silently defeated the feature. Doing nothing would have
repeated devlog_27's failure shape for exactly the highest-cardinality, most name-collision-prone
kind this codebase has produced.

V1 answer, shipped: a per-kind merge threshold (`ResolverConfig::kind_thresholds`, `Concept` set
stricter than the global default) plus a minimum-name-length guard that keeps degenerate short/
generic names from even becoming blocking candidates. Both regression tests pass:
`concept_same_real_entity_across_two_documents_merges` (genuine merge succeeds) and
`concept_generic_short_names_across_unrelated_documents_do_not_all_merge` (the devlog_27 shape does
not reoccur). A neighborhood-overlap structural signal — the real analogue of the column-overlap fix
that already disambiguates same-named SQL tables — is left as documented future work; it needs real
merge/non-merge examples from an actual corpus to calibrate correctly, not synthetic ones.

### Cost gating

Unlike every other pass in this connector, this one makes O(sections) LLM calls — potentially
thousands for a large corpus. Opt-in only (`[document-semantics] enabled = true`), with
`max_sections` as a blunt safety valve. Zero LLM calls happen unless a user explicitly turns it on.

---

## Decisions

- **Create new `Concept` objects rather than enrich existing ones** — `SqlAnalyzerPass` (the only
  prior LLM-extraction pass) only enriches properties on objects a structural pass already created,
  matched by table name. Free prose has no equivalent pre-existing name to match against, so this
  pass had to take a genuinely different shape: creation, not enrichment.
- **Per-(section, concept) deterministic ids, not per-concept-name** — necessary, not just
  consistent with the rest of the codebase: `Ledger::append_object` is versioned by
  `(id, content_signature)`, so a shared id across two real mentions would silently version-overwrite
  the first instead of giving the identity resolver two distinct objects to actually propose merging.
- **No new MCP tool** — extracted `Concept`s surface through `ekos_search`/`ekos_neighborhood`/
  `ekos_dependents`/`ekos ask`, all already generic over any `KirObject` kind. This was an explicit
  user decision, confirmed before design started, not a default assumption.
- **Threshold/name-length fix over neighborhood-overlap scoring for v1** — the more faithful fix
  needs real corpus data to calibrate well and is a larger, riskier change to `structural_score`'s
  signature; shipping the cheap fix now and refining later mirrors the exact arc RFC 0024's own
  devlog used for the Section bug.

---

## Testing

- `plugins/localdocs`: 56 tests, including per-parser unit tests for all three new formats and the
  cross-format `LocalDocAnalyzerPass` regression proving zero downstream change.
- `crates/recovery`: 62 tests, including the full `document_semantics_analyzer` suite (creation,
  bad-JSON tolerance, idempotency, unknown-concept-in-relationship handling) and `llm_json`'s tests.
- `crates/identity`: 26 tests, including both new Concept merge/non-merge regression tests.
- `crates/compiler-core`: 22 tests, including `DocumentSemanticsConfig` parsing/defaults.
- Full workspace: `cargo test` — 166 passed, 0 failed, across all five touched crates. `cargo clippy
  --workspace --all-targets` clean (all pre-existing warnings traced to files this session didn't
  touch, confirmed via `git diff`). `cargo fmt --check` clean. Zero `unsafe` introduced.

---

## Real-world rescan (same session, second pass)

Ran the full RFC 0026 end-to-end verification recipe for real: built the release binary, stood up
a scratch workspace with genuinely mixed real content (2 real PDFs from the same 82-book library
used in devlog 25–27, 2 real Markdown files, the repo's real LICENSE as `.txt`, real `cargo doc`
HTML output, and a realistic hand-written `.eml`), pointed `[llm] provider = "ollama"` at a local
`qwen2.5:1.5b`, and ran `build → recover → resolve → compile → commit → query` against it.

**Confirmed working end-to-end, not just in unit tests:** all 5 new RFC 0025 formats produced
independently searchable content (`docs/identity-crate-docs.html: section 1`,
`docs/rollout-note.eml: section 2`, etc.); the RFC 0026 pass extracted 206 concepts / 75
relationships from 36 sections against a real local model; genuine cross-mention merging worked
(7 separate "Machine Learning" references across one PDF resolved into one canonical `Concept`
with real page-citing evidence); no runaway over-merge (14 merge groups out of 278 candidates);
zero LLM calls occurred until the config gate was explicitly turned on.

**Two real, previously-undiscovered bugs surfaced**, neither introduced by this session's own
code — both in the pre-existing `DefaultResolver` blocking logic, only ever triggered by a mixed
real-format corpus sharing one folder:

- `normalize()` never strips path segments, and blocking groups on `(kind, first 3 normalized
  chars of name)`. Any files sharing a folder prefix (`docs/...` — an entirely ordinary layout)
  collide on that prefix regardless of actual content. Live repro: **7 unrelated `Document`
  objects** (two different PDFs, an RFC, a devlog, a license, an HTML page, an email) collapsed
  into one canonical object at confidence 0.90.
- The same collision hits PDF/DOCX-derived `Table` objects, which also lack a `columns` property
  and so get the same free structural-score floor RFC 0024 already diagnosed for `Section`. Live
  repro: **9 distinct tables from one real PDF** collapsed into one canonical `Table` at
  confidence 0.99 — the identical failure shape RFC 0024 fixed for `Section`, on a kind RFC 0024
  deliberately left untouched (SQL-derived tables need real cross-file name dedup, so blanket
  exclusion isn't the right fix here).

Added two regression tests reproducing both exactly with synthetic fixtures matching the real
run's object names/counts (`unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`,
`distinct_pdf_tables_in_one_document_do_not_all_merge`, `crates/identity/src/lib.rs`), marked
`#[ignore]` with a reason pointing at this entry so the default `cargo test` stays green while the
bug stays documented and reproducible on demand (`cargo test -- --ignored`).

A **third** bug surfaced but was not regression-tested this session (flagged for follow-up
instead of expanding scope further): `ekos ask` (`crates/cli/src/commands/ask.rs`) hardcodes
`AnthropicProvider` construction directly rather than calling `recover.rs`'s `build_llm_provider`
— so it fails with "No LLM provider configured" even when `config.llm.provider = "ollama"` is set
and `ekos recover` is correctly using it in the same workspace. RFC 0021 added Ollama support to
the recovery path; `ekos ask` was never updated to match.

---

## Knowledge Captured

- **`normalize()`'s blocking prefix operates on the full name, path segments included — any
  real project layout where multiple files/objects share a folder is a latent over-merge risk
  the single-format test corpora used through devlog 25–27 never exercised**, because those
  workspaces' object names didn't happen to share a 3-character-identical prefix. A mixed-format
  "docs/" folder — about as ordinary as real project structure gets — reliably triggers it.
  Worth checking before trusting `DefaultResolver`'s output on any workspace with multiple
  documents/tables in one directory, until the blocking key itself is fixed (basename instead of
  full relative path is the likely correct fix).
- **A resolver's "same-kind fallback" signal is a hidden landmine for any new high-cardinality
  object kind — and the fix isn't always "exclude it."** devlog_27 already flagged this once for
  Section. This session is the second time it mattered, and the fix had to be the *opposite* shape:
  Section needed blanket exclusion (no legitimate merge case exists), Concept needed a stricter
  threshold plus a length guard (legitimate merges are the entire point). Any future connector
  adding a new high-volume `Custom(...)` kind needs to reason about which of these two shapes its
  kind actually needs — copying the nearest precedent without checking can silently break the
  feature in either direction (defeats real merges, or repeats the over-merge bug).
- **`KnowledgeArtifact` ids are content-addressed, not derivable from the producing pass's name** —
  a pass that needs to read another pass's output (as opposed to reading raw `ObservationArtifact`s,
  which every existing analyzer pass does) has to scan the artifact store and filter on
  `content.pass_name`, not compute the id directly. This is the first pass in the codebase to read
  another pass's `KnowledgeArtifact` output rather than raw observations — worth knowing before
  assuming a `KnowledgeArtifact::new(&pass_id, ...)`-style shortcut exists.
- **Two Claude Code subagents given disjoint file sets and run in parallel finished real, correct
  work but never sent a final status report** — both went idle after their work bursts; live editor
  diagnostics (not agent messages) were what actually revealed transient compile errors mid-flight,
  and direct verification (`git status`, `cargo test`/`clippy`/`fmt`) — not the agents' self-report —
  is what confirmed completion. Don't trust an idle notification as a completion signal; verify the
  repo state directly.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0025-additional-document-formats.md` | New RFC |
| `docs/rfcs/0026-document-semantics-extraction.md` | New RFC |
| `plugins/localdocs/src/text.rs`, `html.rs`, `email.rs` | New parsers (RFC 0025) |
| `plugins/localdocs/src/lib.rs` | `supported_extensions()` default method, wiring, chunking helper |
| `plugins/localdocs/Cargo.toml` | `html2text`, `mail-parser` deps |
| `plugins/localdocs/tests/fixtures/` | New fixture files for text/Markdown/HTML/email |
| `crates/recovery/src/document_semantics_analyzer.rs` | New pass (RFC 0026) |
| `crates/recovery/src/llm_json.rs` | Shared JSON-fence-stripping helper |
| `crates/recovery/src/sql_analyzer.rs` | Uses the new shared `llm_json` helper |
| `crates/recovery/src/local_docs_analyzer.rs` | Cross-format regression test |
| `crates/recovery/src/lib.rs` | New module declarations |
| `crates/identity/src/lib.rs` | `ResolverConfig::kind_thresholds`, short-name blocking guard, two RFC 0026 regression tests, plus two `#[ignore]`d regression tests documenting the real folder-prefix over-merge bug found in the rescan |
| `crates/compiler-core/src/config.rs` | `DocumentSemanticsConfig` |
| `crates/cli/src/commands/recover.rs` | Pass registration + config gating |
| `README.md` | Connector list updated; new "Document semantic memory" section |
| `TODO.md` | RFC 0025/0026 entries added under Phase 14, checked off |

# RFC 0024 — Document Section Indexing

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-30
**Gating:** none (additive; extends RFC 0023's local document connector and RFC 0014's content
indexing, no schema change)

---

## Motivation

Live testing demonstrated that `ekos_search`/`ekos_ekl` cannot find content deep inside long
PDF/DOCX documents. `Cloud Design Patterns.pdf` mentions "replication" 36 times, including a whole
section titled "Data Replication and Synchronization Guidance" — but `ekos_search(query:
"replication")` returns zero matches.

Root cause, traced end to end:

1. `plugins/localdocs/src/lib.rs` caps captured text at `EXCERPT_MAX_CHARS = 600` chars — content
   past char 600 of the *whole document* is discarded before it reaches the artifact store.
2. `LocalDocAnalyzerPass` (`crates/recovery/src/local_docs_analyzer.rs`, RFC 0023) writes that one
   600-char excerpt onto a single `KirObject(Custom("Document"))` per file, however long the file
   is.
3. `KirObject::indexed_content()` (`crates/kir/src/lib.rs`) is the *only* thing either search
   backend (SQLite FTS5 — `crates/ledger/src/lib.rs`; tantivy — `crates/ledger/src/search.rs`, RFC
   0016) reads — it concatenates `properties["excerpt"]` + `properties["symbols"]`. Content past
   the excerpt cap simply does not exist anywhere the search backends can see.

RFC 0014 scoped the 600-char cap deliberately: "600 chars captures headings/preamble... without
bloating a 22K-object index with megabytes of code." RFC 0016 explicitly declined to lift it even
under tantivy. This RFC does not remove that cap globally — it argues the cap is being applied at
the wrong granularity for one specific object kind. RFC 0014's stated worry was about a single
monolithic file (e.g. a large generated source file) whose *whole* content could be megabytes. A
PDF/DOCX decomposes naturally into pages/paragraphs; each unit is small on its own. Instead of one
600-char excerpt for an entire 300-page book, this RFC emits one small-capped excerpt **per
page/section**, mirroring `crates/recovery/src/sql_analyzer.rs`'s existing precedent of emitting
one `KirObject(Table)` per `CREATE TABLE` statement rather than one object per SQL file.

A second, smaller gap surfaced during this investigation and is bundled here as a one-line fix:
`indexed_content()` never reads `properties["ocr_text"]`, so OCR'd text from scanned pages — RFC
0023's entire reason for existing — is not searchable via `ekos_search` either, only reachable if
the object id is already known through some other path.

**Naming note:** RFC 0016 already uses the word "segment" for something unrelated — an immutable,
content-addressed, ~8MB-sealed batch-commit file of EAV facts (a storage/commit unit, nothing to do
with document structure). To avoid confusion, this RFC uses **"Section"** for document
chapter/page/chunk units and never "segment."

## Design

### New type and constants (`plugins/localdocs/src/lib.rs`)

```rust
pub struct DocumentSection {
    pub page: Option<u32>,   // 1-indexed PDF page; None for DOCX (no page concept in docx-rs)
    pub index: usize,        // 0-indexed position among this document's sections
    pub text: String,
}
```

`ParsedDocument` gains `pub sections: Vec<DocumentSection>`.

Four new constants, alongside the existing `EXCERPT_MAX_CHARS`/`TABLES_MAX`/`TABLE_ROWS_MAX`:

- **`SECTIONS_MAX = 300`** — cap on sections captured per document. Justification: RFC 0023's
  devlog validation run against a real 82-book library produced 45 `Document` objects from 71
  successfully-parsed PDFs. Worst case, every one hitting this cap: 45 × 300 = 13,500 new `Section`
  objects added to a ~22K-object index (≈1.6× growth) — a bounded, real number, not the unbounded
  "megabytes of code" blowup RFC 0014 was actually worried about, because each section's *indexed*
  content is separately, tightly capped (next constant). Documents with more pages keep only the
  first 300 (documented truncation, logged as a warning — same honesty precedent RFC 0023 set for
  its PDF table heuristic).
- **`SECTION_TEXT_MAX_CHARS = 3000`** — cap on raw per-section text stored in the artifact
  (generous enough for a full technical-book page, ~2000–2500 chars typical).
- **`SECTION_EXCERPT_MAX_CHARS = 1200`** — cap on the searchable `excerpt` property written per
  `Section` KirObject. Larger than the whole-document `EXCERPT_MAX_CHARS` (600) because the scope
  here is one page, not an entire book — this is what actually closes the demonstrated failure:
  "Data Replication and Synchronization Guidance" only needs to fit in *its own page's* budget, not
  compete with 300 other pages for one shared 600-char window.
- **`DOCX_CHUNK_CHAR_BUDGET = 2500`** — DOCX has no page concept (pagination is a rendering-time
  detail docx-rs doesn't expose); paragraph text accumulates into a section until this budget is
  hit, then a new section starts. Chosen to approximate one PDF page's typical prose density.
  Explicitly approximate, `page` always `None`, documented as such.

### `PdfParser` (`plugins/localdocs/src/pdf.rs`)

Calls `pdf_extract::extract_text_from_mem_by_pages(bytes)` — a real per-page extraction API already
present in the pinned `pdf-extract` 0.12.0 dependency — alongside the existing
`extract_text_from_mem` call (kept unchanged, still used for the whole-document `text`/Document-level
excerpt). Builds one `DocumentSection` per returned page (1-indexed, matching the existing
`EmbeddedImage.page`/`ExtractedTable.page` convention already in this file), capped at
`SECTIONS_MAX`, text capped at `SECTION_TEXT_MAX_CHARS`.

**Verified caveat** (read from the vendored `pdf-extract` source):
`extract_text_from_mem_by_pages`'s internal loop is `while let Ok(content) =
extract_text_by_page(&doc, page_num)` — it stops at the *first* page that errors, silently dropping
every page after it, unlike `extract_text_from_mem`'s different, whole-document code path. If the
page count returned is less than what the existing `lopdf`-based page walk already computes, a
`tracing::warn!` notes the truncation and processing proceeds with whatever pages were returned —
still a strict improvement over today's single 600-char whole-document cap, but an honestly
documented limitation, not a silently-perfect one. Already covered by the `catch_unwind` wrapping
all of `parse_inner` (RFC 0023).

### `DocxParser` (`plugins/localdocs/src/docx.rs`)

While accumulating paragraph text into the existing whole-document `text` buffer, also accumulates
into a `current_section` buffer; flushes into a `DocumentSection { page: None, .. }` once
`DOCX_CHUNK_CHAR_BUDGET` is reached, flushing any remainder at the end. Capped at `SECTIONS_MAX`.

### `LocalDocsObserver::scan` (`plugins/localdocs/src/lib.rs`)

After the existing `tables_json` construction, builds `sections_json` the same way: every section's
text runs through the existing `sanitize_text()` (RFC 0023's prompt-injection hardening —
non-negotiable, every string reaching the ledger gets sanitized, same as excerpt/table-cells/OCR
text already do), truncated to `SECTION_TEXT_MAX_CHARS`, added to the artifact `data` as
`"sections"` (unconditional, empty array is a valid/normal value — same shape as `"tables"`).

### `LocalDocAnalyzerPass` (`crates/recovery/src/local_docs_analyzer.rs`)

Adds `SectionData { index, page, text }` (deserialize, mirroring `TableData`) and `#[serde(default)]
sections: Vec<SectionData>` on `DocumentData` — `#[serde(default)]` is essential so artifacts
written before this RFC, lacking the `sections` key entirely, still deserialize cleanly.

Adds `section_kir_id(path, index)` mirroring `table_kir_id`'s `Uuid::new_v5` scheme
(`"localdocs:{path}:section:{index}"`).

For each section: one `KirObject(Custom("Section"))` — **no `ObjectKind` enum change**, matching
the existing `Custom("Document")`/Confluence's `Custom("Page")` precedent — named `"{path}: page
{n}"` (PDF) or `"{path}: section {n}"` (DOCX, matching `Table`'s established `"{path}: table {n}"`
convention), properties `excerpt` (capped to `SECTION_EXCERPT_MAX_CHARS`), `page`, `section_index`;
evidence citing the page/section number; a `Contains` edge from the Document object. This alone is
what makes sections searchable — `indexed_content()` already reads `properties["excerpt"]` on
*any* object kind, so no search-core change is needed for section content itself.

### `crates/kir/src/lib.rs` — bundled fix

`indexed_content()` extended to also fold in `properties["ocr_text"]` when present, using the same
join pattern already used for excerpt/symbols. Additive only — no behavior change for objects
without `ocr_text`.

## Alternatives Considered

- **Removing `EXCERPT_MAX_CHARS` globally, for every connector** — rejected; reintroduces exactly
  the unbounded-per-object-size blowup RFC 0014 was built to prevent, for object kinds (source
  files, etc.) where that cap is still doing its job. This RFC's fix is specific to documents,
  which decompose naturally into small units; a monolithic file does not.
- **Per-image `KirObject`s carrying page-region text** — rejected, out of scope; no bounding-box or
  structural data exists to justify a standalone object, same reasoning RFC 0023 already applied to
  per-image objects generally.
- **LLM-based chapter/heading detection for smarter section boundaries** — rejected as a heavier
  mechanism than this bug fix warrants. Page-per-object (PDF) and fixed-budget chunking (DOCX) are
  large, easily-verified improvements over the status quo; a "smarter chunking" RFC — closer to how
  `book-to-skill` synthesizes chapter summaries — can follow later if page/chunk-level granularity
  proves too coarse in practice.
- **A dedicated `Section`/`Chunk` `ObjectKind` enum variant instead of `Custom("Section")`** —
  rejected for this RFC; `Custom(String)` is the established, zero-schema-change pattern this exact
  codebase already uses for `Document`/`Page`, and nothing about sections needs enum-level
  first-class treatment yet.

## Testing

- **`plugins/localdocs/src/lib.rs`**: sections capped at `SECTIONS_MAX`; section text sanitized
  (reusing RFC 0023's hidden-Unicode fixture pattern); section text truncated to
  `SECTION_TEXT_MAX_CHARS`; page numbers pass through unmodified; every existing `ParsedDocument`
  test literal updated for the new field.
- **`crates/recovery/src/local_docs_analyzer.rs`**: one `Section` object + `Contains` edge per
  section; a section's `indexed_content()` actually surfaces its excerpt text — the direct
  regression test for the demonstrated bug; zero sections → zero `Section` objects (backward
  compatibility with pre-RFC-0024 artifacts); idempotent section ids across reruns; a real-content
  test seeding a section whose text contains "replication" past char 600 of a synthetic combined
  document, asserting it is now findable via `indexed_content()`.
- **`crates/kir/src/lib.rs`**: `indexed_content()` includes `ocr_text` when present; concatenates
  excerpt + symbols + ocr_text when all three are set.
- End-to-end: rebuild the real 82-book scratch ledger from devlog 25/26, confirm
  `ekos_search(query: "replication")` now returns a `Section` hit for `Cloud Design Patterns.pdf`,
  and spot-check a term known only from OCR'd text to confirm the bundled `indexed_content()` fix.

**Second bug found during end-to-end verification, fixed as part of this RFC** (see devlog 27):
the real rerun initially collapsed 8,624 raw Document/Table/Section objects down to 120 after
identity resolution — `crates/identity`'s `DefaultResolver` was merging nearly every page of a book
into one canonical `Section`, because pages of the same document share a long name prefix
(`"{path}: page "`) that scores high on Jaro-Winkler, and `structural_score`'s same-kind fallback of
`1.0` (no `columns` property to compare) added a flat +0.3 floor on top. Fixed by excluding
`Custom("Section")` objects from resolution blocking entirely in
`crates/identity/src/lib.rs::DefaultResolver::resolve` — each Section is already deterministically
identified by (document, page/index), so no two distinct Section objects can legitimately represent
the same real-world entity; there is no correct case for merging them. After the fix, the same rerun
produced 8,187 final objects / 8,225 relationships (vs. the pre-fix 120), and
`ekos_search(query: "replication")` returned 30 real matches, including `Cloud Design Patterns.pdf`
pages 211, 212, 214, 215, and 216 — its actual "Data Replication and Synchronization Guidance"
section.

## Acceptance Criteria

- [x] `Section` KirObjects are emitted with `Contains` edges from their parent `Document`, each with
      evidence.
- [x] `ekos_search(query: "replication")` returns a match against the real book library where it
      previously returned none. (Verified: 30 matches, including `Cloud Design Patterns.pdf` pages
      211–216.)
- [x] `indexed_content()` includes `ocr_text`, verified by a passing unit test.
- [x] All new/updated unit tests pass; `cargo clippy --workspace --all-targets` and `cargo fmt
      --check` clean; zero `unsafe` introduced; no `ObjectKind` schema change.
- [x] Real observed index growth from the end-to-end rerun recorded in the devlog against the
      projected 45×300 bound.

# Devlog 24 — RFC 0023: Local document connector (PDF/DOCX, text + tables + image OCR)

**Date:** 2026-07-30
**PRs:** worked on `main` (single session)
**Branch:** main

---

## Summary

Adds `ekos-plugin-localdocs`, EKOS's first connector that reads *inside* binary document
formats. `FileObserver` has always hashed/excerpted every file, but treated PDF/DOCX as opaque
blobs (they fail the UTF-8 excerpt path). The new connector parses PDF and DOCX files found
under the observed workspace paths — extracting prose text, tables, and OCR'd text from embedded
images — and a new `LocalDocAnalyzerPass` promotes that into the ledger as `Document` and `Table`
KIR objects. Verified end-to-end against a real generated PDF (table + JPEG scan) and DOCX (table
+ PNG image): both the OCR'd image text and the extracted table cell content are confirmed present
and searchable in a committed ledger.

The key design decision: OCR shells out to the `tesseract` CLI as a subprocess rather than linking
an FFI binding, keeping the crate `unsafe`-free per CLAUDE.md's coding rules, at the cost of a
soft runtime dependency (missing binary → soft-skip, not a hard failure).

---

## RFC 0023 — Local Document Connector

### What was built

| Component | File | Detail |
|---|---|---|
| `DocumentParser`/`OcrEngine` traits | `plugins/localdocs/src/lib.rs` | Client-trait shape every connector uses, so tests never depend on `tesseract` or the parsing crates' runtime behavior |
| `PdfParser` | `plugins/localdocs/src/pdf.rs` | `pdf-extract` for text, `lopdf` for structure/embedded-image (JPEG/`DCTDecode` only) extraction, whitespace-column heuristic for tables |
| `DocxParser` | `plugins/localdocs/src/docx.rs` | `docx-rs` for paragraphs/tables (structurally explicit, no heuristic needed); images read directly from the `.docx` zip's `word/media/` entries via the `zip` crate, independent of `docx-rs`'s document-model traversal |
| `TesseractOcr`, `MockOcr` | `plugins/localdocs/src/ocr.rs` | Real engine shells out to `tesseract <path> stdout`; missing binary is a soft-skip (`OcrError::Unavailable`), not a hard failure |
| `LocalDocsObserver` | `plugins/localdocs/src/lib.rs` | Walks the tree like `FileObserver` but filtered to `.pdf`/`.docx`; connector name `"localdocs"` (distinct from `"file"`, so the two never collide in the artifact index) |
| `LocalDocAnalyzerPass` | `recovery/src/local_docs_analyzer.rs` | One `KirObject(Custom("Document"))` per file, one `KirObject(Table)` per extracted table + `Contains` edge, no LLM in the loop |
| `docs/rfcs/0023-local-document-connector.md` | new | Full RFC written first |
| `build.rs`/`recover.rs` wiring | `cli/src/commands/{build,recover}.rs` | Unconditional (no credential to gate on — it's local files), alongside `FileObserver`/`GitObserver` |

### Implementation details worth remembering

- **PDF table extraction is a text-heuristic, not glyph-position clustering.** `pdf-extract`
  returns a flat text stream, not per-glyph coordinates, so `extract_tables` groups contiguous
  lines that each split into ≥2 fields on runs of ≥2 whitespace characters. This worked for a
  `reportlab`-generated PDF's DOCX-equivalent layout in ad hoc testing but **did not** detect the
  table in the end-to-end test's `reportlab`-rendered `Table` flowable — the ledger ended up with
  only 1 `Table` object (from the DOCX) out of 2 documents processed, confirming the RFC's
  documented limitation is real, not theoretical. A future RFC wanting reliable PDF table
  extraction will need actual glyph-position data (`lopdf`'s content-stream operators), not this
  heuristic.
- **PDF image extraction only decodes `DCTDecode` (JPEG) streams.** Other PDF image filters
  (raw, `CCITTFaxDecode`, etc.) are skipped with a debug log rather than writing a full PDF image
  decoder — same "honest scoping" every connector in this codebase uses for what it doesn't cover.
- **DOCX images are read from the raw zip, not through `docx-rs`'s object model.** A `.docx` file
  is itself a zip archive; `word/media/*` entries are read directly via the `zip` crate. This
  sidesteps needing `docx-rs` to expose media relationships through its (primarily write-oriented)
  API.
- **`zip` crate's default version on crates.io is a prerelease** (`9.0.0-pre2` at time of writing).
  Pinned explicitly to `zip = "2"` (stable series) instead.
- **End-to-end verification path**: no CLI command dumps ledger content as readable text (`ekos
  query find` only searches object *names*, and the SQLite ledger's FTS tables use a newer
  `contentless_delete` FTS5 option the system `sqlite3` CLI doesn't support). Verified instead with
  `strings .ekos/ledger/ledger.db | grep <expected text>` — confirmed OCR'd text and table cell
  content are both present in the committed ledger payloads.

### Decisions (alternatives considered, why this choice)

- **FFI OCR bindings (`tesseract-rs`/`leptess`)** — rejected: would need a formally-justified
  `unsafe` carve-out per CLAUDE.md, plus a heavier native build dependency. Subprocess `tesseract`
  keeps zero `unsafe` at the cost of a runtime-only dependency, which soft-skips cleanly.
- **EPUB/FB2 in v1** — rejected for this RFC (PDF/DOCX cover most enterprise document estates);
  `DocumentParser` is deliberately format-agnostic so a follow-up connector is additive.
- **True diagram/vector-shape extraction** — rejected as out of scope; no mature pure-Rust
  vector-diagram parser exists. v1 treats a diagram exactly like any other embedded image (OCR the
  text on it, nothing more).
- **Per-image `KirObject`s** — rejected for v1; OCR'd text rides on the parent document object's
  `ocr_text` property instead, since there's no per-image structure yet to justify a standalone
  object.

---

## Knowledge Captured

- `pdf-extract::extract_text_from_mem` and `lopdf::Document::load_mem` both parse the same PDF
  bytes independently (no shared AST) — two separate parses of the same file, an accepted
  trade-off for keeping text extraction and structural/image extraction each on the library best
  suited for it, rather than building one from the other's output.
- `lopdf::Document::get_page_resources(page_id)` returns
  `Result<(Option<&Dictionary>, Vec<(u32,u16)>), Error>` directly — not a tuple-returning method
  needing a further `.0` field access on a `Result`. Destructure the whole `Ok((Some(resources),
  _))` pattern in one match/let-else.
- `docx-rs`'s builder API (`Docx::new().add_paragraph(...).add_table(...)`) round-trips correctly
  through `read_docx` — verified by building a docx in-memory with the crate's own writer and
  reading it back in a unit test, avoiding the need for hand-crafted or vendored binary DOCX
  fixtures.
- System `sqlite3` CLI on this host is too old for the FTS5 options EKOS's bundled `rusqlite`
  writes (`contentless_delete`) — `sqlite3 .ekos/ledger/ledger.db "select ... from object_fts ..."`
  fails with "unrecognized option", even though the app itself reads/writes the same file fine.
  `strings <db file> | grep <text>` is a reliable fallback for spot-checking ledger content
  without needing the exact FTS5 version.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0023-local-document-connector.md` | New RFC |
| `ekos/plugins/localdocs/**` | New crate: observer, PDF/DOCX parsers, OCR engines, tests |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | New `LocalDocAnalyzerPass` + tests |
| `ekos/crates/recovery/src/lib.rs` | Export `LocalDocAnalyzerPass` |
| `ekos/crates/cli/src/commands/build.rs` | Wire `LocalDocsObserver` in unconditionally |
| `ekos/crates/cli/src/commands/recover.rs` | Wire `LocalDocAnalyzerPass` + artifact collection + summary line |
| `ekos/crates/cli/Cargo.toml` | Add `ekos-plugin-localdocs` dependency |
| `ekos/Cargo.toml` | Add `plugins/localdocs` workspace member |

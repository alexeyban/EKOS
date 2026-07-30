# Devlog 25 — Real-world validation: local document connector vs. an 82-book library

**Date:** 2026-07-30
**PRs:** worked on `main` (single session, follow-up to devlog 24 / RFC 0023)
**Branch:** main

---

## Summary

Ran `ekos-plugin-localdocs` (RFC 0023, shipped in devlog 24) end-to-end against a real 955MB,
82-PDF personal library (`/home/legion/Documents/Books/Data-Science-Library`, outside the repo —
not committed, read-only). This was the connector's first exposure to real-world documents rather
than synthetic `reportlab`/`docx-rs`-generated fixtures, and it found three real bugs the synthetic
test suite couldn't have caught, all now fixed with regression tests built from the actual book
content that exposed them.

The enrichment itself worked: `ekos build → recover → resolve → compile → commit` produced a
ledger with 45 `Document` objects and 30 `Table` objects from 71 successfully-parsed PDFs, 18 of
which carry real OCR'd text pulled from scanned/image-only cover pages that would otherwise be
invisible to the ledger. The demo ledger lives in a scratch workspace
(`/tmp/.../scratchpad/ekos-books-v2`), not committed — the book PDFs themselves are copyrighted and
were never copied into the repo; only short fair-use excerpts of extracted text went into test
fixtures.

---

## Bugs found and fixed

### 1. `pdf-extract`/`lopdf` panic on malformed real-world PDFs — crashed the whole `ekos build`

**Symptom:** `pdf_extract::extract_text_from_mem` panics (doesn't return `Err`) on PDFs with
malformed font definitions — a Type3 font missing a glyph width, "missing unicode map and
encoding," an unexpected `Reference` where a `Dictionary` was expected, an out-of-bounds index.
6 of 82 real books hit one of these. Since `PdfParser::parse` only wrapped `Result`-returning
calls, a panic inside `pdf-extract` propagated straight through `LocalDocsObserver::scan` and took
down the entire `ekos build` process — every other file's work lost, not just the one bad PDF.

**Fix:** `PdfParser::parse` now wraps the actual parsing logic (moved to a `parse_inner` associated
function) in `std::panic::catch_unwind`, converting a caught panic into an ordinary
`ParseError::Malformed` — the same soft-skip path any other parse failure already took. `&[u8]` is
automatically `UnwindSafe` (shared reference to `Sync`, `RefUnwindSafe` data), so no
`AssertUnwindSafe` wrapper was needed.

**Why the synthetic test suite missed this:** every `reportlab`-generated fixture PDF has clean,
well-formed font tables — there was no reason for a hand-built fixture to exercise a malformed-font
code path. This is a class of bug that specifically needs real, "in the wild" documents.

### 2. Table-cell text with internal single spaces got silently mangled

**Symptom:** `split_table_row`'s original implementation only pushed a whitespace character into
the current field when a run of ≥2 was later found *not* to reach the delimiter threshold — but the
logic never actually appended single-space runs to the buffer at all. `"Putting it all together"`
came out as `"Puttingitalltogether"`. Every synthetic test fixture used single-word cells ("Name",
"Value", "a", "1"), so this never surfaced until a real book's multi-word table-of-contents entries
hit it.

**Fix:** rewrote `split_table_row` to always append whitespace characters to the buffer, then, on
hitting the ≥2 run threshold, slice off exactly the trailing delimiter whitespace before finalizing
the field — so single spaces inside a field's text survive, only genuine ≥2-space column gaps get
treated as delimiters.

### 3. Justified body prose routinely misdetected as a "table"

**Symptom (not a crash — a precision/quality issue):** PDF text extraction leaves irregular,
variable-width multi-space gaps in justified paragraphs (to fill the line to the margin). The
original heuristic ("≥2 spaces = column boundary, ≥2 consecutive matching lines = a table") had no
defense against this — an entire paragraph of prose from a real book ("Big Data and Business
Analytics comes of age.pdf") came through as a single bogus 18-field "table" spanning unrelated
sentences.

**Fix:** added a uniform-column-count requirement — a candidate table's rows are only kept if every
row has the *same* field count as the first. Justified prose's field count varies line to line (the
words per line differ); real tables, even simple ones like a two-column table of contents, keep a
constant column count. This eliminated the worst false positives (full-paragraph "tables") without
losing legitimate uniform 2–3 column content (TOC entries, numbered Q&A lists). It does **not**
eliminate every false positive — some numbered lists and title-wrapped-across-two-lines patterns
still pass the uniform-count check and get recorded as spurious 1-row-pair "tables." This residual
imprecision is now explicitly documented in RFC 0023 rather than claimed away.

---

## Real-world enrichment results

| Metric | Value |
|---|---|
| PDFs in library | 82 |
| PDFs that panicked `pdf-extract`/`lopdf` (soft-skipped, build did not crash) | 6 |
| PDFs successfully parsed and analysed | 71 |
| `Document` KIR objects after identity resolution | 45 |
| `Table` KIR objects after identity resolution | 30 |
| Documents carrying real OCR'd image text | 18 of 45 |
| `ekos build` wall time (955MB, 82 PDFs + git + file observers) | ~2m30s |

Identity resolution merged 71 raw documents down to 45 and ~513 candidate tables down to 30 —
substantially more aggressive deduplication than the earlier synthetic-fixture testing suggested.
This is `ekos-identity`'s existing `DefaultResolver` behavior (out of scope of RFC 0023 to change),
but worth flagging: it appears to over-merge generically-named/generically-shaped `Table` objects
(e.g. many different books' short 2-column TOC-style tables can look similar enough to trigger a
merge). Not investigated further this session — noted as a follow-up if the localdocs connector's
table output is used at scale.

---

## Test coverage added from real content

Per the session's explicit ask ("make tests using information from books"), three new tests use
literal short excerpts of real `pdf-extract`/`tesseract` output captured during this run (not
hand-crafted synthetic text), committed as string literals — never the copyrighted PDF files
themselves:

- `plugins/localdocs/src/pdf.rs`: `real_justified_prose_produces_no_table` (regression test for bug
  #3), `real_toc_fragment_is_still_detected_as_a_table` (a real 2-column TOC excerpt still
  produces a correct `Table`), `has_uniform_column_count_rejects_mismatched_rows`.
- `plugins/localdocs/src/lib.rs`: `real_book_excerpt_and_ocr_text_ride_on_the_artifact_unmodified`
  — real statistics-course excerpt + real OCR'd book-cover text, verifying both ride through the
  observer unmodified.
- `crates/recovery/src/local_docs_analyzer.rs`: `real_book_table_content_produces_matching_table_object`
  — the real MLOps white paper TOC table, verifying it becomes a correctly-shaped `Table` KirObject.

---

## Knowledge Captured

- **`pdf-extract` 0.12.0 panics rather than returning `Err` on a non-trivial fraction of real-world
  PDFs** (6/82 ≈ 7% in this sample) — malformed Type3 font widths, missing Unicode maps, unexpected
  object types. Any code calling `pdf_extract::extract_text_from_mem` on untrusted/arbitrary PDFs
  must wrap the call in `catch_unwind`; treating it as an ordinary `Result`-returning API is not
  safe.
- **A heuristic tested only against clean synthetic fixtures will look correct and still be
  substantially wrong on real input.** Both the single-space bug and the justified-prose
  false-positive were invisible across the entire original test suite (all passing) because every
  fixture was hand-built with single-word cells and short paragraphs. Real-world validation against
  actual documents is not optional polish for a heuristic-based extractor — it is where the actual
  bugs live.
- `sqlite3`-CLI content inspection continues to need the zstd-frame unwrap
  (`payload[1:]` skips the 1-byte dict-version tag before the zstd frame, per devlog 24) — used
  Python + the `zstandard` package this session to decode and inspect real committed ledger content
  by object kind, which is how the post-fix table quality was actually verified rather than assumed.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0023-local-document-connector.md` | Updated table-heuristic description: uniform-column-count refinement, real-world validation note |
| `ekos/plugins/localdocs/src/pdf.rs` | `catch_unwind` around PDF parsing (bug #1); rewrote `split_table_row` to preserve internal single spaces (bug #2); added `has_uniform_column_count` (bug #3); new tests from real book content |
| `ekos/plugins/localdocs/src/lib.rs` | New test using real excerpt + real OCR text |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | New test using real book table content |

# Devlog 27 — RFC 0024: document section indexing, and a real identity-resolution bug it exposed

**Date:** 2026-07-30
**PRs:** worked on `main` (single session, follow-up to devlog 24–26 — RFC 0023/0024)
**Branch:** main

---

## Summary

Fixed a real, demonstrated bug: `ekos_search`/`ekos_ekl` could not find content deep inside long
PDF/DOCX documents. `Cloud Design Patterns.pdf` mentions "replication" 36 times, including a whole
section titled "Data Replication and Synchronization Guidance" — but `ekos_search(query:
"replication")` returned zero matches, because `ekos-plugin-localdocs` only ever captured a
600-character excerpt of a document's *entire* text, and `KirObject::indexed_content()` (the only
thing either search backend reads) only sees `properties["excerpt"]`.

RFC 0024 fixes this by decomposing each document into `Section` objects — one per PDF page (via
`pdf-extract`'s real per-page API) or DOCX character-budget chunk — each carrying its own small,
independently-indexed excerpt. This alone would have been enough, but end-to-end verification
against the real 82-book library surfaced a second, more serious bug: identity resolution was
merging nearly every page of a book into one canonical object, silently defeating the whole fix.
Both are fixed, tested, and verified against the real library in this session.

---

## RFC 0024 — Document Section Indexing

### What was built

| Component | File | Detail |
|---|---|---|
| `DocumentSection` type + 4 new constants | `plugins/localdocs/src/lib.rs` | `SECTIONS_MAX=300`, `SECTION_TEXT_MAX_CHARS=3000`, `DOCX_CHUNK_CHAR_BUDGET=2500`; `SECTION_EXCERPT_MAX_CHARS=1200` lives in `crates/recovery` |
| Per-page PDF sectioning | `plugins/localdocs/src/pdf.rs` | `extract_sections()` via `pdf_extract::extract_text_from_mem_by_pages` — real per-page text, not a heuristic |
| Fixed-budget DOCX chunking | `plugins/localdocs/src/docx.rs` | Paragraph accumulation into ~2500-char sections, `page: None` (no page concept in the document model) |
| `Section` KirObjects + `Contains` edges | `crates/recovery/src/local_docs_analyzer.rs` | `Custom("Section")` — zero `ObjectKind` schema change, same pattern as `Custom("Document")` |
| Bundled fix: `ocr_text` now searchable | `crates/kir/src/lib.rs` | `indexed_content()` previously read only `excerpt`+`symbols`; OCR'd scanned-page text (RFC 0023's whole point) was never actually findable via `ekos_search` |
| `docs/rfcs/0024-document-section-indexing.md` | new | Full RFC, written first |

### The second bug: identity resolution ate almost everything

First end-to-end rerun: `local-docs-analyzer` produced 8,624 raw objects (Documents + Tables +
Sections). After `ekos resolve`/`ekos compile`, only **120** survived. `ekos_search(query:
"replication")` still returned nothing — the fix looked broken.

Root cause, in `crates/identity/src/lib.rs::DefaultResolver`:
- Blocking groups objects by `(kind, first 3 chars of normalized name)`. Every page of the same book
  shares the same block (same kind `"Section"`, same first-3-chars from the shared `"{path}: page "`
  prefix).
- `DefaultResolver::score` computes `combined = 0.7 * jaro_winkler(name) + 0.3 * structural_score`.
- `structural_score` falls back to **`1.0`** for any two same-kind objects that lack a `columns`
  property (a fallback designed for low-cardinality kinds like SQL tables, where "same kind" is
  already a decent signal) — Section objects don't have `columns`, so every pair gets a free +0.3
  floor.
- Jaro-Winkler on two names sharing a long common prefix (`"Cloud Design Patterns.pdf: page "`,
  differing only in the trailing number) scores high even for genuinely unrelated pages (page 1 vs.
  page 213). Combined with the +0.3 floor, nearly every page of a book cleared the 0.85 merge
  threshold and got Union-Find-merged into one canonical object per book.

This is architecturally guaranteed to happen for any high-cardinality, per-document-instance kind —
Table objects have the same fallback exposure and devlog 25 already documented milder symptoms of it
(592 candidate tables → 33 after resolution), but Section objects' sheer volume and name-prefix
structure made it catastrophic instead of just lossy.

**Fix**: `Custom("Section")` objects are now excluded from resolution blocking entirely
(`crates/identity/src/lib.rs`). Justification: each Section is already deterministically identified
by `(document path, page/index)` via `Uuid::new_v5` — the same page of the same document always
produces the same id on every rebuild, and two *different* Section ids can never legitimately
represent the same real-world entity. There is no correct scenario where merging them helps;
excluding them can only prevent false-positive merges, never cause a missed true merge. `Table`'s
resolution behavior was deliberately left untouched — `ObjectKind::Table` is also used by
SQL-derived objects (`sql_analyzer.rs`) where fuzzy-name deduplication across different SQL files
naming the same table ("Customer" vs. "customer") is an intentional, tested feature; blanket-
excluding `Table` would have broken that.

After the fix: same rerun produced **8,187 final objects / 8,225 relationships** (vs. 120 before),
and `ekos_search(query: "replication")` returned **30 real matches**, including `Cloud Design
Patterns.pdf` pages 211, 212, 214, 215, and 216 — its actual "Data Replication and Synchronization
Guidance" section. Pairwise comparison cost also dropped enormously as a side effect: 1,598,766
pairs compared before the fix (Sections were the overwhelming majority of blocked candidates) → just
25,811 after excluding them from blocking.

---

## Decisions

- **Page-per-object (PDF) / fixed-budget-chunk-per-object (DOCX) instead of LLM-based chapter
  detection** — rejected the heavier mechanism (closer to how `book-to-skill` synthesizes chapter
  summaries) as more than this bug fix warranted. Page/chunk granularity is a large, cheaply-verified
  improvement over the status quo; a "smarter chunking" RFC can follow later if coarser granularity
  proves insufficient in practice.
- **`Custom("Section")` instead of a new `ObjectKind` enum variant** — zero schema change, matches
  the codebase's existing `Custom("Document")`/`Custom("Page")` precedent.
- **Excluding Section from resolution entirely, not tuning the threshold or adding a Section-specific
  structural signal** — the simplest fix that's also provably correct for this kind specifically
  (see justification above), versus threshold-tuning, which would have been a global, harder-to-reason-about
  change risking regressions on the kinds the resolver's existing tests protect.
- **`extract_text_from_mem_by_pages`'s early-stop behavior** (verified in the vendored `pdf-extract`
  0.12.0 source: the internal loop stops at the first page that errors, silently dropping every page
  after it) is treated as a documented limitation, not fixed — still a strict improvement over the
  old whole-document 600-char cap even when truncated, and consistent with this connector's existing
  honest-scoping precedent (RFC 0023's table heuristic, panic soft-skips).

---

## Testing

- `plugins/localdocs`: sections capped/sanitized/truncated (unit), plus a genuine round-trip test —
  a 2-page PDF built with `lopdf`'s own writer, parsed by the real `PdfParser`, asserting one Section
  per page with correct page numbers and text (not a byte-string fixture).
- `crates/recovery`: Section KirObjects + `Contains` edges; the direct regression test —
  `section_excerpt_is_searchable_via_indexed_content` — asserts a section's `indexed_content()`
  contains "replication" while the *document's own* excerpt does not, proving the fix is real and
  not incidental; idempotency; zero-sections backward compatibility.
- `crates/kir`: `indexed_content()` includes `ocr_text`.
- `crates/identity`: `section_objects_are_never_merged_even_with_near_identical_names` (the
  regression test for the second bug) and `other_custom_kinds_still_resolve_normally` (pins the fix
  to the literal string `"Section"`, not `Custom` in general).
- Full workspace: `cargo test --workspace` (all green), `cargo clippy --workspace --all-targets`
  (zero new warnings), `cargo fmt --check` clean.
- End-to-end: real 82-book library, `build → recover → resolve → compile → commit`, verified via a
  live `ekos mcp serve` session (same JSON-RPC-over-stdio approach as devlog 25/26).

---

## Knowledge Captured

- **A resolver's "same-kind fallback" signal is a hidden landmine for any new high-cardinality
  object kind.** `structural_score`'s `1.0` fallback was correct and safe when every `ObjectKind`
  in the system was low-cardinality (one `Table` per SQL statement, one `Document` per file).
  Introducing a kind with hundreds of same-document instances and structurally similar names (page
  numbers) turned a reasonable default into a near-total data-loss bug. Any future connector adding a
  new high-volume `Custom(...)` kind should check this exclusion pattern first, or explicitly
  reason about whether the default resolver's blocking/scoring is safe for it.
- **`pdf_extract::extract_text_from_mem_by_pages` (0.12.0) stops at the first page-extraction
  error and silently drops the rest** — confirmed by reading the vendored source
  (`while let Ok(content) = extract_text_by_page(...)`), not documented in the crate's public API
  docs. Anyone relying on this function for a page count should cross-check against an independent
  page count (this connector uses `lopdf`'s page walk, already computed for other reasons) and log
  when they disagree.
- **`ekos commit` on ~16K objects can exceed a short timeout** (observed: initial attempt killed at
  60s; a resumed run completed the remainder in ~38s) — append-only + id-skip-if-already-present
  made the resumed run safe and correct, but this is worth knowing when scripting against larger
  workspaces.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0024-document-section-indexing.md` | New RFC, updated post-verification with real numbers and the identity-resolution bug/fix |
| `ekos/plugins/localdocs/src/lib.rs` | `DocumentSection` type, new constants, sections wired into `scan()` |
| `ekos/plugins/localdocs/src/pdf.rs` | Real per-page sectioning via `pdf-extract`'s by-pages API |
| `ekos/plugins/localdocs/src/docx.rs` | Fixed-budget paragraph chunking into sections |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | `Section` KirObjects + `Contains` edges |
| `ekos/crates/kir/src/lib.rs` | `indexed_content()` now includes `ocr_text` |
| `ekos/crates/identity/src/lib.rs` | `Custom("Section")` excluded from resolution blocking — the second bug's fix |

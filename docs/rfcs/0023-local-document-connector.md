# RFC 0023 — Local Document Connector (PDF / DOCX, text + tables + image OCR)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-07-30
**Gating:** none (additive; follows RFC 0006's Observer contract and RFC
0017/0020/0022's Observer + recovery-pass integration shape)

---

## Motivation

`plugins/file`'s `FileObserver` is EKOS's only local-filesystem observer. It
hashes every file and, for files it can decode as UTF-8, captures a 600-char
excerpt (RFC 0014) and harvested declaration symbols (RFC 0019). Binary
document formats — PDF, Word (`.docx`) — fail the UTF-8 decode and are
recorded as an opaque blob: path, size, hash, nothing else.

Enterprise knowledge lives heavily in these formats: design specs,
contracts, architecture docs, reports, meeting notes exported to PDF. They
routinely contain three kinds of content the ledger currently cannot see at
all:

1. **Prose** — readable text, currently invisible because it's behind a
   binary encoding rather than plain UTF-8.
2. **Tables** — structured rows/columns (a schema definition, a comparison
   matrix, a pricing table) rendered as PDF/DOCX layout rather than as
   `CREATE TABLE` DDL or a CSV `FileObserver` would already parse.
3. **Embedded images** — screenshots, scanned pages, diagrams — which may be
   the *only* copy of some information (a scanned contract page, a
   whiteboard photo pasted into a spec).

This RFC adds a connector that extracts all three into the ledger, using the
same Observer + recovery-pass shape as every prior connector — no new
architectural mechanism, just a new source.

## Design

### `ekos-plugin-localdocs` — Observer

Unlike the API-backed connectors (Salesforce, GitHub, Confluence), this
source is local files, so it walks the filesystem the same way
`FileObserver` does — reusing `ctx.workspace_root`, `ctx.ignore_patterns`,
and `ctx.is_ignored` — but filters to `.pdf` and `.docx` extensions only.
It runs **alongside** `FileObserver`, not instead of it: `FileObserver`
still hashes/records every file (including these) under connector name
`"file"`; `LocalDocsObserver` adds a second, richer artifact under connector
name `"localdocs"`. Different connector names mean different artifact-index
keys (`build.rs` keys artifacts by `"{connector}/{target}"`), so the two
never collide.

Parsing is abstracted behind traits, mirroring every other connector's
`XClient` real/mock split — so unit tests never depend on an external
`tesseract` binary or the exact parsing crate's runtime behavior:

```rust
pub trait DocumentParser: Send + Sync {
    /// Lowercase extension this parser handles, e.g. "pdf".
    fn supported_extension(&self) -> &str;
    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError>;
}

pub struct ParsedDocument {
    pub page_count: Option<u32>,
    /// Full extracted prose, in reading order.
    pub text: String,
    pub tables: Vec<ExtractedTable>,
    pub images: Vec<EmbeddedImage>,
}

pub struct ExtractedTable {
    pub page: Option<u32>,
    pub rows: Vec<Vec<String>>,
}

pub struct EmbeddedImage {
    pub page: Option<u32>,
    pub bytes: Vec<u8>,
    pub format: ImageFormat, // Png | Jpeg
}

pub trait OcrEngine: Send + Sync {
    fn recognize(&self, image: &EmbeddedImage) -> Result<String, OcrError>;
}
```

- **`PdfParser`** — `lopdf` (document structure, page/XObject walk) +
  `pdf-extract` (text layer extraction). Table extraction is a v1
  whitespace-column heuristic operating on the flat text `pdf-extract`
  returns (not per-page glyph coordinates): contiguous lines that each
  split into ≥2 fields on runs of ≥2 whitespace characters are grouped,
  then kept only if every row in the group has the *same* field count
  (rows = lines, cells = fields). The uniform-column-count check was added
  after running against a real 82-book library (see devlog): without it,
  justified body prose routinely misfired as a "table," since PDF text
  extraction leaves irregular multi-space gaps in justified paragraphs but,
  unlike a real table, the field count varies wildly line to line — even
  with the check, this remains approximate and can still misfire on
  multi-column prose that happens to align uniformly, or merge/split cells
  on unusual layouts — documented as such, not a layout-ML or
  glyph-position result. Embedded-image extraction walks each page's
  `/Resources /XObject` dictionary for `Subtype /Image` streams; v1 only
  decodes images whose PDF filter is `DCTDecode` (JPEG) — the common case
  for scanned pages — and skips other encodings (e.g. raw/`CCITTFaxDecode`)
  with a debug log rather than implementing a full PDF image-decoder.
- **`DocxParser`** — `docx-rs` for document structure: paragraphs → `text`,
  and `<w:tbl>` tables as exact rows/cells (no heuristic needed — DOCX
  tables are structurally explicit, unlike PDF). Embedded images are read
  directly from the `.docx` zip archive's `word/media/` entries (a `.docx`
  is itself a zip; the `zip` crate reads it independent of `docx-rs`'s
  document-model traversal), classified by file extension (`.png`/`.jpg`/
  `.jpeg`).
- **`TesseractOcr`** — the real `OcrEngine`. Writes the image to a temp file
  (`tempfile` crate) and shells out to `tesseract <path> stdout` via
  `std::process::Command`. This avoids both `unsafe` FFI bindings and a new
  native build-time dependency (linking libtesseract into every build);
  the cost is requiring the `tesseract` binary on `PATH` at *run* time only.
  A missing binary (`io::ErrorKind::NotFound` from `Command::spawn`) is a
  **soft-skip**: log a warning, omit that image's OCR text, and keep
  processing — the document's non-image text and tables are still fully
  recorded. This is the same soft-skip philosophy every credential-gated
  connector (`crypto`, `github`, `confluence`) already uses for missing
  configuration.
- **`MockOcr`** — fixed in-memory image-bytes → text map. The actual test
  bar for OCR-dependent behavior, same convention as every `MockXClient`.

`LocalDocsObserver::scan` emits one `ObservationArtifact` per document,
target = workspace-relative path, `data`:

| field | meaning |
|---|---|
| `path` | relative path (same as `FileObserver`) |
| `size_bytes`, `content_sha256` | same convention as `FileObserver` |
| `doc_format` | `"pdf"` \| `"docx"` |
| `page_count` | from `ParsedDocument.page_count` |
| `excerpt` | first 600 chars of `ParsedDocument.text` (same cap as RFC 0014) |
| `tables` | JSON array of `{page, rows}`, capped at 20 tables / 200 rows each |
| `ocr_text` | concatenated per-image OCR text, capped at 2000 chars |
| `image_count` | total embedded images found |
| `ocr_image_count` | images actually OCR'd (lets a diagnostic report e.g. "tesseract unavailable, 4 images skipped") |

No live-network dependency here (it's all local files), so — unlike the
credential-gated API connectors — there's no env-var gate: `LocalDocsObserver`
is wired into `build.rs`'s `observers` vec unconditionally, next to
`FileObserver`/`GitObserver`.

### `LocalDocAnalyzerPass` — recovery pass

Same shape as `ConfluenceAnalyzerPass`/`GitHubAnalyzerPass`: one
`CompilerPass`, pure structural mapping, no LLM in the loop. Consumes every
`ObservationArtifact` where `connector_name == "localdocs"`.

- **One `KirObject` per document**: `ObjectKind::Custom("Document")` (same
  pattern Confluence used for `Custom("Page")`), named `"{path}"`,
  deterministic id via `Uuid::new_v5(NAMESPACE_URL, "localdocs:{path}")`
  (stable across reruns, same scheme every connector uses). Properties:
  `path`, `doc_format`, `page_count`, `excerpt`, `artifact_id`, `ocr_text`
  (when non-empty) — the `excerpt`-as-searchable-fact convention RFC 0014
  established, extended to OCR'd text so `ekos_search` can find a document
  by content that only exists inside a scanned image. Evidence citing the
  file path (`SourceLocation::file`).
- **One child `KirObject` per extracted table**: `ObjectKind::Table`
  (the existing kind — a PDF/DOCX table is exactly as much a `Table` as a
  SQL one, just without a schema), property `rows` (JSON array of arrays),
  deterministic id via `Uuid::new_v5` on `"localdocs:{path}:table:{index}"`.
  A `RelationshipKind::Contains` edge from the document object to each table
  object, evidence citing the document path + page number — makes table
  content independently searchable via `ekos_search`, the same value
  Confluence's page-hierarchy `Contains` edges provide.
- **No per-image objects in v1** — OCR'd text rides on the parent document
  object's `ocr_text` property rather than spawning a `KirObject` per image;
  v1 has no per-image structure (no bounding box, no image classification)
  that would justify treating an image as its own addressable object. Named
  as a natural follow-up once/if diagram-structure extraction exists (see
  Alternatives Considered).
- No `References`/cross-document edges in v1 — unlike GitHub's closing
  keywords or Confluence's `content-title` links, PDF/DOCX have no
  comparably structural "this document references that document" markup to
  scan for without either OCR-quality filename matching (fragile) or an LLM
  pass (a different, larger mechanism, consistent with how RFC 0020
  originally deferred Confluence for the same reason). Left for future work.

## Alternatives Considered

- **FFI OCR bindings (`tesseract-rs`/`leptess`)** — rejected: requires
  `unsafe`, which CLAUDE.md permits only with a formal RFC justification,
  plus a heavier build (linking libtesseract, needing `libtesseract-dev`
  everywhere the workspace builds). Shelling out to the `tesseract` CLI
  keeps zero `unsafe` and zero new build-time native dependency, at the
  cost of requiring the binary on `PATH` at run time — accepted, since it
  soft-skips cleanly when absent.
- **EPUB/FB2 support in v1** — rejected for this RFC; PDF and DOCX cover
  the large majority of enterprise document estates (specs, contracts,
  reports). EPUB/FB2 (e-book formats) are lower priority for enterprise
  knowledge. A follow-up connector can reuse the same `DocumentParser`
  trait and `LocalDocAnalyzerPass` machinery — this RFC's design
  deliberately keeps `DocumentParser` format-agnostic so that addition is
  additive, not a rewrite.
- **True diagram/shape extraction (vector graphics → structured
  nodes/edges)** — rejected as out of scope; no mature pure-Rust
  vector-diagram parser exists, and building one is a project of its own.
  V1 treats a diagram exactly like any other embedded image: OCR the text
  on it, nothing more. A diagram whose value is purely structural (shapes,
  arrows, no legible text) is not recoverable by this RFC.
- **Per-image `KirObject`s** — rejected for v1 (see Design, above):
  insufficient per-image structure to justify a standalone object yet.
- **PDF layout-ML table detection (e.g. a trained model) instead of a
  position-heuristic** — rejected; no such dependency exists in the
  workspace today, and pulling one in is a much larger scope increase than
  this RFC's goal (prove the connector pattern extends to binary document
  formats). The heuristic's known failure modes are documented above.

## Testing

- `MockOcr`/fixture-bytes-driven observer tests (small PDF/DOCX fixtures
  checked into `plugins/localdocs/tests/fixtures/`, generated once and
  committed rather than built at test time): one artifact per document;
  table extraction produces the expected `rows` on a fixture with a known
  table; `ocr_text` present only when the fixture has embedded images and
  `MockOcr` returns non-empty text for them; same input produces the same
  artifact id (idempotency); a `DocumentParser`/`OcrEngine` double that
  reports "binary not found" exercises the soft-skip path (document still
  produces an artifact, `ocr_image_count` is 0, no panic/hard error).
- `LocalDocAnalyzerPass` tests (mirroring `confluence_analyzer.rs`'s style):
  one `Document` object per artifact; one `Table` child object plus a
  `Contains` edge per extracted table, each with evidence; the same
  document across two passes resolves to the same object id (idempotent
  re-run); a document with zero tables produces zero `Table` objects.
- No live-OCR integration test requiring an actual `tesseract` install —
  same honest scoping every other connector in this codebase uses for
  external dependencies it can't exercise in CI.

## Acceptance Criteria

- [ ] `LocalDocsObserver` + `PdfParser`/`DocxParser`/`MockOcr` pass the
      fixture-driven test suite.
- [ ] `LocalDocAnalyzerPass` emits one `Document` object per artifact and
      `Contains` edges to `Table` child objects, each with evidence.
- [ ] Missing `tesseract` binary soft-skips OCR without failing the scan.
- [ ] Wired into `build.rs` (unconditional, alongside `FileObserver`) and
      `recover.rs` (artifact collection + pass registration), following the
      established connector pattern.
- [ ] `cargo clippy --workspace` and `cargo fmt --check` clean; zero
      `unsafe` introduced.

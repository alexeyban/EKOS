# RFC 0025 — Additional Document Formats (Text/Markdown, HTML, Email)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-03
**Gating:** none (additive; follows RFC 0023's `DocumentParser` contract, which was designed
format-agnostic for exactly this)

---

## Motivation

`plugins/localdocs` (RFC 0023/0024) parses PDF and DOCX into `Document`/`Table`/`Section` KIR
objects with page/chunk-level search coverage. Everything else — plain text, Markdown, HTML,
email — still falls through `FileObserver`'s generic path: a single 600-char excerpt (RFC 0014)
regardless of the file's real length, and no section-level search coverage at all. These formats
carry the same kind of enterprise prose RFC 0023 targeted (notes, exported docs, web-saved specs,
email threads) but get a worse search experience than a PDF does today, purely because
`LocalDocsObserver` only registers two `DocumentParser` implementations.

RFC 0023 designed `DocumentParser` deliberately format-agnostic, naming EPUB/FB2 as an example of
a future additive extension. This RFC is that extension, for text/Markdown, HTML, and email
instead.

## Design

### New parsers (`plugins/localdocs/src/`)

Each new file mirrors `pdf.rs`/`docx.rs`'s shape: a real parser implementing `DocumentParser`,
fixture-driven `#[cfg(test)]` tests, no live network dependency.

**`text.rs` — `TextParser` (`.txt`, `.md`)**

```rust
pub struct TextParser {
    extension: &'static str,
}

impl TextParser {
    pub fn new(extension: &'static str) -> Self {
        Self { extension }
    }
}
```

`parse` decodes with `String::from_utf8_lossy` (never fails — no `ParseError::Malformed` case for
encoding), then chunks into `DocumentSection`s by a new `TEXT_CHUNK_CHAR_BUDGET` constant
(matching `DOCX_CHUNK_CHAR_BUDGET`'s existing pattern and value, 2500 chars), `page: None` for
every section (no page concept, same as DOCX), `page_count: None`, no tables, no images.
Markdown is treated as plain text — no `pulldown-cmark` AST parse. See Alternatives Considered.

**`html.rs` — `HtmlParser` (`.html`, `.htm`)**

```rust
pub struct HtmlParser {
    extension: &'static str,
}
```

Uses **`html2text`** (pure Rust, zero `unsafe`, no new native build dependency) to render the
document to block-structure-preserving plain text via `html2text::from_read`, then chunks the
result exactly like `TextParser` (same `TEXT_CHUNK_CHAR_BUDGET`). `<table>` elements are not
parsed into `ExtractedTable`s in v1 — `html2text` flattens tables into text, which is an accepted
lossy simplification (see Alternatives Considered); table rows still appear as searchable prose
inside a Section, just not as a structured `Table` object.

**`email.rs` — `EmailParser` (`.eml`)**

```rust
pub struct EmailParser;
```

Uses **`mail-parser`** (pure Rust, zero `unsafe`, handles MIME multipart) to parse the raw
message. Builds:
- A header block (`Subject`, `From`, `To`, `Date`) as the first `DocumentSection`.
- Body sections chunked from the message body: prefer the `text/plain` MIME part; if absent, fall
  back to the `text/html` part converted via a shared `html_to_text` helper factored out of
  `html.rs` (so the HTML-to-text logic is written once, used by both `HtmlParser` and
  `EmailParser`).
- No attachment parsing (attachment bytes are not decoded or searched in v1).
- No `.msg` support — Outlook's binary CFB/OLE2 container format has no mature pure-Rust parser in
  the workspace's dependency spirit; explicit non-goal, same honest-scoping precedent RFC 0023
  used for EPUB/FB2.

Every new parser's extracted text runs through the existing `sanitize_text` (RFC 0023's
prompt-injection hardening — zero-width/Unicode-tag-block stripping) at the same call sites
`pdf.rs`/`docx.rs` already use before anything reaches an `ObservationArtifact`.

### `DocumentParser` trait extension

`DocumentParser::supported_extension(&self) -> &str` returns one extension per parser instance,
but `.txt`/`.md` and `.htm`/`.html` need two extensions routed to the same parser logic. Rather
than change the trait's core method (which would touch `PdfParser`/`DocxParser` for no reason) or
register duplicate parser structs, add a default method:

```rust
pub trait DocumentParser: Send + Sync {
    fn supported_extension(&self) -> &str;

    /// Additional extensions this parser also handles, beyond `supported_extension()`.
    /// Default: none — only `PdfParser`/`DocxParser`-style single-extension parsers need not
    /// override this.
    fn supported_extensions(&self) -> Vec<&str> {
        vec![self.supported_extension()]
    }
}
```

Backward compatible — `PdfParser`/`DocxParser` need no change. `TextParser`/`HtmlParser` override
`supported_extensions()` to return both extensions they were constructed to accept, or (simpler,
chosen for this RFC) each gets constructed twice with a fixed extension
(`TextParser::new("txt")`, `TextParser::new("md")`, `HtmlParser::new("html")`,
`HtmlParser::new("htm")`) and `supported_extensions()` stays at its one-extension default — this
keeps `LocalDocsObserver`'s extension→parser lookup a simple one-to-one map, no ambiguity, lowest
implementation risk. `EmailParser` needs only `.eml`, so its `supported_extension()` returns
`"eml"` directly with no override.

### Wiring

`LocalDocsObserver::with_defaults` (`plugins/localdocs/src/lib.rs`) grows from
`vec![Arc::new(PdfParser), Arc::new(DocxParser)]` to also register
`Arc::new(TextParser::new("txt"))`, `Arc::new(TextParser::new("md"))`,
`Arc::new(HtmlParser::new("html"))`, `Arc::new(HtmlParser::new("htm"))`,
`Arc::new(EmailParser)`. No other call site changes — `crates/cli/src/commands/build.rs`'s
`LocalDocsObserver::with_defaults(...)` call needs nothing further, since it already registers
whatever `with_defaults` returns.

### Downstream: zero changes required

`LocalDocAnalyzerPass` (`crates/recovery/src/local_docs_analyzer.rs`) reads `doc_format` as an
opaque string off the artifact's JSON `data` and never branches on its value — `"txt"`, `"md"`,
`"html"`, `"htm"`, `"eml"` flow through exactly like `"pdf"`/`"docx"` do today, producing
`Document`/`Section` KIR objects with `Contains` edges and evidence. This RFC's implementation
must prove this claim with a test (see Testing), not just assert it.

## Alternatives Considered

- **Markdown-aware chunking (`pulldown-cmark`, splitting on headings)** — rejected for v1;
  fixed-budget chunking is consistent with how DOCX (which also has no page concept) already
  works, and heading-aware chunking is a real scope increase (parsing structure, deciding chunk
  boundaries around nested lists/code blocks) for a benefit not yet demonstrated to matter. Can
  follow as a dedicated RFC if page/chunk-level granularity proves too coarse for Markdown
  specifically, the same "ship simple, refine later" arc RFC 0024 used for PDF/DOCX sections.
- **`scraper` instead of `html2text` for HTML parsing** — rejected; `scraper` (built on
  `html5ever` + `selectors`) is aimed at CSS-selector-driven structured extraction, heavier than
  needed when the goal is just "readable prose out of an HTML file." `html2text` is a smaller,
  more directly-fit dependency.
- **Structured `<table>` extraction for HTML, mirroring DOCX's exact-cell extraction** — rejected
  for v1; `html2text` doesn't preserve table structure, and hand-rolling an HTML table parser is
  a scope increase this RFC doesn't need to take on to close the gap (table content is still
  captured as prose, just not as a queryable `Table` object). Left as future work if a real HTML
  document with tables demonstrates the loss matters.
- **Outlook `.msg` support** — rejected for this RFC; binary CFB/OLE2 format, no mature
  pure-Rust parser exists in the workspace's dependency spirit. `.eml` (the interoperable,
  text-based email format most mail clients can export to) is the pragmatic v1 scope.
- **Email attachment parsing** — rejected for v1; attachments are a distinct sub-problem
  (potentially any of the formats this connector already/will support, recursively) better left
  to a focused follow-up once plain `.eml` body/header extraction is proven useful.

## Testing

- Fixture files under `plugins/localdocs/tests/fixtures/`: a `.txt` sample, a `.md` sample with
  headings, a small `.html` sample with nested tags (including a `<table>`, to confirm the
  documented lossy-flattening behavior), a small anonymized `.eml` fixture with a multipart
  (`text/plain` + `text/html`) body.
- Per-parser unit tests (`text.rs`/`html.rs`/`email.rs`, inline `#[cfg(test)]` mirroring `pdf.rs`):
  correct extension routing via `LocalDocsObserver`'s parser lookup; section chunking respects
  `TEXT_CHUNK_CHAR_BUDGET`; `sanitize_text` is applied (reusing RFC 0023's hidden-Unicode fixture
  pattern); malformed/truncated input degrades to `ParseError` rather than panicking (for HTML/
  email — text parsing cannot fail, per `from_utf8_lossy`); `EmailParser` produces a header
  section plus body section(s) and correctly falls back to the HTML body when no `text/plain`
  part exists.
- `LocalDocsObserver::scan` test: a mixed-format fixture directory produces one artifact per file
  regardless of extension.
- `LocalDocAnalyzerPass` test: seed an artifact with `doc_format: "eml"` (or any new format) and
  confirm it produces `Document`+`Section` objects identically to the existing PDF-format test —
  the direct regression proof that "nothing else changes" downstream.
- `ekos_search` integration-style test: a term that only appears past character 600 of a Markdown
  or HTML fixture is findable via `indexed_content()`, the same demonstrated-bug-fix shape RFC
  0024 used for PDF sections.

## Acceptance Criteria

- [ ] `TextParser`, `HtmlParser`, `EmailParser` implement `DocumentParser`, registered in
      `LocalDocsObserver::with_defaults` for `.txt`/`.md`/`.html`/`.htm`/`.eml`.
- [ ] `LocalDocAnalyzerPass` requires zero code changes to handle the new formats; proven by test.
- [ ] `ekos_search` finds prose from a Markdown/HTML/eml fixture that would previously have been
      truncated or invisible under `FileObserver`'s generic 600-char excerpt.
- [ ] `.msg` and email attachments are explicitly out of scope, documented as future work, not
      attempted.
- [ ] All new/updated unit tests pass; `cargo clippy --workspace --all-targets` and `cargo fmt
      --check` clean; zero `unsafe` introduced.
- [ ] New dependencies (`html2text`, `mail-parser`) added to `plugins/localdocs/Cargo.toml` only —
      no workspace-wide dependency bump.

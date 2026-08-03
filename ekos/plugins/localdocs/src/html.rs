//! HTML parsing (RFC 0025): `html2text` renders the document to
//! block-structure-preserving plain text, which is then chunked exactly
//! like plain text. `<table>` elements are rendered into the text stream
//! (as box-drawn rows) rather than extracted as `ExtractedTable`s — an
//! accepted v1 simplification documented in the RFC's Alternatives
//! Considered: cell content stays searchable, it just isn't a queryable
//! `Table` object.

use crate::{DocumentParser, ParseError, ParsedDocument, TEXT_CHUNK_CHAR_BUDGET, text::chunk_text};

/// Render width handed to `html2text`. Deliberately far wider than any
/// display width: `html2text` hard-wraps at this column, and a wrap
/// inserted mid-sentence splits phrases across lines, so a search for
/// "escalate to the on-call engineer" would miss a document that contains
/// exactly that. Chunking into sections is `chunk_text`'s job, not the
/// renderer's.
const RENDER_WIDTH: usize = 100_000;

pub struct HtmlParser {
    extension: &'static str,
}

impl HtmlParser {
    pub fn new(extension: &'static str) -> Self {
        Self { extension }
    }
}

impl DocumentParser for HtmlParser {
    fn supported_extension(&self) -> &str {
        self.extension
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let text = html_to_text(bytes)?;
        let sections = chunk_text(&text, TEXT_CHUNK_CHAR_BUDGET);
        Ok(ParsedDocument {
            page_count: None,
            text,
            tables: Vec::new(),
            images: Vec::new(),
            sections,
        })
    }
}

/// Shared HTML-to-plain-text conversion, used by `HtmlParser` and by
/// `EmailParser`'s `text/html` fallback body.
pub(crate) fn html_to_text(bytes: &[u8]) -> Result<String, ParseError> {
    html2text::from_read(bytes, RENDER_WIDTH)
        .map_err(|e| ParseError::Malformed(format!("html render failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/runbook.html");

    #[test]
    fn extension_is_whatever_the_parser_was_constructed_with() {
        assert_eq!(HtmlParser::new("html").supported_extension(), "html");
        assert_eq!(HtmlParser::new("htm").supported_extension(), "htm");
    }

    #[test]
    fn nested_tags_are_flattened_into_prose_without_markup() {
        let parsed = HtmlParser::new("html").parse(HTML_FIXTURE).unwrap();
        assert!(parsed.text.contains("Nightly Batch Runbook"));
        assert!(parsed.text.contains("escalate to the on-call engineer"));
        assert!(!parsed.text.contains("<p>"));
        assert!(!parsed.text.contains("<div"));
        assert_eq!(parsed.page_count, None);
        assert!(parsed.images.is_empty());
    }

    /// Documents the accepted v1 lossiness: table *content* survives as
    /// searchable prose, but no `ExtractedTable` is produced.
    #[test]
    fn table_content_survives_as_prose_but_no_structured_table_is_extracted() {
        let parsed = HtmlParser::new("html").parse(HTML_FIXTURE).unwrap();
        assert!(parsed.tables.is_empty());
        assert!(parsed.text.contains("ingest_orders"));
        assert!(parsed.text.contains("02:15"));
    }

    #[test]
    fn sections_have_no_page_and_respect_the_budget() {
        let parsed = HtmlParser::new("html").parse(HTML_FIXTURE).unwrap();
        assert!(!parsed.sections.is_empty());
        for s in &parsed.sections {
            assert_eq!(s.page, None);
            assert!(s.text.chars().count() <= TEXT_CHUNK_CHAR_BUDGET);
        }
    }

    #[test]
    fn truncated_markup_degrades_gracefully_rather_than_panicking() {
        // html5ever's error recovery makes almost any byte soup parseable;
        // the contract this pins down is "no panic, and either Ok or a
        // ParseError" — never an abort mid-`ekos build`.
        for input in [
            &b"<html><body><p>unclosed"[..],
            &b"<<<>>> not really markup"[..],
            &[0xff, 0xfe, 0x00, 0x01][..],
            &b""[..],
        ] {
            let result = HtmlParser::new("html").parse(input);
            assert!(result.is_ok() || matches!(result, Err(ParseError::Malformed(_))));
        }
    }
}

//! Plain-text and Markdown parsing (RFC 0025). Markdown is treated as
//! plain text — no AST parse; see the RFC's Alternatives Considered.
//!
//! Also home to `chunk_text`, the fixed-budget chunker shared with
//! `HtmlParser` and `EmailParser`.

use crate::{
    DocumentParser, DocumentSection, ParseError, ParsedDocument, SECTION_TEXT_MAX_CHARS,
    SECTIONS_MAX, TEXT_CHUNK_CHAR_BUDGET,
};

pub struct TextParser {
    extension: &'static str,
}

impl TextParser {
    pub fn new(extension: &'static str) -> Self {
        Self { extension }
    }
}

impl DocumentParser for TextParser {
    fn supported_extension(&self) -> &str {
        self.extension
    }

    /// Decoding uses `from_utf8_lossy`, so this never returns `Err` —
    /// invalid bytes become replacement characters rather than a parse
    /// failure that would silently drop the whole file.
    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let text = String::from_utf8_lossy(bytes).into_owned();
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

/// Splits text into `DocumentSection`s of at most `budget` characters,
/// breaking on line boundaries where possible. A single line longer than
/// the budget is split mid-line rather than being allowed to exceed it.
///
/// `page` is `None` for every section: none of the formats using this have
/// a page concept, same as DOCX.
pub(crate) fn chunk_text(text: &str, budget: usize) -> Vec<DocumentSection> {
    let mut sections: Vec<DocumentSection> = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;

    let flush = |current: &mut String, current_chars: &mut usize, sections: &mut Vec<_>| {
        if !current.trim().is_empty() && sections.len() < SECTIONS_MAX {
            sections.push(DocumentSection {
                page: None,
                index: sections.len(),
                text: std::mem::take(current)
                    .chars()
                    .take(SECTION_TEXT_MAX_CHARS)
                    .collect(),
            });
        }
        current.clear();
        *current_chars = 0;
    };

    for line in text.lines() {
        for piece in split_to_budget(line, budget) {
            let piece_chars = piece.chars().count();
            if current_chars + piece_chars + 1 > budget && !current.is_empty() {
                flush(&mut current, &mut current_chars, &mut sections);
            }
            if !current.is_empty() {
                current.push('\n');
                current_chars += 1;
            }
            current.push_str(piece);
            current_chars += piece_chars;
        }
    }
    flush(&mut current, &mut current_chars, &mut sections);

    sections
}

/// Splits one line into `budget`-character pieces. Returns the line
/// untouched when it already fits, which is the overwhelmingly common case.
fn split_to_budget(line: &str, budget: usize) -> Vec<&str> {
    if line.chars().count() <= budget {
        return vec![line];
    }
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut count = 0usize;
    for (offset, _) in line.char_indices() {
        if count == budget {
            pieces.push(&line[start..offset]);
            start = offset;
            count = 0;
        }
        count += 1;
    }
    pieces.push(&line[start..]);
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKDOWN_FIXTURE: &str = include_str!("../tests/fixtures/notes.md");

    #[test]
    fn extension_is_whatever_the_parser_was_constructed_with() {
        assert_eq!(TextParser::new("txt").supported_extension(), "txt");
        assert_eq!(TextParser::new("md").supported_extension(), "md");
    }

    #[test]
    fn supported_extensions_defaults_to_the_single_extension() {
        assert_eq!(TextParser::new("md").supported_extensions(), vec!["md"]);
    }

    #[test]
    fn plain_text_round_trips_into_text_and_one_section() {
        let parsed = TextParser::new("txt")
            .parse(b"hello world\nsecond line")
            .unwrap();
        assert_eq!(parsed.text, "hello world\nsecond line");
        assert_eq!(parsed.sections.len(), 1);
        assert_eq!(parsed.sections[0].page, None);
        assert_eq!(parsed.sections[0].index, 0);
        assert!(parsed.tables.is_empty());
        assert!(parsed.images.is_empty());
        assert_eq!(parsed.page_count, None);
    }

    #[test]
    fn invalid_utf8_degrades_to_replacement_chars_rather_than_failing() {
        let parsed = TextParser::new("txt")
            .parse(&[b'o', b'k', 0xff, 0xfe])
            .unwrap();
        assert!(parsed.text.starts_with("ok"));
        assert!(parsed.text.contains('\u{FFFD}'));
    }

    #[test]
    fn markdown_headings_are_kept_as_literal_text() {
        let parsed = TextParser::new("md")
            .parse(MARKDOWN_FIXTURE.as_bytes())
            .unwrap();
        assert!(parsed.text.contains("# Retention Policy Notes"));
        assert!(!parsed.sections.is_empty());
    }

    #[test]
    fn chunking_respects_the_budget_and_indexes_sequentially() {
        let text = (0..200)
            .map(|i| format!("line {i} of the document"))
            .collect::<Vec<_>>()
            .join("\n");
        let sections = chunk_text(&text, TEXT_CHUNK_CHAR_BUDGET);
        assert!(
            sections.len() > 1,
            "long text must produce several sections"
        );
        for (i, s) in sections.iter().enumerate() {
            assert_eq!(s.index, i);
            assert_eq!(s.page, None);
            assert!(s.text.chars().count() <= TEXT_CHUNK_CHAR_BUDGET);
        }
    }

    #[test]
    fn a_single_line_longer_than_the_budget_is_split_not_overflowed() {
        let text = "x".repeat(TEXT_CHUNK_CHAR_BUDGET * 2 + 10);
        let sections = chunk_text(&text, TEXT_CHUNK_CHAR_BUDGET);
        assert_eq!(sections.len(), 3);
        assert!(
            sections
                .iter()
                .all(|s| s.text.chars().count() <= TEXT_CHUNK_CHAR_BUDGET)
        );
    }

    #[test]
    fn multibyte_text_splits_on_char_boundaries() {
        let text = "é".repeat(TEXT_CHUNK_CHAR_BUDGET + 5);
        let sections = chunk_text(&text, TEXT_CHUNK_CHAR_BUDGET);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[1].text.chars().count(), 5);
    }

    #[test]
    fn sections_are_capped_at_sections_max() {
        let line = "y".repeat(TEXT_CHUNK_CHAR_BUDGET);
        let text = vec![line; SECTIONS_MAX + 20].join("\n");
        let sections = chunk_text(&text, TEXT_CHUNK_CHAR_BUDGET);
        assert_eq!(sections.len(), SECTIONS_MAX);
    }

    #[test]
    fn whitespace_only_input_produces_no_sections() {
        assert!(chunk_text("   \n\n  \n", TEXT_CHUNK_CHAR_BUDGET).is_empty());
    }
}

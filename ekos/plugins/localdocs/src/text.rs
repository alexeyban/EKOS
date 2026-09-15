//! Plain-text and Markdown parsing (RFC 0025). Markdown gets no AST parse (see the
//! RFC's Alternatives Considered), but since RFC 0144 it is split per ATX heading by
//! `chunk_markdown` rather than by a blind character budget.
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
        let sections = if self.extension.eq_ignore_ascii_case("md") {
            chunk_markdown(&text, TEXT_CHUNK_CHAR_BUDGET)
        } else {
            chunk_text(&text, TEXT_CHUNK_CHAR_BUDGET)
        };
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
                ..Default::default()
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

/// RFC 0144: splits Markdown into one section per ATX heading (`#`…`######`),
/// each recording its heading, the stack of enclosing headings, and its real
/// 1-indexed line range. A heading's body longer than `budget` is sub-chunked
/// on line boundaries, every part inheriting the same heading path. Lines inside
/// a fenced code block are never treated as headings. Still no Markdown AST —
/// both rules are line-local (RFC 0025's Alternatives Considered).
pub(crate) fn chunk_markdown(text: &str, budget: usize) -> Vec<DocumentSection> {
    let mut sections: Vec<DocumentSection> = Vec::new();
    // (level, heading text) — the currently-open heading stack.
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut block: Vec<(u32, &str)> = Vec::new();
    let mut fence: Option<&str> = None;

    for (i, line) in text.lines().enumerate() {
        let line_no = i as u32 + 1;
        let trimmed = line.trim_start();
        if let Some(marker) = fence_marker(trimmed) {
            match fence {
                Some(open) if marker == open => fence = None,
                None => fence = Some(marker),
                _ => {}
            }
        } else if fence.is_none()
            && let Some((level, heading)) = atx_heading(line)
        {
            flush_markdown_block(&mut block, &stack, budget, &mut sections);
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            stack.push((level, heading));
        }
        block.push((line_no, line));
    }
    flush_markdown_block(&mut block, &stack, budget, &mut sections);
    sections
}

/// `Some("```")`/`Some("~~~")` when `trimmed` opens or closes a fenced code block.
fn fence_marker(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("```") {
        Some("```")
    } else if trimmed.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

/// `Some((level, text))` for an ATX heading line: up to 3 leading spaces, 1–6 `#`,
/// then a space (or nothing). Trailing closing `#`s are stripped.
fn atx_heading(line: &str) -> Option<(usize, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let level = rest.len() - rest.trim_start_matches('#').len();
    if level == 0 || level > 6 {
        return None;
    }
    let after = &rest[level..];
    if !after.is_empty() && !after.starts_with(' ') && !after.starts_with('\t') {
        return None;
    }
    let heading = after.trim().trim_end_matches('#').trim_end().to_string();
    if heading.is_empty() {
        return None;
    }
    Some((level, heading))
}

/// Emits `block` as one or more sections, each at most `budget` characters, tagged
/// with the current heading stack. Clears `block`.
fn flush_markdown_block(
    block: &mut Vec<(u32, &str)>,
    stack: &[(usize, String)],
    budget: usize,
    sections: &mut Vec<DocumentSection>,
) {
    let heading_path: Vec<String> = stack.iter().map(|(_, h)| h.clone()).collect();
    let heading = heading_path.last().cloned();

    let mut current = String::new();
    let mut current_chars = 0usize;
    let mut range: Option<(u32, u32)> = None;
    let emit = |current: &mut String,
                current_chars: &mut usize,
                range: &mut Option<(u32, u32)>,
                sections: &mut Vec<DocumentSection>| {
        if !current.trim().is_empty() && sections.len() < SECTIONS_MAX {
            let (start, end) = range.unwrap_or_default();
            sections.push(DocumentSection {
                page: None,
                index: sections.len(),
                text: std::mem::take(current)
                    .chars()
                    .take(SECTION_TEXT_MAX_CHARS)
                    .collect(),
                heading: heading.clone(),
                heading_path: heading_path.clone(),
                line_start: Some(start),
                line_end: Some(end),
            });
        }
        current.clear();
        *current_chars = 0;
        *range = None;
    };

    for (line_no, line) in block.iter().copied() {
        for piece in split_to_budget(line, budget) {
            let piece_chars = piece.chars().count();
            if current_chars + piece_chars + 1 > budget && !current.is_empty() {
                emit(&mut current, &mut current_chars, &mut range, sections);
            }
            if !current.is_empty() {
                current.push('\n');
                current_chars += 1;
            }
            current.push_str(piece);
            current_chars += piece_chars;
            range = Some(match range {
                Some((start, _)) => (start, line_no),
                None => (line_no, line_no),
            });
        }
    }
    emit(&mut current, &mut current_chars, &mut range, sections);
    block.clear();
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

    // ── RFC 0144: heading-aware Markdown sections ───────────────────────

    #[test]
    fn markdown_splits_on_headings_with_nested_heading_paths_and_line_ranges() {
        let md = "# RFC 0016 — Fact segments\n\n**Status:** Accepted\n\n## Motivation\n\nDepends on RFC 0015.\n\n### Detail\n\nDeep text.\n\n## Design\n\nThe design.\n";
        let sections = chunk_markdown(md, TEXT_CHUNK_CHAR_BUDGET);
        let summary: Vec<(Option<&str>, Vec<&str>, u32, u32)> = sections
            .iter()
            .map(|s| {
                (
                    s.heading.as_deref(),
                    s.heading_path.iter().map(String::as_str).collect(),
                    s.line_start.unwrap(),
                    s.line_end.unwrap(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    Some("RFC 0016 — Fact segments"),
                    vec!["RFC 0016 — Fact segments"],
                    1,
                    4
                ),
                (
                    Some("Motivation"),
                    vec!["RFC 0016 — Fact segments", "Motivation"],
                    5,
                    8
                ),
                (
                    Some("Detail"),
                    vec!["RFC 0016 — Fact segments", "Motivation", "Detail"],
                    9,
                    12
                ),
                (
                    Some("Design"),
                    vec!["RFC 0016 — Fact segments", "Design"],
                    13,
                    15
                ),
            ]
        );
        assert!(sections[1].text.starts_with("## Motivation"));
        assert!(sections[1].text.contains("Depends on RFC 0015."));
        for (i, s) in sections.iter().enumerate() {
            assert_eq!(s.index, i);
        }
    }

    #[test]
    fn a_hash_inside_a_code_fence_is_not_a_heading() {
        let md = "## Usage\n\n```bash\n# install first\ncargo build\n```\n\n~~~\n# also not a heading\n~~~\nafter\n";
        let sections = chunk_markdown(md, TEXT_CHUNK_CHAR_BUDGET);
        assert_eq!(sections.len(), 1, "{sections:?}");
        assert_eq!(sections[0].heading.as_deref(), Some("Usage"));
        assert!(sections[0].text.contains("# install first"));
    }

    #[test]
    fn text_before_the_first_heading_is_a_heading_less_section() {
        let sections = chunk_markdown("preamble\n\n# Title\nbody\n", TEXT_CHUNK_CHAR_BUDGET);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, None);
        assert!(sections[0].heading_path.is_empty());
        assert_eq!(sections[1].heading.as_deref(), Some("Title"));
    }

    #[test]
    fn hashtags_and_closing_hashes_are_handled() {
        assert_eq!(atx_heading("#hashtag"), None);
        assert_eq!(atx_heading("####### seven"), None);
        assert_eq!(atx_heading("    # indented code"), None);
        assert_eq!(atx_heading("## Closed ##"), Some((2, "Closed".to_string())));
        assert_eq!(atx_heading("#"), None);
    }

    #[test]
    fn a_long_heading_body_is_sub_chunked_and_every_part_keeps_its_heading() {
        let body = (0..400)
            .map(|i| format!("line {i} of a long motivation section"))
            .collect::<Vec<_>>()
            .join("\n");
        let md = format!("# Doc\n## Motivation\n{body}\n");
        let sections = chunk_markdown(&md, TEXT_CHUNK_CHAR_BUDGET);
        let motivation: Vec<_> = sections
            .iter()
            .filter(|s| s.heading.as_deref() == Some("Motivation"))
            .collect();
        assert!(motivation.len() > 1);
        for pair in motivation.windows(2) {
            assert_eq!(pair[0].line_end.unwrap() + 1, pair[1].line_start.unwrap());
        }
        assert!(
            sections
                .iter()
                .all(|s| s.text.chars().count() <= TEXT_CHUNK_CHAR_BUDGET)
        );
        assert_eq!(motivation.last().unwrap().line_end, Some(402));
    }

    #[test]
    fn md_parser_uses_heading_sections_but_txt_does_not() {
        let md = TextParser::new("md")
            .parse(MARKDOWN_FIXTURE.as_bytes())
            .unwrap();
        assert!(
            md.sections
                .iter()
                .any(|s| s.heading.as_deref() == Some("Scope"))
        );
        let txt = TextParser::new("txt")
            .parse(MARKDOWN_FIXTURE.as_bytes())
            .unwrap();
        assert!(txt.sections.iter().all(|s| s.heading.is_none()));
    }
}

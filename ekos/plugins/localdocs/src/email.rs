//! `.eml` (RFC 5322 / MIME) parsing via `mail-parser` (RFC 0025).
//!
//! Produces a header section (Subject/From/To/Date) followed by body
//! sections chunked from the `text/plain` part, falling back to the
//! `text/html` part rendered through `html.rs`'s shared `html_to_text`.
//! Attachments and Outlook `.msg` are explicit non-goals — see the RFC.

use crate::{
    DocumentParser, DocumentSection, ParseError, ParsedDocument, SECTION_TEXT_MAX_CHARS,
    SECTIONS_MAX, TEXT_CHUNK_CHAR_BUDGET, html::html_to_text, text::chunk_text,
};
use mail_parser::{Address, Message, MessageParser};

pub struct EmailParser;

impl DocumentParser for EmailParser {
    fn supported_extension(&self) -> &str {
        "eml"
    }

    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
        let message = MessageParser::default()
            .parse(bytes)
            .ok_or_else(|| ParseError::Malformed("not a parseable RFC 5322 message".into()))?;

        let header_text = header_block(&message);
        let body_text = body_text(&message)?;

        let mut sections = Vec::with_capacity(1 + SECTIONS_MAX);
        sections.push(DocumentSection {
            page: None,
            index: 0,
            text: header_text.chars().take(SECTION_TEXT_MAX_CHARS).collect(),
        });
        for mut section in chunk_text(&body_text, TEXT_CHUNK_CHAR_BUDGET) {
            if sections.len() >= SECTIONS_MAX {
                break;
            }
            section.index = sections.len();
            sections.push(section);
        }

        let text = if body_text.is_empty() {
            header_text
        } else {
            format!("{header_text}\n\n{body_text}")
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

fn header_block(message: &Message<'_>) -> String {
    let mut out = String::new();
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(&value);
            out.push('\n');
        }
    };
    push("Subject", message.subject().unwrap_or_default().to_string());
    push("From", render_address(message.from()));
    push("To", render_address(message.to()));
    push(
        "Date",
        message.date().map(|d| d.to_string()).unwrap_or_default(),
    );
    out
}

/// Renders an address header as `Name <addr>` entries, comma-separated.
/// Groups are flattened — the group name itself carries no search value.
fn render_address(address: Option<&Address<'_>>) -> String {
    let Some(address) = address else {
        return String::new();
    };
    let rendered: Vec<String> = address
        .clone()
        .into_list()
        .iter()
        .filter_map(|addr| match (addr.name(), addr.address()) {
            (Some(name), Some(email)) => Some(format!("{name} <{email}>")),
            (None, Some(email)) => Some(email.to_string()),
            (Some(name), None) => Some(name.to_string()),
            (None, None) => None,
        })
        .collect();
    rendered.join(", ")
}

/// Prefers the `text/plain` part; falls back to rendering the `text/html`
/// part. An email with neither is not an error — it yields header-only
/// sections rather than being dropped from the ledger entirely.
fn body_text(message: &Message<'_>) -> Result<String, ParseError> {
    if let Some(plain) = message.body_text(0)
        && !plain.trim().is_empty()
    {
        return Ok(plain.into_owned());
    }
    match message.body_html(0) {
        Some(html) => html_to_text(html.as_bytes()),
        None => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTIPART_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/incident-thread.eml");
    const HTML_ONLY_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/html-only.eml");

    #[test]
    fn supported_extension_is_eml() {
        assert_eq!(EmailParser.supported_extension(), "eml");
        assert_eq!(EmailParser.supported_extensions(), vec!["eml"]);
    }

    #[test]
    fn first_section_is_the_header_block() {
        let parsed = EmailParser.parse(MULTIPART_FIXTURE).unwrap();
        let header = &parsed.sections[0].text;
        assert!(header.contains("Subject: Nightly batch failure"));
        assert!(header.contains("From: Dana Ops <dana.ops@example.invalid>"));
        assert!(header.contains("To:"));
        assert!(header.contains("Date:"));
        assert_eq!(parsed.sections[0].page, None);
    }

    #[test]
    fn body_sections_follow_the_header_with_sequential_indexes() {
        let parsed = EmailParser.parse(MULTIPART_FIXTURE).unwrap();
        assert!(parsed.sections.len() >= 2);
        for (i, s) in parsed.sections.iter().enumerate() {
            assert_eq!(s.index, i);
            assert!(s.text.chars().count() <= TEXT_CHUNK_CHAR_BUDGET.max(SECTION_TEXT_MAX_CHARS));
        }
        let body = parsed.sections[1..]
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>();
        assert!(body.contains("ingest_orders"));
    }

    /// The `text/plain` part wins over `text/html` when both exist — the
    /// HTML alternative's markup must never leak into the ledger.
    #[test]
    fn text_plain_part_is_preferred_over_the_html_alternative() {
        let parsed = EmailParser.parse(MULTIPART_FIXTURE).unwrap();
        assert!(parsed.text.contains("plain-text alternative"));
        assert!(!parsed.text.contains("html-only marker"));
        assert!(!parsed.text.contains("<p>"));
    }

    #[test]
    fn html_body_is_used_when_no_text_plain_part_exists() {
        let parsed = EmailParser.parse(HTML_ONLY_FIXTURE).unwrap();
        assert!(parsed.text.contains("Quarterly retention review"));
        assert!(!parsed.text.contains("<strong>"));
    }

    #[test]
    fn no_tables_images_or_page_count() {
        let parsed = EmailParser.parse(MULTIPART_FIXTURE).unwrap();
        assert!(parsed.tables.is_empty());
        assert!(parsed.images.is_empty());
        assert_eq!(parsed.page_count, None);
    }

    #[test]
    fn malformed_input_returns_parse_error_rather_than_panicking() {
        for input in [
            &b""[..],
            &[0xff, 0xfe, 0x00][..],
            &b"not an email at all"[..],
        ] {
            let result = EmailParser.parse(input);
            assert!(
                result.is_ok() || matches!(result, Err(ParseError::Malformed(_))),
                "must never panic or produce another error kind"
            );
        }
    }
}

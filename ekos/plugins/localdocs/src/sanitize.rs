//! Prompt-injection sanitization for extracted document text (RFC 0023
//! addendum).
//!
//! A PDF/DOCX's text layer or table cells can carry characters that are
//! invisible when the document is viewed but that an LLM still reads as
//! text — zero-width joiners, the Unicode "tag" block (historically used
//! for language tagging, repurposed in the wild for ASCII-smuggled hidden
//! instructions), and stray BOMs. Since every string this connector
//! extracts (prose excerpt, table cells, OCR output) eventually lands in
//! front of an agent through `ekos_search`/`ekos_state`, a malicious
//! document could otherwise plant instructions no human reviewer would
//! ever see. This strips that class of character before any text is
//! written to an artifact — the same scope (not a general-purpose Unicode
//! sanitizer) `book-to-skill` uses for the same threat model.

/// Zero-width / invisible characters stripped outright: ZWSP, ZWNJ, ZWJ,
/// word joiner, BOM/zero-width no-break space.
const ZERO_WIDTH: [char; 5] = ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];

/// Unicode tag block (`U+E0000`–`U+E007F`) — originally for language
/// tagging, now the primary vector for invisible ASCII-smuggled text
/// (each tag codepoint maps 1:1 onto an ASCII character, rendering as
/// nothing).
const TAG_BLOCK_START: u32 = 0xE0000;
const TAG_BLOCK_END: u32 = 0xE007F;

fn is_sanitized_char(c: char) -> bool {
    if ZERO_WIDTH.contains(&c) {
        return true;
    }
    let cp = c as u32;
    (TAG_BLOCK_START..=TAG_BLOCK_END).contains(&cp)
}

/// Result of sanitizing one string: the cleaned text and how many
/// characters were removed.
pub struct Sanitized {
    pub text: String,
    pub removed: usize,
}

/// Strip zero-width and Unicode-tag-block characters from `input`.
pub fn sanitize_text(input: &str) -> Sanitized {
    let mut removed = 0usize;
    let text: String = input
        .chars()
        .filter(|c| {
            if is_sanitized_char(*c) {
                removed += 1;
                false
            } else {
                true
            }
        })
        .collect();
    Sanitized { text, removed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_zero_width_characters() {
        let s = sanitize_text("hel\u{200B}lo\u{FEFF} world\u{200D}");
        assert_eq!(s.text, "hello world");
        assert_eq!(s.removed, 3);
    }

    #[test]
    fn strips_unicode_tag_block() {
        // Tag-block "hidden" text spelling "hi" via U+E0068 U+E0069
        // (tag-h, tag-i) — invisible when rendered, legible to a model
        // reading the raw string.
        let hidden = "\u{E0068}\u{E0069}";
        let s = sanitize_text(&format!("visible{hidden}text"));
        assert_eq!(s.text, "visibletext");
        assert_eq!(s.removed, 2);
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        let s = sanitize_text("Putting it all together — 34 pages, § 3.2, café");
        assert_eq!(s.text, "Putting it all together — 34 pages, § 3.2, café");
        assert_eq!(s.removed, 0);
    }

    #[test]
    fn empty_input_is_a_no_op() {
        let s = sanitize_text("");
        assert_eq!(s.text, "");
        assert_eq!(s.removed, 0);
    }
}

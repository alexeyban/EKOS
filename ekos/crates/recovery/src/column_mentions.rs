//! RFC 0172 (documentation vs data) — sentences in prose documentation that name one column.
//!
//! A Markdown or Confluence page may say `` `invoice.status` is one of open, closed or void ``.
//! That is a checkable claim about a column, exactly like a `COMMENT ON COLUMN`, but recovery
//! cannot know which tables exist. So this module only records the **candidates**: every sentence
//! (or table row, or list item) that names exactly one `` `table.column` `` in code formatting.
//! `ekos migrate assess` matches them against the live catalog and reads the claim; a mention of
//! something that is not a column there (`` `schema.sql` ``) is simply never matched.
//!
//! Pure functions, no I/O.

use serde::{Deserialize, Serialize};

/// The property a `Section` / `Page` carries its mentions under.
pub const PROPERTY: &str = "column_mentions";

const MAX_PER_SOURCE: usize = 100;
const MAX_TEXT_CHARS: usize = 400;

/// One sentence that names one column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMention {
    /// Lowercase, as written: `invoice` or `public.invoice`.
    pub table: String,
    /// Lowercase.
    pub column: String,
    /// The sentence, with the mention still in it.
    pub text: String,
    /// 1-based line in the source file, or 0 when the source has no lines (HTML).
    pub line: u32,
}

/// `table.column` or `schema.table.column`, each part a plain identifier.
fn qualified(span: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = span.split('.').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let ident = |p: &str| {
        p.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if !parts.iter().all(|p| ident(p)) {
        return None;
    }
    let column = parts[parts.len() - 1].to_lowercase();
    let table = parts[..parts.len() - 1].join(".").to_lowercase();
    Some((table, column))
}

/// The inline code spans of `unit`.
fn code_spans(unit: &str) -> Vec<&str> {
    unit.split('`').skip(1).step_by(2).collect()
}

/// Split a line into sentences, never inside a code span.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_code = false;
    let bytes = line.as_bytes();
    for (i, c) in line.char_indices() {
        match c {
            '`' => in_code = !in_code,
            '.' | '!' | '?' | ';'
                if !in_code && bytes.get(i + 1).is_none_or(|b| b.is_ascii_whitespace()) =>
            {
                out.push(&line[start..=i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < line.len() {
        out.push(&line[start..]);
    }
    out
}

fn mention(unit: &str, line: u32) -> Option<ColumnMention> {
    let mut found: Vec<(String, String)> =
        code_spans(unit).into_iter().filter_map(qualified).collect();
    found.sort();
    found.dedup();
    // Two columns in one sentence: the claim could be about either, so it is about neither.
    let [(table, column)] = found.as_slice() else {
        return None;
    };
    let text: String = unit.trim().chars().take(MAX_TEXT_CHARS).collect();
    Some(ColumnMention {
        table: table.clone(),
        column: column.clone(),
        text,
        line,
    })
}

/// Mentions in Markdown/plain text whose first line is `first_line`. Fenced code is skipped: a
/// query in a code block names columns without saying anything about them.
pub fn parse_text(text: &str, first_line: u32) -> Vec<ColumnMention> {
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, raw) in text.lines().enumerate() {
        let t = raw.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || !t.contains('`') {
            continue;
        }
        let line = first_line + i as u32;
        // A table row is one statement about the column in its first cell.
        let units = if t.starts_with('|') {
            vec![t]
        } else {
            sentences(t)
        };
        for unit in units {
            if let Some(m) = mention(unit, line) {
                out.push(m);
                if out.len() == MAX_PER_SOURCE {
                    return out;
                }
            }
        }
    }
    out
}

/// Mentions in an HTML body (Confluence storage format): `<code>` is the code formatting, and
/// block ends are line ends. HTML has no source lines, so every mention has line 0.
pub fn parse_html(html: &str) -> Vec<ColumnMention> {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].to_lowercase();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        match name {
            "code" => text.push('`'),
            "p" | "li" | "tr" | "br" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                text.push('\n')
            }
            "td" | "th" => text.push(' '),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    parse_text(&text, 0)
        .into_iter()
        .map(|m| ColumnMention { line: 0, ..m })
        .collect()
}

/// `text` without its `table.column` code spans, so a claim is read from what the sentence says
/// about the column and never from the column's own name (`unique_id`, `is_required`).
pub fn without_mention(text: &str) -> String {
    text.split('`')
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 && qualified(part).is_some() {
                "this column"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(ms: &[ColumnMention]) -> Vec<(&str, &str, u32)> {
        ms.iter()
            .map(|m| (m.table.as_str(), m.column.as_str(), m.line))
            .collect()
    }

    #[test]
    fn a_sentence_naming_one_column_is_a_mention_with_its_line() {
        let text = "# Invoices\n\
                    \n\
                    `invoice.status` is one of open, closed or void. Totals are in `invoice.total`.\n\
                    - `Public.Invoice.Qty` is always positive\n\
                    | `invoice.ref` | never null |\n";
        let ms = parse_text(text, 10);
        assert_eq!(
            pairs(&ms),
            vec![
                ("invoice", "status", 12),
                ("invoice", "total", 12),
                ("public.invoice", "qty", 13),
                ("invoice", "ref", 14),
            ]
        );
        assert_eq!(
            ms[0].text,
            "`invoice.status` is one of open, closed or void."
        );
        assert_eq!(ms[3].text, "| `invoice.ref` | never null |");
    }

    #[test]
    fn what_is_not_one_qualified_column_in_code_formatting_is_not_a_mention() {
        for text in [
            // two columns in one sentence
            "`invoice.status` must match `invoice.kind`.",
            // not qualified, or not in code formatting
            "`status` is never null.",
            "invoice.status is never null.",
            // not identifiers
            "Run `ekos migrate assess --dsn x.y` first.",
            "`a.b.c.d` is too deep.",
            "`1.5` is a number.",
            // a query in a fenced block says nothing about the column
            "```sql\nSELECT `invoice.status` FROM invoice; -- never null\n```",
        ] {
            assert!(parse_text(text, 1).is_empty(), "{text}");
        }
        // The same column twice in a sentence is still one column.
        assert_eq!(
            parse_text("`a.b` is unique; `a.b` again and `a.b` again.", 1).len(),
            2
        );
    }

    #[test]
    fn a_dot_inside_the_code_span_does_not_end_the_sentence() {
        let ms = parse_text("The `invoice.status` column is required. Next.", 1);
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].text, "The `invoice.status` column is required.");
    }

    #[test]
    fn html_code_tags_are_the_code_formatting() {
        let html = "<h2>Rules</h2><p><code>invoice.status</code> is one of open &amp; closed.</p>\
                    <table><tr><td><code>invoice.qty</code></td><td>always positive</td></tr></table>\
                    <p>No code here: invoice.total is required.</p>";
        let ms = parse_html(html);
        assert_eq!(
            pairs(&ms),
            vec![("invoice", "status", 0), ("invoice", "qty", 0)]
        );
        assert_eq!(ms[0].text, "`invoice.status` is one of open & closed.");
    }

    #[test]
    fn the_claim_is_read_without_the_columns_own_name() {
        let cleaned = without_mention("`t.unique_id` is the row `key`.");
        assert!(!cleaned.contains("unique_id"));
        assert!(cleaned.contains("this column"));
        // Other code spans stay.
        assert!(cleaned.contains("key"));
    }
}

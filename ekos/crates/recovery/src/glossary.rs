//! RFC 0170 Phase 3 — business glossaries: the one place meaning *is* written down.
//!
//! A glossary entry (`term` + `definition`) is read only from text that says it is a glossary —
//! a heading or document name with "glossary", "terms", "definitions", "dictionary" or
//! "terminology" — so ordinary prose with a colon never becomes a definition. Recognized shapes:
//!
//! | Shape | Example |
//! |---|---|
//! | bold term + dash/colon | `**Open order** — an order not yet shipped` / `**Open order**: …` |
//! | list item + colon/dash | `- Open order: an order not yet shipped` (a bare `X: y` line is not an entry) |
//! | two-column table | `| Open order | an order not yet shipped |` (header row and `---` skipped) |
//! | HTML table / `<dl>` | Confluence storage format: `<tr><td>term</td><td>def</td></tr>`, `<dt>`/`<dd>` |
//!
//! Synthesis (`ekos_semantic::business_semantics`) matches terms to recovered concepts, code labels
//! and tables by exact normalized words, and reports unmatched terms as gaps.

use serde::{Deserialize, Serialize};

/// One glossary entry with its 1-based line (0 when unknown, e.g. Confluence HTML).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub term: String,
    pub definition: String,
    pub line: u32,
}

const MARKERS: [&str; 5] = [
    "glossary",
    "terms",
    "definitions",
    "dictionary",
    "terminology",
];

/// Whether a heading path or a document name marks its text as a glossary.
pub fn is_glossary(names: &[&str]) -> bool {
    names.iter().any(|n| {
        let n = n.to_lowercase();
        MARKERS.iter().any(|m| n.contains(m))
    })
}

fn clean(s: &str) -> String {
    s.trim()
        .trim_matches(|c: char| c == '*' || c == '_' || c == '`')
        .trim()
        .to_string()
}

fn entry(term: &str, definition: &str, line: u32) -> Option<GlossaryEntry> {
    let term = clean(term);
    let definition = clean(definition);
    let words = term.split_whitespace().count();
    let ok = (1..=6).contains(&words)
        && term.len() <= 60
        && term.chars().next().is_some_and(char::is_alphanumeric)
        && definition.len() >= 3
        && definition.chars().any(char::is_alphabetic);
    ok.then_some(GlossaryEntry {
        term,
        definition,
        line,
    })
}

/// Entries in Markdown/plain text whose first line is `first_line`.
pub fn parse_text(text: &str, first_line: u32) -> Vec<GlossaryEntry> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = first_line + i as u32;
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // Two-column table rows.
        if t.starts_with('|') {
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
            if cells.len() >= 2
                && !cells[0].chars().all(|c| c == '-' || c == ':' || c == ' ')
                && !matches!(cells[0].to_lowercase().as_str(), "term" | "name" | "word")
                && let Some(e) = entry(cells[0], cells[1], line)
            {
                out.push(e);
            }
            continue;
        }
        // Only formatted entries: a list item or a bold term. A bare `X: y` line in a glossary
        // section is as likely an intro sentence as a definition.
        let listed = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "));
        if listed.is_none() && !t.starts_with("**") {
            continue;
        }
        let body = listed.unwrap_or(t);
        // `**Term** — def`, `**Term**: def`, `Term: def`, `Term — def`, `Term - def`.
        let split = if let Some(rest) = body.strip_prefix("**") {
            rest.split_once("**")
                .map(|(term, def)| (term, def.trim_start_matches([':', '—', '–', '-', ' '])))
        } else {
            [" — ", " – ", ": ", " - "]
                .iter()
                .find_map(|sep| body.split_once(sep))
        };
        if let Some((term, def)) = split
            && let Some(e) = entry(term, def, line)
        {
            out.push(e);
        }
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Entries in HTML (Confluence storage format): two-cell table rows and `<dt>`/`<dd>` pairs.
pub fn parse_html(html: &str) -> Vec<GlossaryEntry> {
    let lower = html.to_lowercase();
    let mut out = Vec::new();
    // Table rows.
    let mut at = 0;
    while let Some(start) = lower[at..].find("<tr") {
        let start = at + start;
        let end = lower[start..]
            .find("</tr>")
            .map(|e| start + e)
            .unwrap_or(lower.len());
        let row = &html[start..end];
        let row_l = &lower[start..end];
        let cells: Vec<String> = row_l
            .match_indices("<td")
            .map(|(i, _)| {
                let open_end = row[i..].find('>').map(|e| i + e + 1).unwrap_or(i);
                let close = row_l[open_end..]
                    .find("</td>")
                    .map(|e| open_end + e)
                    .unwrap_or(row.len());
                strip_tags(&row[open_end..close]).trim().to_string()
            })
            .collect();
        if cells.len() >= 2
            && let Some(e) = entry(&cells[0], &cells[1], 0)
        {
            out.push(e);
        }
        at = end.max(start + 3);
    }
    // Definition lists.
    let mut at = 0;
    while let Some(dt) = lower[at..].find("<dt") {
        let dt = at + dt;
        let Some(dt_body) = lower[dt..].find('>').map(|e| dt + e + 1) else {
            break;
        };
        let Some(dt_end) = lower[dt_body..].find("</dt>").map(|e| dt_body + e) else {
            break;
        };
        let Some(dd) = lower[dt_end..].find("<dd").map(|e| dt_end + e) else {
            break;
        };
        let dd_body = lower[dd..].find('>').map(|e| dd + e + 1).unwrap_or(dd);
        let dd_end = lower[dd_body..]
            .find("</dd>")
            .map(|e| dd_body + e)
            .unwrap_or(lower.len());
        if let Some(e) = entry(
            &strip_tags(&html[dt_body..dt_end]),
            &strip_tags(&html[dd_body..dd_end]),
            0,
        ) {
            out.push(e);
        }
        at = dd_end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_shapes_are_read_with_their_lines() {
        let text = "Intro prose: not an entry because it is far too long to be a term at all, really.\n\
**Open order** — an order that is not yet fully shipped\n\
- Customer: an entity that buys from us\n\
| Term | Definition |\n\
|------|------------|\n\
| Equity account | an account in category Q |\n";
        let e = parse_text(text, 10);
        let got: Vec<(&str, u32)> = e.iter().map(|e| (e.term.as_str(), e.line)).collect();
        assert_eq!(
            got,
            vec![("Open order", 11), ("Customer", 12), ("Equity account", 15)]
        );
        assert_eq!(e[0].definition, "an order that is not yet fully shipped");
    }

    #[test]
    fn html_tables_and_definition_lists_are_read() {
        let html = "<table><tr><th>Term</th><th>Meaning</th></tr>\
<tr><td><strong>Open order</strong></td><td>Not yet shipped &amp; not closed</td></tr></table>\
<dl><dt>RFQ</dt><dd>Request for quotation</dd></dl>";
        let e = parse_html(html);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].term, "Open order");
        assert_eq!(e[0].definition, "Not yet shipped & not closed");
        assert_eq!(e[1].term, "RFQ");
    }

    #[test]
    fn only_marked_text_is_a_glossary() {
        assert!(is_glossary(&["docs/business-glossary.md"]));
        assert!(is_glossary(&["Reference", "Terms and definitions"]));
        assert!(!is_glossary(&["Installation", "Configuration"]));
    }
}

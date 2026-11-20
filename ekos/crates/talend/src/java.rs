//! Folding Java string expressions (RFC 0263 §"Resolution").
//!
//! Talend stores SQL, table names and file names as Java expressions:
//! `"SELECT … \"" + context.schema_ods + "\".\"ODS_VENTE\""`. [`fold`] joins string literals
//! (with Java escapes), numbers and resolvable `context.*` references across top-level `+`. Any
//! other operand (`globalMap.get(...)`, a method call, a ternary) becomes a named hole, written
//! into the text as `{{operand}}` and listed, never guessed.

use serde::{Deserialize, Serialize};

/// The result of folding one expression.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folded {
    /// The folded text; holes appear as `{{operand}}`.
    pub text: String,
    /// Operands that could not be folded, as written.
    pub holes: Vec<String>,
    /// `context.*` variables the value was folded from.
    pub context_used: Vec<String>,
}

impl Folded {
    /// True when every operand was folded.
    pub fn complete(&self) -> bool {
        self.holes.is_empty()
    }
}

/// Folds `expr`. `context` resolves a context variable's name to its value.
pub fn fold(expr: &str, context: &dyn Fn(&str) -> Option<String>) -> Folded {
    let mut out = Folded::default();
    for operand in split_concat(expr) {
        fold_operand(strip_parens(operand.trim()), context, &mut out);
    }
    out
}

fn fold_operand(operand: &str, context: &dyn Fn(&str) -> Option<String>, out: &mut Folded) {
    if operand.is_empty() {
        return;
    }
    if let Some(text) = string_literal(operand) {
        out.text.push_str(&text);
        return;
    }
    if let Some(c) = char_literal(operand) {
        out.text.push(c);
        return;
    }
    if is_number(operand) {
        out.text
            .push_str(operand.trim_end_matches(['L', 'l', 'd', 'D', 'f', 'F']));
        return;
    }
    if let Some(name) = context_ref(operand)
        && let Some(value) = context(name)
    {
        out.text.push_str(&value);
        if !out.context_used.iter().any(|c| c == name) {
            out.context_used.push(name.to_string());
        }
        return;
    }
    out.text.push_str("{{");
    out.text.push_str(operand);
    out.text.push_str("}}");
    out.holes.push(operand.to_string());
}

/// `context.name` (a plain identifier after `context.`), else `None`.
pub fn context_ref(operand: &str) -> Option<&str> {
    let name = operand.strip_prefix("context.")?;
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    ok.then_some(name)
}

/// Every `context.name` referenced anywhere in `text` (outside string literals), in order, once.
pub fn context_refs(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            in_string = !(c == b'"' && !escaped);
            escaped = c == b'\\' && !escaped;
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        let at_word_start = i == 0 || !is_ident_byte(bytes[i - 1]);
        // Compared as bytes: `i` can sit inside a multi-byte character (job text is often French).
        if at_word_start && bytes[i..].starts_with(b"context.") {
            let start = i + "context.".len();
            let end = text[start..]
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map_or(text.len(), |n| start + n);
            let name = &text[start..end];
            if !name.is_empty() && !found.iter().any(|f| f == name) {
                found.push(name.to_string());
            }
            i = end;
            continue;
        }
        i += 1;
    }
    found
}

/// Every `qualifier.member` in `text` outside string and char literals whose qualifier is one of
/// `qualifiers` (a tMap's input names, `Var`, `input_row`), in order, once each. A qualifier
/// preceded by `.` or an identifier character is a member of something else and is skipped.
pub fn qualified_refs(text: &str, qualifiers: &[&str]) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            quote = Some(c);
            i += 1;
            continue;
        }
        let starts_word =
            (c.is_ascii_alphabetic() || c == b'_') && (i == 0 || !is_ident_byte(bytes[i - 1]));
        if !starts_word {
            i += 1;
            continue;
        }
        let end = ident_end(bytes, i);
        let qualifier = &text[i..end];
        if bytes.get(end) == Some(&b'.') && qualifiers.contains(&qualifier) {
            let member_end = ident_end(bytes, end + 1);
            let member = &text[end + 1..member_end];
            let called = text[member_end..].trim_start().starts_with('(');
            if !member.is_empty() && !called {
                let pair = (qualifier.to_string(), member.to_string());
                if !found.contains(&pair) {
                    found.push(pair);
                }
            }
            i = member_end.max(end + 1);
            continue;
        }
        i = end;
    }
    found
}

fn ident_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    end
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// Splits on `+` outside string/char literals and parentheses.
fn split_concat(expr: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in expr.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '+' if depth == 0 => {
                parts.push(&expr[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&expr[start..]);
    parts
}

/// Removes parentheses that wrap the whole operand: `(("a"))` → `"a"`.
fn strip_parens(mut s: &str) -> &str {
    while s.starts_with('(') && s.ends_with(')') && wraps_whole(s) {
        s = s[1..s.len() - 1].trim();
    }
    s
}

fn wraps_whole(s: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && i != s.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// The value of a Java string literal `"…"` that is the whole operand, with escapes applied.
pub fn string_literal(operand: &str) -> Option<String> {
    let inner = operand.strip_prefix('"')?.strip_suffix('"')?;
    unescape(inner, '"')
}

fn char_literal(operand: &str) -> Option<char> {
    let inner = operand.strip_prefix('\'')?.strip_suffix('\'')?;
    let text = unescape(inner, '\'')?;
    let mut chars = text.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// Applies Java escapes. `None` when the text holds an unescaped `quote` (so it was not one literal).
fn unescape(inner: &str, quote: char) -> Option<String> {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == quote {
            return None;
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            '0' => out.push('\0'),
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

fn is_number(operand: &str) -> bool {
    let body = operand
        .strip_prefix('-')
        .unwrap_or(operand)
        .trim_end_matches(['L', 'l', 'd', 'D', 'f', 'F']);
    !body.is_empty()
        && body.chars().all(|c| c.is_ascii_digit() || c == '.')
        && body.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(name: &str) -> Option<String> {
        match name {
            "schema_ods" => Some("VENTE_ODS".into()),
            "projectFolder" => Some("C:/PROJET".into()),
            _ => None,
        }
    }

    #[test]
    fn a_query_built_from_literals_and_context_folds_to_sql() {
        let f = fold(
            r#""SELECT \"" + context.schema_ods + "\".\"ODS_VENTE\".\"ID\" FROM \"" + context.schema_ods + "\".\"ODS_VENTE\"""#,
            &ctx,
        );
        assert!(f.complete());
        assert_eq!(
            f.text,
            r#"SELECT "VENTE_ODS"."ODS_VENTE"."ID" FROM "VENTE_ODS"."ODS_VENTE""#
        );
        assert_eq!(f.context_used, vec!["schema_ods"]);
    }

    #[test]
    fn an_operand_that_is_not_a_literal_or_a_known_context_is_a_named_hole() {
        let f = fold(
            r#"context.projectFolder + "/in/" + ((String)globalMap.get("tFileList_1_CURRENT_FILE"))"#,
            &ctx,
        );
        assert!(!f.complete());
        assert_eq!(
            f.text,
            r#"C:/PROJET/in/{{(String)globalMap.get("tFileList_1_CURRENT_FILE")}}"#
        );
        assert_eq!(f.holes.len(), 1);
        // An unknown context variable is a hole too: its value is not in the project.
        assert_eq!(fold("context.missing", &ctx).holes, vec!["context.missing"]);
    }

    #[test]
    fn plus_inside_a_string_or_a_call_does_not_split() {
        let f = fold(r#""a + b" + "|" + 'x' + 12"#, &ctx);
        assert_eq!(f.text, "a + b|x12");
        let g = fold(r#"String.valueOf(1 + 2) + "x""#, &ctx);
        assert_eq!(g.holes, vec!["String.valueOf(1 + 2)"]);
    }

    #[test]
    fn java_escapes_are_applied() {
        assert_eq!(fold(r#""a\tb\n\u0041\\""#, &ctx).text, "a\tb\nA\\");
        // `"a" "b"` is not one literal: an unescaped quote inside means it is not foldable.
        assert_eq!(fold(r#""a" "b""#, &ctx).holes.len(), 1);
    }

    #[test]
    fn qualified_references_name_input_columns_and_skip_strings_and_calls() {
        let refs = qualified_refs(
            r#"row1.QTE*row1.PRIX + Var.x + " row1.NOT " + row2.ID.equals(row3.ID) + a.row1.Z + row1.trim()"#,
            &["row1", "row2", "row3", "Var"],
        );
        assert_eq!(
            refs,
            vec![
                ("row1".into(), "QTE".into()),
                ("row1".into(), "PRIX".into()),
                ("Var".into(), "x".into()),
                ("row2".into(), "ID".into()),
                ("row3".into(), "ID".into()),
            ]
        );
    }

    #[test]
    fn text_with_accents_never_splits_a_character() {
        let text = "// Défini à partir de context.année é + context.dossier_entrée";
        assert_eq!(context_refs(text), vec!["année", "dossier_entrée"]);
        assert_eq!(context_refs("é"), Vec::<String>::new());
        assert_eq!(
            qualified_refs("\"données\" + row1.é + row1.QTÉ + rowé.X", &["row1"]),
            vec![("row1".into(), "QT".into())]
        );
        assert_eq!(
            context_assignments_probe("context.é = 1; context.x = 2;"),
            vec!["x", "é"]
        );
    }

    fn context_assignments_probe(code: &str) -> Vec<String> {
        crate::resolve::context_assignments(code)
            .into_iter()
            .collect()
    }

    #[test]
    fn context_references_are_found_outside_strings_only() {
        assert_eq!(
            context_refs(r#"context.a + "context.b" + x.context.c + context.a + context.d_1"#),
            vec!["a", "d_1"]
        );
    }
}

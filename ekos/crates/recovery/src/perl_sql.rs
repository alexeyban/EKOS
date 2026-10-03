//! RFC 0170 — SQL embedded in Perl strings, as predicate sites.
//!
//! Application code that builds SQL in strings still filters with literals now and then
//! (`WHERE obsolete IS NOT TRUE`, `entity_class = 3`). This scanner finds string literals whose
//! content starts with `SELECT`/`UPDATE`/`DELETE`/`WITH`/`INSERT`, in every Perl quoting form
//! LedgerSMB uses — `q{…}`, `q|…|`, `qq(…)` (brackets nest), `'…'`, `"…"` and heredocs
//! (`<<'SQL'`, `<<"SQL"`, `<<SQL`, `<<~SQL`) — and parses them.
//!
//! In interpolating forms a Perl variable (`$table`, `$self->{id}`, `@{[ … ]}`) becomes the
//! identifier `perl_var`, so a comparison against one is never mistaken for a literal and a table
//! name built from a variable simply resolves to nothing. `?` placeholders are not literals either.
//! Only literal comparisons survive — which, in placeholder-heavy code, is few and deliberate.
//! POD and `#` comments are skipped; a regex containing a quote can confuse the scanner, which
//! costs at worst a missed or unparseable string, never a wrong predicate.

use crate::sql_predicates::{PredicateSite, statement_predicates};

/// One SQL string: its text (interpolations neutralised) and the 1-based line it starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlString {
    pub text: String,
    pub line: u32,
}

fn looks_like_sql(s: &str) -> bool {
    let head: String = s
        .trim_start()
        .chars()
        .take(7)
        .collect::<String>()
        .to_ascii_uppercase();
    ["SELECT", "UPDATE", "DELETE", "WITH", "INSERT"]
        .iter()
        .any(|k| {
            head.starts_with(k)
                && head[k.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
}

/// `$x`, `$x->{k}`, `$x->[0]`, `${x}`, `@{[ … ]}` → `perl_var`. Newlines are kept.
fn neutralise(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if (c == '$' || c == '@')
            && i + 1 < b.len()
            && (b[i + 1] == '{' || b[i + 1].is_alphabetic() || b[i + 1] == '_')
        {
            i += 1;
            if b[i] == '{' {
                let mut depth = 0;
                while i < b.len() {
                    match b[i] {
                        '{' | '[' => depth += 1,
                        '}' | ']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            } else {
                while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == ':') {
                    i += 1;
                }
            }
            // `->{…}`, `->[…]`, `{…}`, `[…]` accessors.
            loop {
                let mut j = i;
                if j + 1 < b.len() && b[j] == '-' && b[j + 1] == '>' {
                    j += 2;
                }
                if j < b.len() && (b[j] == '{' || b[j] == '[') {
                    let close = if b[j] == '{' { '}' } else { ']' };
                    let mut k = j;
                    while k < b.len() && b[k] != close && b[k] != '\n' {
                        k += 1;
                    }
                    if k < b.len() && b[k] == close {
                        i = k + 1;
                        continue;
                    }
                }
                break;
            }
            out.push_str("perl_var");
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn closer(open: char) -> char {
    match open {
        '{' => '}',
        '(' => ')',
        '[' => ']',
        '<' => '>',
        c => c,
    }
}

/// Every SQL-shaped string literal in `source`.
pub fn sql_strings(source: &str) -> Vec<SqlString> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let mut at_line_start = true;
    let mut heredocs: Vec<(String, bool, bool)> = Vec::new(); // (terminator, interpolate, indented)
    let push = |out: &mut Vec<SqlString>, text: String, start_line: u32, interpolate: bool| {
        if looks_like_sql(&text) {
            let leading_newlines =
                text.len() - text.trim_start_matches(['\n', '\r', ' ', '\t']).len();
            let skipped = text[..leading_newlines].matches('\n').count() as u32;
            let body = text.trim_start_matches(['\n', '\r', ' ', '\t']).to_string();
            out.push(SqlString {
                text: if interpolate { neutralise(&body) } else { body },
                line: start_line + skipped,
            });
        }
    };
    while i < chars.len() {
        let c = chars[i];
        // POD: `=word` at a line start up to `=cut`.
        if at_line_start && c == '=' && chars.get(i + 1).is_some_and(|c| c.is_alphabetic()) {
            while i < chars.len() {
                let start = i;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                i += 1;
                line += 1;
                if text.starts_with("=cut") {
                    break;
                }
            }
            at_line_start = true;
            continue;
        }
        if c == '\n' {
            line += 1;
            i += 1;
            at_line_start = true;
            // Pending heredoc bodies start on the line after the operator.
            while let Some((term, interpolate, indented)) = heredocs.first().cloned() {
                heredocs.remove(0);
                let body_line = line;
                let mut body = String::new();
                loop {
                    let start = i;
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    let text: String = chars[start..i].iter().collect();
                    if i < chars.len() {
                        i += 1;
                    }
                    line += 1;
                    let done = if indented {
                        text.trim() == term
                    } else {
                        text == term
                    };
                    if done || i >= chars.len() {
                        break;
                    }
                    body.push_str(&text);
                    body.push('\n');
                }
                push(&mut out, body, body_line, interpolate);
            }
            continue;
        }
        at_line_start = false;
        if c == '#' && !(i > 0 && chars[i - 1] == '$') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let prev_ident = i > 0
            && (chars[i - 1].is_alphanumeric()
                || chars[i - 1] == '_'
                || chars[i - 1] == '$'
                || chars[i - 1] == '@'
                || chars[i - 1] == '%');
        // Heredoc operator.
        if c == '<' && chars.get(i + 1) == Some(&'<') && !prev_ident {
            let mut j = i + 2;
            let indented = chars.get(j) == Some(&'~');
            if indented {
                j += 1;
            }
            let (interpolate, quoted) = match chars.get(j) {
                Some('\'') => (false, Some('\'')),
                Some('"') => (true, Some('"')),
                _ => (true, None),
            };
            if quoted.is_some() {
                j += 1;
            }
            let start = j;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j > start && (quoted.is_none() || chars.get(j) == quoted.as_ref()) {
                let term: String = chars[start..j].iter().collect();
                heredocs.push((term, interpolate, indented));
                i = if quoted.is_some() { j + 1 } else { j };
                continue;
            }
        }
        // q / qq with a delimiter.
        if (c == 'q') && !prev_ident {
            let mut j = i + 1;
            let interpolate = chars.get(j) == Some(&'q');
            if interpolate {
                j += 1;
            }
            let next_ident = chars
                .get(j)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_');
            if !next_ident {
                while chars.get(j).is_some_and(|c| *c == ' ' || *c == '\t') {
                    j += 1;
                }
                if let Some(&open) = chars.get(j)
                    && "{([<|/!#~".contains(open)
                    && !(open == '=')
                {
                    let close = closer(open);
                    let start_line = line;
                    let mut depth = 1;
                    let mut k = j + 1;
                    let mut text = String::new();
                    while k < chars.len() {
                        let ch = chars[k];
                        if ch == '\\' && k + 1 < chars.len() {
                            text.push(ch);
                            text.push(chars[k + 1]);
                            if chars[k + 1] == '\n' {
                                line += 1;
                            }
                            k += 2;
                            continue;
                        }
                        if ch != close && ch == open && open != close {
                            depth += 1;
                        } else if ch == close {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        if ch == '\n' {
                            line += 1;
                        }
                        text.push(ch);
                        k += 1;
                    }
                    push(&mut out, text, start_line, interpolate);
                    i = k + 1;
                    continue;
                }
            }
        }
        // Ordinary quotes.
        if c == '\'' || c == '"' {
            let start_line = line;
            let mut k = i + 1;
            let mut text = String::new();
            while k < chars.len() && chars[k] != c {
                if chars[k] == '\\' && k + 1 < chars.len() {
                    text.push(chars[k]);
                    k += 1;
                }
                if chars[k] == '\n' {
                    line += 1;
                }
                text.push(chars[k]);
                k += 1;
            }
            push(&mut out, text, start_line, c == '"');
            i = k + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// The predicates of every SQL string, lines absolute in the file. Unparseable strings (dynamic
/// SQL assembled from fragments) contribute nothing.
pub fn perl_sql_predicates(source: &str) -> Vec<PredicateSite> {
    use sqlparser::dialect::{GenericDialect, PostgreSqlDialect};
    let mut out = Vec::new();
    for s in sql_strings(source) {
        let parsed = sqlparser::parser::Parser::parse_sql(&PostgreSqlDialect {}, &s.text)
            .or_else(|_| sqlparser::parser::Parser::parse_sql(&GenericDialect {}, &s.text));
        let Ok(stmts) = parsed else { continue };
        // `'$x'` inside the SQL became the string `'perl_var'`: a value, but not a literal one.
        for p in stmts
            .iter()
            .flat_map(statement_predicates)
            .filter(|p| !p.values.iter().any(|v| v.contains("perl_var")))
        {
            let line = if p.line == 0 {
                s.line as u64
            } else {
                s.line as u64 + p.line - 1
            };
            out.push(PredicateSite { line, ..p });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quoting_form_is_found_with_its_line() {
        let src = "package X;\n\
my $a = q{SELECT * FROM parts WHERE obsolete IS NOT TRUE};\n\
my $b = $dbh->prepare(q|\n    SELECT id FROM entity WHERE entity_class = 3 AND id = ?|);\n\
my $c = 'not sql at all';\n\
my $d = \"UPDATE $table SET x = 1 WHERE status = 'open' AND id = $self->{id}\";\n\
my $e = <<'SQL';\nSELECT 1 FROM oe\n WHERE oe_class_id IN (1, 2)\nSQL\n\
# my $f = q{SELECT 1 FROM ignored WHERE a = 1};\n\
=pod\n\nmy $g = q{SELECT 1 FROM pod WHERE a = 1};\n\n=cut\n\
my $h = qq(SELECT x FROM t WHERE (a = 1));\n";
        let found: Vec<(u32, String)> = sql_strings(src)
            .iter()
            .map(|s| {
                (
                    s.line,
                    s.text
                        .split_whitespace()
                        .take(3)
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .collect();
        assert_eq!(
            found,
            vec![
                (2, "SELECT * FROM".to_string()),
                (4, "SELECT id FROM".to_string()),
                (6, "UPDATE perl_var SET".to_string()),
                (8, "SELECT 1 FROM".to_string()),
                (17, "SELECT x FROM".to_string()),
            ]
        );
    }

    #[test]
    fn only_literal_comparisons_survive_and_lines_are_absolute() {
        let src = "my $b = q|\n    SELECT id FROM entity WHERE entity_class = 3 AND id = ?|;\n\
my $d = \"UPDATE ar SET x = 1 WHERE status = 'open' AND id = $self->{id} AND link = '$l'\";\n";
        let c: Vec<(String, u64)> = perl_sql_predicates(src)
            .iter()
            .map(|p| (p.canonical(), p.line))
            .collect();
        assert_eq!(
            c,
            vec![
                ("entity.entity_class IN (3)".to_string(), 2),
                ("ar.status IN ('open')".to_string(), 3),
            ]
        );
    }
}

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
        let is_sigil = (c == '$' || c == '@')
            && i + 1 < b.len()
            && (b[i + 1] == '{' || b[i + 1].is_alphabetic() || b[i + 1] == '_');
        if !is_sigil {
            out.push(c);
            i += 1;
            continue;
        }
        i = variable_name_end(&b, i + 1);
        // `->{…}`, `->[…]`, `{…}`, `[…]` accessors.
        while let Some(next) = accessor_end(&b, i) {
            i = next;
        }
        out.push_str("perl_var");
    }
    out
}

/// The index just past the variable name starting at `i` (after its sigil): a `{…}` block with
/// nested braces and brackets, or a run of name characters.
fn variable_name_end(b: &[char], mut i: usize) -> usize {
    if b[i] != '{' {
        while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == ':') {
            i += 1;
        }
        return i;
    }
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// The index just past one `->{…}` / `->[…]` / `{…}` / `[…]` accessor at `i`, closed on the
/// same line; `None` when none starts there.
fn accessor_end(b: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    if j + 1 < b.len() && b[j] == '-' && b[j + 1] == '>' {
        j += 2;
    }
    let close = match b.get(j)? {
        '{' => '}',
        '[' => ']',
        _ => return None,
    };
    let mut k = j;
    while k < b.len() && b[k] != close && b[k] != '\n' {
        k += 1;
    }
    (k < b.len() && b[k] == close).then_some(k + 1)
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
    let mut s = PerlScanner {
        chars: &chars,
        i: 0,
        line: 1,
        out: Vec::new(),
        heredocs: Vec::new(),
    };
    let mut at_line_start = true;
    while s.i < chars.len() {
        let c = chars[s.i];
        // POD: `=word` at a line start up to `=cut`.
        if at_line_start && c == '=' && chars.get(s.i + 1).is_some_and(|c| c.is_alphabetic()) {
            s.skip_pod();
            continue;
        }
        if c == '\n' {
            s.line += 1;
            s.i += 1;
            at_line_start = true;
            s.read_heredoc_bodies();
            continue;
        }
        at_line_start = false;
        if c == '#' && !(s.i > 0 && chars[s.i - 1] == '$') {
            while s.i < chars.len() && chars[s.i] != '\n' {
                s.i += 1;
            }
            continue;
        }
        if !s.after_identifier() && (s.heredoc_operator() || s.q_string()) {
            continue;
        }
        if c == '\'' || c == '"' {
            s.quoted_string(c);
            continue;
        }
        s.i += 1;
    }
    s.out
}

/// The scan position of [`sql_strings`] and what it has found.
struct PerlScanner<'a> {
    chars: &'a [char],
    i: usize,
    line: u32,
    out: Vec<SqlString>,
    /// Heredocs whose operator was read and whose body starts on the next line:
    /// (terminator, interpolate, indented).
    heredocs: Vec<(String, bool, bool)>,
}

impl PerlScanner<'_> {
    /// Keeps `text` when it is SQL-shaped, at the line its first non-blank character is on.
    fn push(&mut self, text: String, start_line: u32, interpolate: bool) {
        if !looks_like_sql(&text) {
            return;
        }
        let leading_newlines = text.len() - text.trim_start_matches(['\n', '\r', ' ', '\t']).len();
        let skipped = text[..leading_newlines].matches('\n').count() as u32;
        let body = text.trim_start_matches(['\n', '\r', ' ', '\t']).to_string();
        self.out.push(SqlString {
            text: if interpolate { neutralise(&body) } else { body },
            line: start_line + skipped,
        });
    }

    /// Whether the character before the position can end an identifier or a sigil.
    fn after_identifier(&self) -> bool {
        self.i > 0 && {
            let p = self.chars[self.i - 1];
            p.is_alphanumeric() || matches!(p, '_' | '$' | '@' | '%')
        }
    }

    /// Reads the rest of the current line and steps past its newline.
    fn take_line(&mut self) -> String {
        let start = self.i;
        while self.i < self.chars.len() && self.chars[self.i] != '\n' {
            self.i += 1;
        }
        self.chars[start..self.i].iter().collect()
    }

    /// Skips a POD block: every line up to and including `=cut`.
    fn skip_pod(&mut self) {
        while self.i < self.chars.len() {
            let text = self.take_line();
            self.i += 1;
            self.line += 1;
            if text.starts_with("=cut") {
                break;
            }
        }
    }

    /// Pending heredoc bodies start on the line after the operator.
    fn read_heredoc_bodies(&mut self) {
        while !self.heredocs.is_empty() {
            let (term, interpolate, indented) = self.heredocs.remove(0);
            let body_line = self.line;
            let mut body = String::new();
            loop {
                let text = self.take_line();
                if self.i < self.chars.len() {
                    self.i += 1;
                }
                self.line += 1;
                let done = if indented {
                    text.trim() == term
                } else {
                    text == term
                };
                if done || self.i >= self.chars.len() {
                    break;
                }
                body.push_str(&text);
                body.push('\n');
            }
            self.push(body, body_line, interpolate);
        }
    }

    /// A heredoc operator (`<<TERM`, `<<~TERM`, `<<'TERM'`, `<<"TERM"`) at the position: its
    /// terminator is queued and the operator stepped over.
    fn heredoc_operator(&mut self) -> bool {
        let chars = self.chars;
        if !(chars[self.i] == '<' && chars.get(self.i + 1) == Some(&'<')) {
            return false;
        }
        let mut j = self.i + 2;
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
        if j == start || (quoted.is_some() && chars.get(j) != quoted.as_ref()) {
            return false;
        }
        let term: String = chars[start..j].iter().collect();
        self.heredocs.push((term, interpolate, indented));
        self.i = if quoted.is_some() { j + 1 } else { j };
        true
    }

    /// A `q` / `qq` string with a delimiter at the position: read whole and stepped over.
    fn q_string(&mut self) -> bool {
        let chars = self.chars;
        if chars[self.i] != 'q' {
            return false;
        }
        let mut j = self.i + 1;
        let interpolate = chars.get(j) == Some(&'q');
        if interpolate {
            j += 1;
        }
        if chars
            .get(j)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            return false;
        }
        while chars.get(j).is_some_and(|c| *c == ' ' || *c == '\t') {
            j += 1;
        }
        let Some(&open) = chars.get(j) else {
            return false;
        };
        if !"{([<|/!#~".contains(open) {
            return false;
        }
        let start_line = self.line;
        let (text, end) = self.delimited_text(open, j + 1);
        self.push(text, start_line, interpolate);
        self.i = end + 1;
        true
    }

    /// The text from `from` to the delimiter closing `open` (nested pairs counted, a backslash
    /// escaping the next character), and the index of that closing delimiter. Counts lines.
    fn delimited_text(&mut self, open: char, from: usize) -> (String, usize) {
        let chars = self.chars;
        let close = closer(open);
        let mut depth = 1;
        let mut k = from;
        let mut text = String::new();
        while k < chars.len() {
            let ch = chars[k];
            if ch == '\\' && k + 1 < chars.len() {
                text.push(ch);
                text.push(chars[k + 1]);
                if chars[k + 1] == '\n' {
                    self.line += 1;
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
                self.line += 1;
            }
            text.push(ch);
            k += 1;
        }
        (text, k)
    }

    /// An ordinary `'…'` or `"…"` string opening at the position.
    fn quoted_string(&mut self, quote: char) {
        let chars = self.chars;
        let start_line = self.line;
        let mut k = self.i + 1;
        let mut text = String::new();
        while k < chars.len() && chars[k] != quote {
            if chars[k] == '\\' && k + 1 < chars.len() {
                text.push(chars[k]);
                k += 1;
            }
            if chars[k] == '\n' {
                self.line += 1;
            }
            text.push(chars[k]);
            k += 1;
        }
        self.push(text, start_line, quote == '"');
        self.i = k + 1;
    }
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

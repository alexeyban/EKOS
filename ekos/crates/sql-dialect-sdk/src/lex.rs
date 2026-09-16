//! Lexical primitives shared by every SQL dialect crate and by `ekos-recovery` (RFC 0146).
//!
//! These answer one question — "is the text at this position real SQL code, or is it inside a
//! string literal, a quoted identifier, a dollar-quoted block or a comment?" — and they are here
//! rather than in one dialect crate because two independent consumers need the identical answer:
//!
//! * `ekos-plugin-sql-dialect-postgres`'s `preprocess` passes, which rewrite or remove statements
//!   `sqlparser` cannot handle, and
//! * `ekos-recovery`'s `sql_comments`, which lifts `COMMENT ON ... IS ...` text out of the same
//!   files *before* those passes strip it.
//!
//! Keeping one copy is deliberate. A second, subtly different quote scanner is precisely the kind
//! of duplicated logic this codebase has watched drift apart before (see `recover.rs`'s several
//! hand-copied `project_key` blocks), and here the two copies would have to agree exactly or the
//! extractor would read text the stripper did not remove, or vice versa.

/// True for characters that may appear inside an unquoted SQL identifier.
pub fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// For a `$` at `start`, the index of the `$` closing its dollar-quote tag (`$$` → `start + 1`,
/// `$body$` → `start + 5`), or `None` if this is not a dollar-quote opener.
///
/// Postgres tags must not begin with a digit, which is what keeps positional parameters (`$1`,
/// `$2`) from being mistaken for the start of a quoted block.
pub fn dollar_quote_tag_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start + 1;
    if matches!(chars.get(j), Some(c) if c.is_ascii_digit()) {
        return None;
    }
    while matches!(chars.get(j), Some(c) if is_ident_char(*c)) {
        j += 1;
    }
    (chars.get(j) == Some(&'$')).then_some(j)
}

/// If `chars[i]` opens a string literal, quoted identifier, dollar-quoted block, `--` line comment
/// or `/* */` block comment, returns the index just past its end. Otherwise `None`.
///
/// Unterminated constructs return `chars.len()` rather than `None`, so callers always make forward
/// progress and a malformed tail is treated as opaque instead of being scanned as code.
pub fn skip_non_code(chars: &[char], i: usize) -> Option<usize> {
    match chars[i] {
        '\'' => {
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == '\\' && j + 1 < chars.len() {
                    j += 2;
                    continue;
                }
                if chars[j] == '\'' {
                    // A doubled '' is an escaped quote, not the end of the literal.
                    if chars.get(j + 1) == Some(&'\'') {
                        j += 2;
                        continue;
                    }
                    return Some(j + 1);
                }
                j += 1;
            }
            Some(chars.len())
        }
        '"' => {
            let mut j = i + 1;
            while j < chars.len() {
                if chars[j] == '"' {
                    return Some(j + 1);
                }
                j += 1;
            }
            Some(chars.len())
        }
        '-' if chars.get(i + 1) == Some(&'-') => {
            let mut j = i;
            while j < chars.len() && chars[j] != '\n' {
                j += 1;
            }
            Some(j)
        }
        '/' if chars.get(i + 1) == Some(&'*') => {
            let mut j = i + 2;
            while j + 1 < chars.len() && !(chars[j] == '*' && chars[j + 1] == '/') {
                j += 1;
            }
            Some((j + 2).min(chars.len()))
        }
        '$' => {
            let tag_end = dollar_quote_tag_end(chars, i)?;
            let tag = &chars[i..=tag_end];
            let mut j = tag_end + 1;
            while j < chars.len() {
                if chars[j] == '$' && chars[j..].starts_with(tag) {
                    return Some(j + tag.len());
                }
                j += 1;
            }
            Some(chars.len())
        }
        _ => None,
    }
}

/// Byte-free `[start, end)` spans of each top-level statement in `chars`, each span running up to
/// and including its terminating `;` (the final span may be unterminated). Spans are contiguous
/// and cover the whole input, so reassembling them reproduces it exactly.
///
/// Boundaries come from [`skip_non_code`], so a `;` inside a dollar-quoted PL/pgSQL body, a string
/// literal or a comment never ends a statement — the guarantee a plain `split(';')` cannot make.
pub fn statement_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    let mut i = 0;

    while i < chars.len() {
        if let Some(next) = skip_non_code(chars, i) {
            i = next;
            continue;
        }
        if chars[i] == ';' {
            spans.push((start, i + 1));
            start = i + 1;
        }
        i += 1;
    }
    if start < chars.len() {
        spans.push((start, chars.len()));
    }
    spans
}

/// True if `word` occurs at `start` as a whole word — neither the character before it nor the one
/// after it is an identifier character. `case_sensitive` selects exact or ASCII-insensitive
/// comparison.
///
/// Hand-written schemas mix keyword case freely where `pg_dump` output never does, so callers
/// reading real-world SQL almost always want `case_sensitive: false`.
pub fn is_word_at(chars: &[char], start: usize, word: &str, case_sensitive: bool) -> bool {
    let wchars: Vec<char> = word.chars().collect();
    let end = start + wchars.len();
    if end > chars.len() {
        return false;
    }
    let matches = chars[start..end].iter().zip(wchars.iter()).all(|(a, b)| {
        if case_sensitive {
            a == b
        } else {
            a.eq_ignore_ascii_case(b)
        }
    });
    if !matches {
        return false;
    }
    let prev_is_ident = start > 0 && is_ident_char(chars[start - 1]);
    let next_is_ident = end < chars.len() && is_ident_char(chars[end]);
    !prev_is_ident && !next_is_ident
}

/// Index of the first character of real code at or after `from`, skipping whitespace and both
/// comment forms. Returns `chars.len()` if only whitespace and comments remain.
///
/// Statement classification must go through this. Reading a statement's first characters directly
/// misclassifies every statement carrying a `--` banner, which in a hand-maintained schema is most
/// of them.
pub fn skip_trivia(chars: &[char], from: usize) -> usize {
    let mut i = from;
    loop {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            return chars.len();
        }
        let is_comment = (chars[i] == '-' && chars.get(i + 1) == Some(&'-'))
            || (chars[i] == '/' && chars.get(i + 1) == Some(&'*'));
        if !is_comment {
            return i;
        }
        match skip_non_code(chars, i) {
            Some(next) if next > i => i = next,
            _ => return i,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cv(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn skips_single_quoted_literal_with_doubled_escape() {
        let c = cv("'it''s' rest");
        assert_eq!(skip_non_code(&c, 0), Some(7));
    }

    #[test]
    fn skips_dollar_quoted_block_containing_semicolons() {
        let c = cv("$$ a; b; c $$ rest");
        assert_eq!(skip_non_code(&c, 0), Some(13));
    }

    #[test]
    fn skips_tagged_dollar_quote() {
        let c = cv("$body$ x; $body$rest");
        assert_eq!(skip_non_code(&c, 0), Some(16));
    }

    #[test]
    fn positional_parameter_is_not_a_dollar_quote() {
        assert_eq!(dollar_quote_tag_end(&cv("$1"), 0), None);
        assert_eq!(dollar_quote_tag_end(&cv("$$"), 0), Some(1));
        assert_eq!(dollar_quote_tag_end(&cv("$body$"), 0), Some(5));
    }

    #[test]
    fn unterminated_constructs_still_make_progress() {
        assert_eq!(skip_non_code(&cv("'abc"), 0), Some(4));
        assert_eq!(skip_non_code(&cv("$$abc"), 0), Some(5));
    }

    #[test]
    fn statement_spans_ignore_semicolons_inside_dollar_quotes() {
        let sql = "CREATE TABLE a (id INT); COMMENT ON TABLE a IS $$x; y$$; SELECT 1;";
        let c = cv(sql);
        let spans = statement_spans(&c);
        assert_eq!(spans.len(), 3, "got: {spans:?}");
        let rendered: Vec<String> = spans
            .iter()
            .map(|(s, e)| c[*s..*e].iter().collect())
            .collect();
        assert!(rendered[1].contains("$$x; y$$"), "got {rendered:?}");
    }

    #[test]
    fn statement_spans_reassemble_to_the_original() {
        let sql = "SELECT 1; -- note; not a boundary\nSELECT 2";
        let c = cv(sql);
        let joined: String = statement_spans(&c)
            .iter()
            .flat_map(|(s, e)| c[*s..*e].iter())
            .collect();
        assert_eq!(joined, sql);
    }

    #[test]
    fn is_word_at_respects_boundaries_and_case() {
        let c = cv("INHERITS (x)");
        assert!(is_word_at(&c, 0, "INHERITS", true));
        let c = cv("inherits (x)");
        assert!(!is_word_at(&c, 0, "INHERITS", true));
        assert!(is_word_at(&c, 0, "INHERITS", false));
        let c = cv("INHERITSX");
        assert!(!is_word_at(&c, 0, "INHERITS", false));
    }

    #[test]
    fn skip_trivia_passes_comment_banners() {
        let c = cv("--\n-- Name: t\n--\nCOMMENT ON TABLE a IS 'x'");
        let i = skip_trivia(&c, 0);
        let rest: String = c[i..].iter().collect();
        assert!(rest.starts_with("COMMENT ON"), "got {rest:?}");
    }

    #[test]
    fn skip_trivia_on_all_trivia_returns_end() {
        let c = cv("  -- only a comment\n  ");
        assert_eq!(skip_trivia(&c, 0), c.len());
    }
}

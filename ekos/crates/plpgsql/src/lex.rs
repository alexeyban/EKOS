//! RFC 0163 — lexing, and dollar-quoting in particular.
//!
//! This has to be right before anything else can be, because everything downstream depends on
//! knowing where a function body actually ends. `sqlparser` does not solve it: to that parser a
//! `CREATE FUNCTION … AS $$ … $$` body is a single opaque string literal, which is exactly why
//! `sql_transform_analyzer.rs` produces `Unmapped` fragments and why an anti-invention check over
//! them passes on everything.
//!
//! The rules PostgreSQL actually applies, each of which breaks a naive scan:
//!
//! - A dollar quote is `$tag$ … $tag$`, where `tag` is empty or an identifier. `$$ … $$` and
//!   `$body$ … $body$` are both common.
//! - Dollar quotes **nest** when the tags differ: `$outer$ … $inner$ … $inner$ … $outer$`.
//! - `$1` is a parameter, not the start of a quote — a tag must be an identifier, and a digit
//!   cannot start one.
//! - A dollar quote inside a single-quoted string is *text*, not a quote.
//! - And inside a `--` or `/* */` comment, likewise.

/// A lexical token, enough for the statement parser to work with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// A bare word: a keyword or an identifier. Compared case-insensitively by the parser.
    Word(String),
    /// A single-quoted string literal, with the quotes stripped and `''` unescaped.
    Str(String),
    Num(String),
    /// A dollar-quoted block, with the delimiters stripped. The tag is kept because a nested body
    /// needs it to be re-emitted faithfully.
    Dollar {
        tag: String,
        body: String,
    },
    /// `$1`, `$2` — a parameter reference.
    Param(u32),
    Punct(char),
    /// `:=`, `..`, `||`, `<=`, `>=`, `<>`, `!=`
    Op(String),
}

/// A token and where it came from, so every recovered statement can cite its source text exactly —
/// the same discipline RFC 0150 applies with IL offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LexError {
    #[error("unterminated string literal starting at byte {at}")]
    UnterminatedString { at: usize },
    #[error("unterminated dollar-quoted block `${tag}$` starting at byte {at}")]
    UnterminatedDollar { tag: String, at: usize },
    #[error("unterminated block comment starting at byte {at}")]
    UnterminatedComment { at: usize },
}

/// Read a dollar-quote delimiter at `i`, returning its tag and the offset just past it.
///
/// `None` when this `$` does not open one: `$1` is a parameter, and `$` followed by anything that
/// cannot start an identifier is punctuation.
fn dollar_tag(s: &[u8], i: usize) -> Option<(String, usize)> {
    debug_assert_eq!(s[i], b'$');
    let mut j = i + 1;
    while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_') {
        // A tag is an identifier: it may contain digits but may not start with one. `$1` is a
        // parameter and `$1$` is not a valid delimiter.
        if j == i + 1 && s[j].is_ascii_digit() {
            return None;
        }
        j += 1;
    }
    if j < s.len() && s[j] == b'$' {
        Some((String::from_utf8_lossy(&s[i + 1..j]).into_owned(), j + 1))
    } else {
        None
    }
}

/// Public probe for a dollar-quote delimiter, so the statement splitter can skip a body without
/// re-lexing it.
pub fn probe_dollar(src: &str, i: usize) -> Option<(String, usize)> {
    let s = src.as_bytes();
    if i >= s.len() || s[i] != b'$' {
        return None;
    }
    dollar_tag(s, i)
}

/// Tokenize PL/pgSQL source.
pub fn lex(src: &str) -> Result<Vec<Spanned>, LexError> {
    let s = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < s.len() {
        let c = s[i];

        // Whitespace.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment.
        if c == b'-' && i + 1 < s.len() && s[i + 1] == b'-' {
            while i < s.len() && s[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Block comment. PostgreSQL's nest, unlike C's.
        if c == b'/' && i + 1 < s.len() && s[i + 1] == b'*' {
            let start = i;
            let mut depth = 1;
            i += 2;
            while i < s.len() && depth > 0 {
                if s[i] == b'/' && i + 1 < s.len() && s[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if s[i] == b'*' && i + 1 < s.len() && s[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                return Err(LexError::UnterminatedComment { at: start });
            }
            continue;
        }

        // String literal. `''` is an escaped quote, not a terminator followed by an opener.
        if c == b'\'' {
            let start = i;
            let mut value = String::new();
            i += 1;
            loop {
                if i >= s.len() {
                    return Err(LexError::UnterminatedString { at: start });
                }
                if s[i] == b'\'' {
                    if i + 1 < s.len() && s[i + 1] == b'\'' {
                        value.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                value.push(s[i] as char);
                i += 1;
            }
            out.push(Spanned {
                tok: Tok::Str(value),
                start,
                end: i,
            });
            continue;
        }

        // Dollar quote or parameter.
        if c == b'$' {
            let start = i;
            if let Some((tag, body_start)) = dollar_tag(s, i) {
                let close = format!("${tag}$");
                let rest = &src[body_start..];
                let Some(rel) = rest.find(&close) else {
                    return Err(LexError::UnterminatedDollar { tag, at: start });
                };
                let body = rest[..rel].to_string();
                i = body_start + rel + close.len();
                out.push(Spanned {
                    tok: Tok::Dollar { tag, body },
                    start,
                    end: i,
                });
                continue;
            }
            // `$1` — a parameter.
            let mut j = i + 1;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 {
                let n = src[i + 1..j].parse().unwrap_or(0);
                out.push(Spanned {
                    tok: Tok::Param(n),
                    start: i,
                    end: j,
                });
                i = j;
                continue;
            }
            out.push(Spanned {
                tok: Tok::Punct('$'),
                start: i,
                end: i + 1,
            });
            i += 1;
            continue;
        }

        // Quoted identifier — kept as a word, with the quotes stripped.
        if c == b'"' {
            let start = i;
            let mut value = String::new();
            i += 1;
            loop {
                if i >= s.len() {
                    return Err(LexError::UnterminatedString { at: start });
                }
                if s[i] == b'"' {
                    if i + 1 < s.len() && s[i + 1] == b'"' {
                        value.push('"');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                value.push(s[i] as char);
                i += 1;
            }
            out.push(Spanned {
                tok: Tok::Word(value),
                start,
                end: i,
            });
            continue;
        }

        // Word.
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == b'_') {
                i += 1;
            }
            out.push(Spanned {
                tok: Tok::Word(src[start..i].to_string()),
                start,
                end: i,
            });
            continue;
        }

        // Number.
        if c.is_ascii_digit() {
            let start = i;
            while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'.') {
                i += 1;
            }
            out.push(Spanned {
                tok: Tok::Num(src[start..i].to_string()),
                start,
                end: i,
            });
            continue;
        }

        // Multi-character operators.
        let two = if i + 1 < s.len() { &src[i..i + 2] } else { "" };
        if matches!(two, ":=" | ".." | "||" | "<=" | ">=" | "<>" | "!=") {
            out.push(Spanned {
                tok: Tok::Op(two.to_string()),
                start: i,
                end: i + 2,
            });
            i += 2;
            continue;
        }

        out.push(Spanned {
            tok: Tok::Punct(c as char),
            start: i,
            end: i + 1,
        });
        i += 1;
    }
    Ok(out)
}

/// Extract the body of `CREATE FUNCTION … AS $tag$ … $tag$`.
///
/// Returns the body and its byte span in `src`, so every statement the parser recovers can cite an
/// offset into the original text.
pub fn function_body(src: &str) -> Result<Option<(String, usize, usize)>, LexError> {
    for t in lex(src)? {
        if let Tok::Dollar { tag, body } = t.tok {
            let open = tag.len() + 2;
            return Ok(Some((body, t.start + open, t.end - open)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dollars(src: &str) -> Vec<(String, String)> {
        lex(src)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t.tok {
                Tok::Dollar { tag, body } => Some((tag, body)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_plain_and_tagged_forms_both_lex() {
        assert_eq!(dollars("$$ hello $$"), vec![("".into(), " hello ".into())]);
        assert_eq!(
            dollars("$body$ hello $body$"),
            vec![("body".into(), " hello ".into())]
        );
    }

    /// The rule a naive scan gets wrong: quotes nest when the tags differ.
    #[test]
    fn dollar_quotes_nest_when_the_tags_differ() {
        let d = dollars("$outer$ a $inner$ b $inner$ c $outer$");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, "outer");
        assert_eq!(d[0].1, " a $inner$ b $inner$ c ");
    }

    /// `$1` is a parameter. A scanner treating every `$` as a delimiter swallows the rest of the
    /// function.
    #[test]
    fn a_parameter_is_not_a_dollar_quote() {
        let toks = lex("SELECT $1 + $2").unwrap();
        let params: Vec<u32> = toks
            .iter()
            .filter_map(|t| match t.tok {
                Tok::Param(n) => Some(n),
                _ => None,
            })
            .collect();
        assert_eq!(params, vec![1, 2]);
        assert!(dollars("SELECT $1 + $2").is_empty());
    }

    /// A tag may contain digits but may not start with one.
    #[test]
    fn a_tag_may_not_start_with_a_digit() {
        assert!(dollars("$1$ x $1$").is_empty(), "$1 is a parameter");
        assert_eq!(dollars("$a1$ x $a1$").len(), 1);
    }

    #[test]
    fn a_dollar_quote_inside_a_string_is_text() {
        let toks = lex("SELECT '$$ not a quote $$' AS s").unwrap();
        assert!(
            toks.iter().all(|t| !matches!(t.tok, Tok::Dollar { .. })),
            "{toks:?}"
        );
        assert!(
            toks.iter()
                .any(|t| t.tok == Tok::Str("$$ not a quote $$".into()))
        );
    }

    #[test]
    fn a_dollar_quote_inside_a_comment_is_text() {
        for src in [
            "SELECT 1 -- $$ not a quote $$\nSELECT 2",
            "SELECT 1 /* $$ not a quote $$ */ SELECT 2",
        ] {
            assert!(dollars(src).is_empty(), "{src}");
        }
    }

    #[test]
    fn block_comments_nest() {
        let toks = lex("a /* outer /* inner */ still outer */ b").unwrap();
        let words: Vec<&str> = toks
            .iter()
            .filter_map(|t| match &t.tok {
                Tok::Word(w) => Some(w.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["a", "b"]);
    }

    #[test]
    fn a_doubled_quote_is_an_escaped_quote_not_two_strings() {
        let toks = lex("'it''s'").unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].tok, Tok::Str("it's".into()));
    }

    #[test]
    fn unterminated_constructs_are_errors_not_silent_truncation() {
        assert!(matches!(
            lex("'never closed"),
            Err(LexError::UnterminatedString { .. })
        ));
        assert!(matches!(
            lex("$body$ never closed"),
            Err(LexError::UnterminatedDollar { .. })
        ));
        assert!(matches!(
            lex("/* never closed"),
            Err(LexError::UnterminatedComment { .. })
        ));
    }

    #[test]
    fn assignment_and_range_operators_lex_as_one_token() {
        let toks = lex("i := 1 .. 10").unwrap();
        assert!(toks.iter().any(|t| t.tok == Tok::Op(":=".into())));
        assert!(toks.iter().any(|t| t.tok == Tok::Op("..".into())));
    }

    #[test]
    fn spans_point_back_at_the_source() {
        let src = "  hello  ";
        let toks = lex(src).unwrap();
        assert_eq!(&src[toks[0].start..toks[0].end], "hello");
    }

    #[test]
    fn a_function_body_is_extracted_with_its_span() {
        let src = "CREATE FUNCTION f() RETURNS int LANGUAGE plpgsql AS $$ BEGIN RETURN 1; END $$";
        let (body, start, end) = function_body(src).unwrap().unwrap();
        assert_eq!(body, " BEGIN RETURN 1; END ");
        assert_eq!(
            &src[start..end],
            body,
            "the span must locate the body exactly"
        );
    }

    #[test]
    fn a_function_with_no_dollar_body_yields_none() {
        assert_eq!(
            function_body("CREATE FUNCTION f() RETURNS int AS 'SELECT 1'").unwrap(),
            None
        );
    }

    #[test]
    fn quoted_identifiers_become_words_with_the_quotes_stripped() {
        let toks = lex("\"MixedCase\"").unwrap();
        assert_eq!(toks[0].tok, Tok::Word("MixedCase".into()));
    }
}

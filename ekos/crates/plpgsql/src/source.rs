//! RFC 0163 — finding routines in a SQL file.
//!
//! A schema file holds many statements; the routines among them are the ones this crate parses.
//! Splitting happens on the crate's own lexer tokens, so a semicolon inside a body, a string or a
//! comment never splits a routine — and each routine keeps its byte offset in the file, so a span
//! inside it can be turned into a file offset and a line number.

use crate::lex::{LexError, Tok, lex};

/// One `CREATE [OR REPLACE] FUNCTION|PROCEDURE` statement, as written in its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineSource<'a> {
    /// The statement text, from `CREATE` through its terminating `;` (or the end of the file).
    pub text: &'a str,
    /// Byte offset of `text` in the file. A span `s` inside the routine sits at `offset + s.start`.
    pub offset: usize,
}

/// Every routine definition in `src`, in file order.
///
/// Fails only when the file does not lex at all — an unterminated string, dollar quote or block
/// comment — because past that point no statement boundary can be trusted.
pub fn routines(src: &str) -> Result<Vec<RoutineSource<'_>>, LexError> {
    let toks = lex(src)?;
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut push = |from: usize, to: usize| {
        let text = &src[from..to];
        let head: Vec<String> = text
            .split_whitespace()
            .take(4)
            .map(str::to_ascii_uppercase)
            .collect();
        let head: Vec<&str> = head.iter().map(String::as_str).collect();
        if matches!(
            head.as_slice(),
            ["CREATE", "FUNCTION" | "PROCEDURE", ..]
                | ["CREATE", "OR", "REPLACE", "FUNCTION" | "PROCEDURE"]
        ) {
            out.push(RoutineSource { text, offset: from });
        }
    };
    for t in &toks {
        let s = *start.get_or_insert(t.start);
        if t.tok == Tok::Punct(';') {
            start = None;
            push(s, t.end);
        }
    }
    if let (Some(s), Some(last)) = (start, toks.last()) {
        push(s, last.end);
    }
    Ok(out)
}

/// 1-based line number of byte offset `at` in `src`.
pub fn line_of(src: &str, at: usize) -> u32 {
    let at = at.min(src.len());
    1 + src.as_bytes()[..at].iter().filter(|&&b| b == b'\n').count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routines_are_found_with_their_offsets_and_nothing_else_is() {
        let src = "CREATE TABLE t (id int);\n\
                   -- a comment; with a semicolon\n\
                   CREATE OR REPLACE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql;\n\
                   COMMENT ON FUNCTION f() IS $$ CREATE FUNCTION g(); $$;\n\
                   create procedure p() language sql as 'select 1'";
        let r = routines(src).unwrap();
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].text.starts_with("CREATE OR REPLACE FUNCTION f()"));
        assert!(r[0].text.ends_with("LANGUAGE plpgsql;"));
        assert_eq!(&src[r[0].offset..r[0].offset + r[0].text.len()], r[0].text);
        assert_eq!(line_of(src, r[0].offset), 3);
        // The last statement has no `;` and still counts.
        assert!(r[1].text.starts_with("create procedure p()"));
        assert_eq!(line_of(src, r[1].offset), 5);
    }
}

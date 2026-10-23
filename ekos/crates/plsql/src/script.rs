//! Oracle script scanning: where each statement, PL/SQL unit and SQL\*Plus command in a `.sql`
//! file starts and ends.
//!
//! An Oracle script is not one grammar. SQL statements end at `;`; a PL/SQL unit (`CREATE
//! FUNCTION`, an anonymous `BEGIN … END;`) contains `;` everywhere and ends at a `/` alone on a
//! line; SQL\*Plus commands (`SET DEFINE OFF`, `PROMPT …`, `@file`) end at the line break and may
//! hold an unbalanced apostrophe. Splitting on `;` alone cuts every unit into fragments, which is
//! what the ANSI baseline did to iDempiere's migration scripts (RFC 0210 §1).
//!
//! Quoting follows Oracle, not PostgreSQL: a backslash is an ordinary character, a doubled `''`
//! is an escaped quote, and `q'[…]'` (any delimiter) is a literal that may contain bare quotes.
//! Everything works on bytes: every delimiter is ASCII, so a byte index never splits a character
//! that matters.

/// What a [`Unit`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    /// A SQL statement, ending after its `;` or before a `/` line.
    Statement,
    /// A PL/SQL unit: `CREATE [OR REPLACE] FUNCTION | PROCEDURE | TRIGGER | PACKAGE | TYPE`, or an
    /// anonymous block. Ends before its `/` line.
    PlSql,
    /// A SQL\*Plus command, a `/` line or an `@file` line: the whole line.
    SqlPlus,
}

/// One unit of a script, as byte offsets `[start, end)` into the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    pub kind: UnitKind,
    pub start: usize,
    pub end: usize,
    /// For a PL/SQL unit: whether a `/` line ended it. `false` means it ran to the end of the
    /// file, which SQL\*Plus would also have read as part of the unit.
    pub terminated: bool,
}

/// SQL\*Plus commands. `SET` is also SQL (`SET TRANSACTION`, `SET ROLE`, `SET CONSTRAINTS`), so
/// it is checked separately.
const SQLPLUS_COMMANDS: &[&str] = &[
    "PROMPT",
    "SPOOL",
    "WHENEVER",
    "EXIT",
    "QUIT",
    "REM",
    "REMARK",
    "DEFINE",
    "UNDEFINE",
    "SHOW",
    "EXEC",
    "EXECUTE",
    "PAUSE",
    "CONNECT",
    "CONN",
    "DISCONNECT",
    "TTITLE",
    "BTITLE",
    "BREAK",
    "COMPUTE",
    "CLEAR",
    "VARIABLE",
    "VAR",
    "PRINT",
    "ACCEPT",
    "HOST",
    "DESC",
    "DESCRIBE",
];

/// What follows `CREATE [OR REPLACE] [EDITIONABLE]` in a PL/SQL unit.
const PLSQL_OBJECTS: &[&str] = &[
    "FUNCTION",
    "PROCEDURE",
    "TRIGGER",
    "PACKAGE",
    "TYPE",
    "JAVA",
];

/// True for a byte that may appear in an unquoted Oracle identifier.
pub fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'#'
}

/// If `b[i]` opens a string literal, a quoted identifier or a comment, the index just past it.
/// Unterminated constructs end at the end of input, so a caller always makes progress.
pub fn skip_non_code(b: &[u8], i: usize) -> Option<usize> {
    match b[i] {
        b'\'' => Some(literal_end(b, i)),
        b'"' => Some(find(b, i + 1, |j| b[j] == b'"').map_or(b.len(), |j| j + 1)),
        b'-' if b.get(i + 1) == Some(&b'-') => Some(line_end(b, i)),
        b'/' if b.get(i + 1) == Some(&b'*') => Some(
            find(b, i + 2, |j| b[j] == b'*' && b.get(j + 1) == Some(&b'/'))
                .map_or(b.len(), |j| j + 2),
        ),
        _ => q_quote_at(b, i).map(|(_, end)| end),
    }
}

/// An alternative-quoted literal `q'X…X'` (or `nq'…'`) starting at `i`: the index of its `q` and
/// the index just past its closing quote.
fn q_quote_at(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let q = match b[i] {
        b'q' | b'Q' => i,
        b'n' | b'N' if matches!(b.get(i + 1), Some(b'q' | b'Q')) => i + 1,
        _ => return None,
    };
    if i > 0 && is_ident_byte(b[i - 1]) {
        return None;
    }
    if b.get(q + 1) != Some(&b'\'') {
        return None;
    }
    let open = *b.get(q + 2)?;
    if open.is_ascii_whitespace() {
        return None;
    }
    let close = match open {
        b'[' => b']',
        b'(' => b')',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    };
    let end = find(b, q + 3, |j| b[j] == close && b.get(j + 1) == Some(&b'\''))
        .map_or(b.len(), |j| j + 2);
    Some((q, end))
}

/// The index just past the `'…'` literal opening at `open`; `''` inside is an escaped quote.
fn literal_end(b: &[u8], open: usize) -> usize {
    let mut j = open + 1;
    while j < b.len() {
        if b[j] != b'\'' {
            j += 1;
        } else if b.get(j + 1) == Some(&b'\'') {
            j += 2;
        } else {
            return j + 1;
        }
    }
    b.len()
}

fn find(b: &[u8], from: usize, stop: impl Fn(usize) -> bool) -> Option<usize> {
    (from..b.len()).find(|&j| stop(j))
}

/// The index of the line break ending the line that holds `i` (or the end of input).
pub fn line_end(b: &[u8], i: usize) -> usize {
    find(b, i, |j| b[j] == b'\n').unwrap_or(b.len())
}

fn line_start(b: &[u8], i: usize) -> usize {
    b[..i]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1)
}

/// Whether the line starting at `start` holds only a `/` (and whitespace).
fn is_slash_line(b: &[u8], start: usize) -> bool {
    let line = &b[start..line_end(b, start)];
    let t: Vec<u8> = line
        .iter()
        .copied()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    t == b"/"
}

/// The first byte at or after `from` that is not whitespace or a comment.
pub fn skip_trivia(b: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let comment = b[i..].starts_with(b"--") || b[i..].starts_with(b"/*");
        if !comment {
            return i;
        }
        i = skip_non_code(b, i).unwrap_or(i + 1);
    }
    b.len()
}

/// The upper-cased word at `i` and the index after it, if an identifier starts there.
pub fn word_at(b: &[u8], i: usize) -> Option<(String, usize)> {
    if i >= b.len() || !is_ident_byte(b[i]) {
        return None;
    }
    let end = find(b, i, |j| !is_ident_byte(b[j])).unwrap_or(b.len());
    Some((
        String::from_utf8_lossy(&b[i..end]).to_ascii_uppercase(),
        end,
    ))
}

/// The first `n` words from `i`, skipping comments between them. Stops at anything else.
fn leading_words(b: &[u8], i: usize, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = i;
    while out.len() < n {
        at = skip_trivia(b, at);
        let Some((w, end)) = word_at(b, at) else {
            break;
        };
        out.push(w);
        at = end;
    }
    out
}

fn is_sqlplus_command(b: &[u8], i: usize) -> bool {
    if b[i] == b'@' {
        return true;
    }
    let words = leading_words(b, i, 2);
    match words.first().map(String::as_str) {
        Some("SET") => !matches!(
            words.get(1).map(String::as_str),
            Some("TRANSACTION" | "ROLE" | "CONSTRAINT" | "CONSTRAINTS")
        ),
        Some(w) => SQLPLUS_COMMANDS.contains(&w),
        None => false,
    }
}

/// Whether a PL/SQL unit starts at `i`.
pub fn is_plsql_start(b: &[u8], i: usize) -> bool {
    let words = leading_words(b, i, 5);
    let mut w = words.iter().map(String::as_str);
    match w.next() {
        Some("DECLARE" | "BEGIN") => true,
        Some("CREATE") => {
            let mut next = w.next();
            if next == Some("OR") {
                w.next(); // REPLACE
                next = w.next();
            }
            if matches!(next, Some("EDITIONABLE" | "NONEDITIONABLE" | "AND")) {
                next = w.next();
            }
            next.is_some_and(|x| PLSQL_OBJECTS.contains(&x))
        }
        _ => false,
    }
}

/// From inside a PL/SQL unit at `from`: the start of the next `/` line in code, if any.
fn next_slash_line(b: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if let Some(next) = skip_non_code(b, i) {
            i = next;
            continue;
        }
        if b[i] == b'\n' && i + 1 < b.len() && is_slash_line(b, i + 1) {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Where a SQL statement starting at `from` ends: after its `;`, or before a `/` line.
fn statement_end(b: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < b.len() {
        if let Some(next) = skip_non_code(b, i) {
            i = next;
            continue;
        }
        if b[i] == b';' {
            return i + 1;
        }
        if b[i] == b'\n' && i + 1 < b.len() && is_slash_line(b, i + 1) {
            return i + 1;
        }
        i += 1;
    }
    b.len()
}

/// Every unit of an Oracle script, in order. Whitespace and comments between units belong to none.
pub fn units(src: &str) -> Vec<Unit> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        i = skip_trivia(b, i);
        if i >= b.len() {
            return out;
        }
        let unit = unit_at(b, i);
        i = unit.end.max(i + 1);
        out.push(unit);
    }
}

fn unit_at(b: &[u8], i: usize) -> Unit {
    let whole_line = |kind| Unit {
        kind,
        start: i,
        end: line_end(b, i),
        terminated: true,
    };
    let at_line_start = b[line_start(b, i)..i].iter().all(u8::is_ascii_whitespace);
    if at_line_start && is_slash_line(b, line_start(b, i)) {
        return whole_line(UnitKind::SqlPlus);
    }
    if is_sqlplus_command(b, i) {
        return whole_line(UnitKind::SqlPlus);
    }
    if is_plsql_start(b, i) {
        let slash = next_slash_line(b, i);
        return Unit {
            kind: UnitKind::PlSql,
            start: i,
            end: slash.unwrap_or(b.len()),
            terminated: slash.is_some(),
        };
    }
    Unit {
        kind: UnitKind::Statement,
        start: i,
        end: statement_end(b, i),
        terminated: true,
    }
}

/// `text` with Oracle-only quoting rewritten into what an ANSI scanner reads the same way, byte
/// for byte the same length: `q'[it's]'` becomes `  ' it s  '`, and a backslash inside a literal
/// becomes a space (ANSI scanners that honour `\'` would otherwise run past the literal's end).
///
/// Only the literals' contents change, never code, so a statement's structure is untouched.
pub fn neutralize_quotes(text: &str) -> String {
    let mut b = text.as_bytes().to_vec();
    let mut i = 0;
    while i < b.len() {
        if let Some((q, end)) = q_quote_at(&b, i) {
            blank_q_quote(&mut b, i, q, end);
            i = end;
            continue;
        }
        if b[i] == b'\'' {
            let end = literal_end(&b, i);
            for c in &mut b[i + 1..end.saturating_sub(1).max(i + 1)] {
                if *c == b'\\' {
                    *c = b' ';
                }
            }
            i = end;
            continue;
        }
        i = skip_non_code(&b, i).unwrap_or(i + 1);
    }
    String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// `[N]q'X…X'` at `start` → `  ' … '`: prefix and delimiters blanked, inner quotes and
/// backslashes blanked, the outer quotes kept.
fn blank_q_quote(b: &mut [u8], start: usize, q: usize, end: usize) {
    for c in &mut b[start..=q] {
        *c = b' ';
    }
    let inner_start = q + 2;
    let close_quote = end.saturating_sub(1);
    for (j, c) in b.iter_mut().enumerate().take(close_quote).skip(inner_start) {
        let delimiter = j == inner_start || j + 1 == close_quote;
        if delimiter || *c == b'\'' || *c == b'\\' {
            *c = b' ';
        }
    }
}

/// `text` with every `--` and `/* */` comment blanked (line breaks kept), by Oracle's rules:
/// block comments do **not** nest. PostgreSQL's do, so a PL/pgSQL scanner reads iDempiere's
/// `/** … ELSE … END IF; /**/` as an unclosed comment and loses the rest of the routine.
pub fn mask_comments(text: &str) -> String {
    let mut b = text.as_bytes().to_vec();
    let mut i = 0;
    while i < b.len() {
        let Some(end) = skip_non_code(&b, i) else {
            i += 1;
            continue;
        };
        let comment = b[i..].starts_with(b"--") || b[i..].starts_with(b"/*");
        if comment {
            for c in &mut b[i..end] {
                if *c != b'\n' && *c != b'\r' {
                    *c = b' ';
                }
            }
        }
        i = end;
    }
    String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// One code token of a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokKind {
    /// An unquoted identifier or keyword, upper-cased.
    Word(String),
    /// A `"quoted"` identifier, as written between the quotes.
    Quoted(String),
    /// A string literal.
    Str,
    /// A number.
    Num,
    /// Any other single character.
    Sym(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tok {
    pub kind: TokKind,
    pub start: usize,
    pub end: usize,
}

impl Tok {
    /// Whether this token is the keyword `w` (upper case).
    pub fn is(&self, w: &str) -> bool {
        matches!(&self.kind, TokKind::Word(x) if x == w)
    }

    pub fn is_sym(&self, c: u8) -> bool {
        self.kind == TokKind::Sym(c)
    }

    /// The identifier this token names, if it is one (upper-cased when unquoted).
    pub fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokKind::Word(w) | TokKind::Quoted(w) => Some(w),
            _ => None,
        }
    }
}

/// The code tokens of `text`: comments dropped, literals kept as one token each.
pub fn tokens(text: &str) -> Vec<Tok> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let (kind, end) = token_at(b, i);
        if let Some(kind) = kind {
            out.push(Tok {
                kind,
                start: i,
                end,
            });
        }
        i = end.max(i + 1);
    }
    out
}

fn token_at(b: &[u8], i: usize) -> (Option<TokKind>, usize) {
    let c = b[i];
    if c.is_ascii_whitespace() {
        return (None, i + 1);
    }
    if let Some(end) = skip_non_code(b, i) {
        let kind = match c {
            b'"' => Some(TokKind::Quoted(
                String::from_utf8_lossy(&b[i + 1..end.saturating_sub(1).max(i + 1)]).into_owned(),
            )),
            b'-' | b'/' => None,
            _ => Some(TokKind::Str),
        };
        return (kind, end);
    }
    if c.is_ascii_digit() {
        let end =
            find(b, i, |j| !(b[j].is_ascii_alphanumeric() || b[j] == b'.')).unwrap_or(b.len());
        return (Some(TokKind::Num), end);
    }
    if let Some((w, end)) = word_at(b, i) {
        return (Some(TokKind::Word(w)), end);
    }
    (Some(TokKind::Sym(c)), i + 1)
}

/// For an opening `(` at token index `open`, the index of its matching `)`.
pub fn matching_paren(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, t) in toks.iter().enumerate().skip(open) {
        if t.is_sym(b'(') {
            depth += 1;
        } else if t.is_sym(b')') {
            depth -= 1;
            if depth == 0 {
                return Some(j);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(UnitKind, String)> {
        units(src)
            .into_iter()
            .map(|u| (u.kind, src[u.start..u.end].trim().to_string()))
            .collect()
    }

    #[test]
    fn sqlplus_lines_statements_and_units_are_told_apart() {
        let src = "SET SQLBLANKLINES ON\nSET DEFINE OFF\n\n-- a comment\nINSERT INTO t VALUES (1)\n;\n\
                   CREATE OR REPLACE FUNCTION f RETURN NUMBER AS\nBEGIN\n  RETURN 1;\nEND;\n/\n\
                   UPDATE t SET a = 2\n/\nSELECT 1 FROM dual;\n";
        let got = kinds(src);
        assert_eq!(got[0], (UnitKind::SqlPlus, "SET SQLBLANKLINES ON".into()));
        assert_eq!(got[1], (UnitKind::SqlPlus, "SET DEFINE OFF".into()));
        assert_eq!(
            got[2],
            (UnitKind::Statement, "INSERT INTO t VALUES (1)\n;".into())
        );
        assert_eq!(got[3].0, UnitKind::PlSql);
        assert!(got[3].1.ends_with("END;"), "{:?}", got[3]);
        assert_eq!(got[4], (UnitKind::SqlPlus, "/".into()));
        assert_eq!(got[5], (UnitKind::Statement, "UPDATE t SET a = 2".into()));
        assert_eq!(got[6], (UnitKind::SqlPlus, "/".into()));
        assert_eq!(got[7], (UnitKind::Statement, "SELECT 1 FROM dual;".into()));
        assert_eq!(got.len(), 8);
    }

    #[test]
    fn set_transaction_is_sql_not_a_sqlplus_command() {
        let got = kinds("SET TRANSACTION READ ONLY;\n");
        assert_eq!(got[0].0, UnitKind::Statement);
    }

    #[test]
    fn a_prompt_line_may_hold_an_unbalanced_quote() {
        let got = kinds("PROMPT it's done\nSELECT 'x' FROM dual;\n");
        assert_eq!(got[0], (UnitKind::SqlPlus, "PROMPT it's done".into()));
        assert_eq!(
            got[1],
            (UnitKind::Statement, "SELECT 'x' FROM dual;".into())
        );
    }

    #[test]
    fn a_unit_without_a_slash_runs_to_the_end_of_the_file() {
        let u = units("BEGIN\n  NULL;\nEND;\nSELECT 1 FROM dual;\n");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].kind, UnitKind::PlSql);
        assert!(!u[0].terminated);
    }

    #[test]
    fn a_slash_inside_a_comment_or_literal_does_not_end_a_unit() {
        let src = "BEGIN\n/*\n/\n*/\n  x := '\n/\n';\nEND;\n/\n";
        let u = units(src);
        assert_eq!(u[0].kind, UnitKind::PlSql);
        assert!(u[0].terminated);
        assert!(src[u[0].start..u[0].end].trim_end().ends_with("END;"));
    }

    #[test]
    fn q_quotes_and_backslashes_are_neutralized_without_moving_anything() {
        let src = "UPDATE t SET a = q'[it's; done]', b = 'C:\\' WHERE c = nq'{x}'";
        let n = neutralize_quotes(src);
        assert_eq!(n.len(), src.len());
        assert_eq!(
            n,
            "UPDATE t SET a =  ' it s; done ', b = 'C: ' WHERE c =   ' x '"
        );
    }

    #[test]
    fn a_q_quote_keeps_a_semicolon_inside_one_statement() {
        let u = units("UPDATE t SET a = q'[x; y]';\nSELECT 1 FROM dual;");
        assert_eq!(u.len(), 2);
    }

    #[test]
    fn backslash_is_not_an_escape() {
        let u = units("SELECT 'a\\' FROM dual;\nSELECT 2 FROM dual;");
        assert_eq!(u.len(), 2);
    }

    #[test]
    fn plsql_starts_are_recognised() {
        for s in [
            "CREATE OR REPLACE FUNCTION f",
            "create procedure p",
            "CREATE OR REPLACE EDITIONABLE TRIGGER t",
            "CREATE OR REPLACE PACKAGE BODY p",
            "CREATE TYPE t AS OBJECT",
            "DECLARE x NUMBER;",
            "BEGIN NULL; END;",
        ] {
            assert!(is_plsql_start(s.as_bytes(), 0), "{s}");
        }
        for s in [
            "CREATE TABLE t (a NUMBER)",
            "CREATE OR REPLACE VIEW v AS SELECT 1 FROM dual",
        ] {
            assert!(!is_plsql_start(s.as_bytes(), 0), "{s}");
        }
    }

    #[test]
    fn block_comments_do_not_nest() {
        let src = "a /** x\n /**/ b -- c\n'--not' d";
        let m = mask_comments(src);
        assert_eq!(m.len(), src.len());
        assert_eq!(m, "a      \n      b     \n'--not' d");
    }

    #[test]
    fn tokens_keep_quoted_identifiers_and_drop_comments() {
        let t = tokens("ALTER TABLE \"My Tab\" -- c\n ADD x NUMBER(10)");
        let idents: Vec<_> = t.iter().filter_map(Tok::ident).collect();
        assert_eq!(idents, ["ALTER", "TABLE", "My Tab", "ADD", "X", "NUMBER"]);
        assert_eq!(t.iter().filter(|x| x.kind == TokKind::Num).count(), 1);
    }
}

//! RFC 0163 — the statement parser.
//!
//! Hand-written recursive descent. The grammar is small, stable and documented, and the
//! alternative — shelling out to `plpgsql_check` or a live server — is excluded by RFC 0147's
//! no-shell-out precedent and by the requirement that recovery be deterministic and offline.
//!
//! **Recovery is local.** A statement that cannot be parsed becomes exactly one
//! [`ProcStmt::Unrecovered`] with its span and reason, and the parser resynchronizes at the next
//! statement boundary. This is the direct fix for `sql_transform_analyzer.rs`'s current behaviour,
//! where one unparseable construct costs the whole routine and leaves partial, duplicate `Unmapped`
//! fragments behind.

use crate::ir::{
    CursorOp, ExceptionHandler, LoopKind, ProcSignature, ProcStmt, ProcedureIr, Span, VarDecl,
};

/// Where a body sits inside its original source, so spans point at the file rather than at the
/// extracted fragment.
#[derive(Debug, Clone, Copy, Default)]
pub struct Origin {
    pub offset: usize,
}

struct Parser<'a> {
    /// The comment-masked body. **Every fragment the parser handles is a subslice of this**, so a
    /// fragment's span is its address within it — computed, never searched for.
    ///
    /// Searching (`parent.find(fragment)`) was the old approach, and it was wrong in two ways at
    /// once: the parent it searched was often a re-trimmed copy whose start was not the offset it
    /// was paired with, and `find` returns the *first* match, so `IF a THEN x; ELSE x;` gave the
    /// ELSE branch the THEN branch's span. Both produced plausible offsets pointing at the wrong
    /// text — worse than an obviously broken span.
    src: &'a str,
    origin: Origin,
}

/// Split a body into statements at top-level semicolons.
///
/// Semicolons inside strings, dollar quotes, comments and parentheses do not end a statement, and
/// neither does the one closing a nested `END;` — nesting is tracked by keyword depth so a block's
/// interior stays with it. Each statement's span covers exactly its trimmed text.
pub fn split_statements(body: &str, origin: Origin) -> Vec<(String, Span)> {
    split_slices(body)
        .into_iter()
        .map(|s| {
            let at = s.as_ptr() as usize - body.as_ptr() as usize;
            (
                s.to_string(),
                Span {
                    start: origin.offset + at,
                    end: origin.offset + at + s.len(),
                },
            )
        })
        .collect()
}

/// [`split_statements`] as trimmed subslices of `body`.
fn split_slices(body: &str) -> Vec<&str> {
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut depth = 0i32;
    // The openers currently open, so an `END` knows what it closes (see [`end_tail_len`]).
    let mut open: Vec<&'static str> = Vec::new();
    let mut parens = 0i32;
    // `DECLARE` is not a block opener, but its semicolons still belong to the block: they separate
    // declarations, not statements. So it suppresses splitting until the `BEGIN` that does open the
    // block. Without this, removing DECLARE from the opener list trades one bug for another — the
    // declaration list gets split into free-standing statements that classify as nothing.
    let mut in_declare = false;

    // Byte comparison throughout. Slicing a `&str` at an arbitrary index panics on a multibyte
    // character, and a parser that panics on `¿` is not a parser that recovers locally — the first
    // non-ASCII identifier or comment in a real routine would take the process down.
    let keyword_at = |i: usize| -> Option<(&'static str, usize)> {
        // **`DECLARE` is not an opener.** It introduces a declaration list; `BEGIN` opens the
        // block and `END` closes it. Counting DECLARE as a level leaves the depth permanently one
        // too high, so the closing `END` never brings it back to zero and the entire routine reads
        // as one unterminated statement.
        for kw in [
            "BEGIN",
            "CASE",
            "LOOP",
            "IF",
            "END",
            "EXCEPTION",
            "ELSIF",
            "ELSE",
        ] {
            let n = kw.len();
            if i + n <= b.len()
                && b[i..i + n].eq_ignore_ascii_case(kw.as_bytes())
                && (i == 0 || !is_word_byte(b[i - 1]))
                && (i + n == b.len() || !is_word_byte(b[i + n]))
            {
                return Some((kw, n));
            }
        }
        None
    };

    while i < b.len() {
        let c = b[i];
        // Skip anything a semicolon can hide inside.
        if let Some(next) = skip_quoted(body, i) {
            i = next;
            continue;
        }
        if c == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        if c == b'(' {
            parens += 1;
        } else if c == b')' {
            parens -= 1;
        }

        if i + 7 <= b.len()
            && b[i..i + 7].eq_ignore_ascii_case(b"DECLARE")
            && (i == 0 || !is_word_byte(b[i - 1]))
            && (i + 7 == b.len() || !is_word_byte(b[i + 7]))
        {
            in_declare = true;
            i += 7;
            continue;
        }

        if let Some((kw, n)) = keyword_at(i)
            && (kw != "IF" || at_statement_start(b, i))
        {
            if kw == "BEGIN" {
                in_declare = false;
            }
            let mut consumed = n;
            match kw {
                "END" => {
                    depth -= 1;
                    // **`END IF` is one closer, not a close followed by an open.** Matching `END`
                    // and then re-matching the `IF` on the next pass decrements and immediately
                    // increments, so the depth never returns to zero and the whole routine reads
                    // as one unterminated statement. The trailing keyword is consumed here.
                    consumed = n + end_tail_len(b, i + n, open.pop());
                }
                "ELSIF" | "ELSE" | "EXCEPTION" => {}
                // `CASE` appears both as a statement and as an expression (`SELECT CASE WHEN …`);
                // an expression `CASE` still has a matching `END`, so counting both keeps the
                // depth balanced.
                _ => {
                    depth += 1;
                    open.push(kw);
                }
            }
            i += consumed;
            continue;
        }

        if c == b';' && depth <= 0 && parens <= 0 && !in_declare {
            let text = slice(body, start, i).trim();
            if !text.is_empty() {
                out.push(text);
            }
            start = i + 1;
        }
        i += 1;
    }
    let tail = slice(body, start.min(body.len()), body.len()).trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// After an `END` ending at `after`, the length of a trailing `IF`/`LOOP`/`CASE` (with the
/// whitespace before it) that belongs to the same closer, or 0.
///
/// `closing` is the opener this `END` closes. The tail belongs to the closer only when it names
/// that construct: a `CASE` expression's `END` followed by a loop's opening `LOOP` is two keywords,
/// not `END LOOP`. Matching any tail let the expression swallow the loop's opener and left the
/// whole loop unterminated. With no opener on record (an unbalanced fragment), any tail counts, as
/// it always did.
fn end_tail_len(b: &[u8], after: usize, closing: Option<&str>) -> usize {
    let skip = b[after..]
        .iter()
        .take_while(|c| c.is_ascii_whitespace())
        .count();
    for tail in ["IF", "LOOP", "CASE"] {
        let m = tail.len();
        let at = after + skip;
        if closing.is_none_or(|c| c == tail)
            && at + m <= b.len()
            && b[at..at + m].eq_ignore_ascii_case(tail.as_bytes())
            && (at + m == b.len() || !is_word_byte(b[at + m]))
        {
            return skip + m;
        }
    }
    0
}

/// If a quoted construct starts at byte `i`, the offset just past it.
///
/// A keyword or semicolon inside a string, a quoted identifier or a dollar quote is text. `E'…'`
/// strings treat a backslash as an escape, so `E'it\'s'` is one string, not an unterminated one.
fn skip_quoted(s: &str, i: usize) -> Option<usize> {
    let b = s.as_bytes();
    match b[i] {
        q @ (b'\'' | b'"') => {
            let escapes = q == b'\''
                && i > 0
                && matches!(b[i - 1], b'E' | b'e')
                && (i < 2 || !is_word_byte(b[i - 2]));
            let mut j = i + 1;
            while j < b.len() {
                if escapes && b[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if b[j] == q {
                    if b.get(j + 1) == Some(&q) {
                        j += 2;
                        continue;
                    }
                    return Some(j + 1);
                }
                j += 1;
            }
            Some(b.len())
        }
        b'$' => {
            let (tag, after) = crate::lex::probe_dollar(s, i)?;
            let close = format!("${tag}$");
            Some(
                s[after..]
                    .find(&close)
                    .map_or(s.len(), |r| after + r + close.len()),
            )
        }
        _ => None,
    }
}

/// Slice by byte offsets, snapped outward to the nearest char boundaries.
///
/// Every offset in this module is a byte offset, and a `&str` slice at a non-boundary panics. That
/// is unacceptable in a parser whose contract is local recovery: one `¿` in a comment must cost one
/// statement, not the process.
fn slice(s: &str, mut from: usize, mut to: usize) -> &str {
    while from < s.len() && !s.is_char_boundary(from) {
        from += 1;
    }
    while to < s.len() && !s.is_char_boundary(to) {
        to += 1;
    }
    let to = to.min(s.len());
    let from = from.min(to);
    &s[from..to]
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether byte `i` begins a statement: it is the first thing in the text, or follows `;`, a
/// `<<label>>`, or a keyword after which a statement list starts.
///
/// `IF` opens a block only here. `DROP TABLE IF EXISTS` and `ADD COLUMN IF NOT EXISTS` are DDL, and
/// counting their `IF` as an opener left the depth one too high: every statement after it was
/// swallowed into the DDL's SQL text, and the routine still read as fully recovered.
fn at_statement_start(b: &[u8], i: usize) -> bool {
    let mut j = i;
    while j > 0 && b[j - 1].is_ascii_whitespace() {
        j -= 1;
    }
    if j == 0 || matches!(b[j - 1], b';' | b'>') {
        return true;
    }
    let end = j;
    while j > 0 && is_word_byte(b[j - 1]) {
        j -= 1;
    }
    let word = &b[j..end];
    ["THEN", "ELSE", "LOOP", "BEGIN"]
        .iter()
        .any(|k| word.eq_ignore_ascii_case(k.as_bytes()))
}

/// The text after a leading keyword of `n` bytes. The keyword is ASCII by construction, so the
/// offset is a boundary — but the *slice* has to come from the trimmed string, and doing it in one
/// place is how the rest of this module stays free of index arithmetic. The result is a subslice
/// of `text`, so its span is still computable.
fn after_kw(text: &str, n: usize) -> &str {
    let t = text.trim_start();
    if t.len() <= n {
        return "";
    }
    t[n..].trim()
}

fn first_word(s: &str) -> String {
    s.split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_')
        .to_ascii_uppercase()
}

/// Byte comparison, not slicing: `t[..kw.len()]` panics when the text starts with a multibyte
/// character, and PL/pgSQL is full of comments and identifiers that do.
fn starts_with_kw(s: &str, kw: &str) -> bool {
    let t = s.trim_start();
    let b = t.as_bytes();
    let n = kw.len();
    b.len() >= n
        && b[..n].eq_ignore_ascii_case(kw.as_bytes())
        && b.get(n)
            .is_none_or(|c| !c.is_ascii_alphanumeric() && *c != b'_')
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, origin: Origin) -> Self {
        Self { src, origin }
    }

    /// Where `frag` — a subslice of `self.src` — sits in the original source.
    fn span_of(&self, frag: &str) -> Span {
        let base = self.src.as_ptr() as usize;
        let at = (frag.as_ptr() as usize)
            .checked_sub(base)
            .filter(|at| at + frag.len() <= self.src.len());
        debug_assert!(
            at.is_some(),
            "fragment is not a slice of the body: {frag:?}"
        );
        let at = at.unwrap_or(0);
        Span {
            start: self.origin.offset + at,
            end: self.origin.offset + at + frag.len(),
        }
    }

    fn parse_all(&self) -> Vec<ProcStmt> {
        self.sub(self.src)
    }

    /// Parse an interior fragment — a subslice of the body — into statements.
    fn sub(&self, text: &'a str) -> Vec<ProcStmt> {
        split_slices(text)
            .into_iter()
            .map(|s| self.statement(s))
            .collect()
    }

    fn statement(&self, text: &'a str) -> ProcStmt {
        let span = self.span_of(text);
        let head = first_word(text);
        match head.as_str() {
            "RETURN" => self.ret(text, span),
            "RAISE" => self.raise(text, span),
            "PERFORM" => ProcStmt::Perform {
                sql: after_kw(text, 7).to_string(),
                span,
            },
            "EXECUTE" => self.dynamic(text, span),
            "EXIT" | "CONTINUE" => self.exit(text, span, head == "CONTINUE"),
            "OPEN" | "FETCH" | "MOVE" | "CLOSE" => self.cursor(text, span, &head),
            "IF" => self.if_stmt(text, span),
            "CASE" => self.case_stmt(text, span),
            "LOOP" | "WHILE" | "FOR" | "FOREACH" => self.loop_stmt(text, span, None),
            "DECLARE" | "BEGIN" => self.block(text, span),
            "NULL" => ProcStmt::Sql {
                sql: "NULL".into(),
                into: None,
                span,
            },
            // Every SQL command PL/pgSQL executes directly. `GET [CURRENT] DIAGNOSTICS` is a
            // PL/pgSQL statement but reads like one, and is carried the same way.
            "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "WITH" | "MERGE" | "CREATE" | "DROP"
            | "ALTER" | "TRUNCATE" | "REFRESH" | "COMMIT" | "ROLLBACK" | "SET" | "GET" | "CALL"
            | "NOTIFY" | "LISTEN" | "UNLISTEN" | "LOCK" | "GRANT" | "REVOKE" | "ANALYZE"
            | "COMMENT" | "RESET" | "DISCARD" | "VALUES" | "TABLE" | "COPY" | "REINDEX"
            | "CLUSTER" | "SECURITY" | "IMPORT" => self.sql(text, span),
            _ => {
                // An assignment is the only other shape: `target := expr`.
                if let Some((lhs, rhs)) = split_assign(text) {
                    return ProcStmt::Assign {
                        target: lhs,
                        expr: rhs,
                        span,
                    };
                }
                // A label opens a loop: `<<outer>> LOOP … END LOOP`.
                if let Some((label, rest)) = split_label(text) {
                    return self.loop_stmt(rest, span, Some(label));
                }
                ProcStmt::Unrecovered {
                    raw: text.to_string(),
                    reason: format!("unrecognized statement starting with {head:?}"),
                    span,
                }
            }
        }
    }

    fn sql(&self, text: &str, span: Span) -> ProcStmt {
        // `SELECT … INTO a, b` binds results to variables; the target list is part of the control
        // flow, not of the query, so it is lifted out here.
        let (sql, into) = match find_into(text) {
            Some((before, targets, after)) => (
                format!("{before} {after}").trim().to_string(),
                Some(targets),
            ),
            None => (text.to_string(), None),
        };
        ProcStmt::Sql { sql, into, span }
    }

    fn ret(&self, text: &str, span: Span) -> ProcStmt {
        let rest = after_kw(text, 6);
        if starts_with_kw(rest, "QUERY") {
            return ProcStmt::Return {
                value: None,
                query: Some(after_kw(rest, 5).to_string()),
                next: false,
                span,
            };
        }
        if starts_with_kw(rest, "NEXT") {
            return ProcStmt::Return {
                value: Some(after_kw(rest, 4).to_string()),
                query: None,
                next: true,
                span,
            };
        }
        ProcStmt::Return {
            value: (!rest.is_empty()).then(|| rest.to_string()),
            query: None,
            next: false,
            span,
        }
    }

    fn raise(&self, text: &str, span: Span) -> ProcStmt {
        let rest = after_kw(text, 5);
        let level = first_word(rest);
        let known = ["DEBUG", "LOG", "INFO", "NOTICE", "WARNING", "EXCEPTION"];
        let (level, message) = if known.contains(&level.as_str()) {
            (level.to_ascii_lowercase(), after_kw(rest, level.len()))
        } else {
            // A bare `RAISE` re-raises the current exception, and `RAISE 'msg'` defaults to
            // EXCEPTION.
            ("exception".to_string(), rest)
        };
        ProcStmt::Raise {
            level,
            message: message.to_string(),
            span,
        }
    }

    fn dynamic(&self, text: &str, span: Span) -> ProcStmt {
        let rest = after_kw(text, 7);
        let (expr, using) = match rest.to_ascii_uppercase().find(" USING ") {
            Some(at) => (
                rest[..at].trim().to_string(),
                rest[at + 7..]
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
            ),
            None => (rest.to_string(), Vec::new()),
        };
        let (expr, into) = match find_into(&expr) {
            Some((before, targets, after)) => (
                format!("{before} {after}").trim().to_string(),
                Some(targets),
            ),
            None => (expr, None),
        };
        ProcStmt::DynamicExecute {
            expr,
            using,
            into,
            span,
        }
    }

    /// `OPEN c [FOR query]`, `FETCH [direction FROM] c INTO targets`, `MOVE …`, `CLOSE c`.
    fn cursor(&self, text: &str, span: Span, head: &str) -> ProcStmt {
        let op = match head {
            "OPEN" => CursorOp::Open,
            "FETCH" => CursorOp::Fetch,
            "MOVE" => CursorOp::Move,
            _ => CursorOp::Close,
        };
        let rest = after_kw(text, head.len());
        let (rest, into) = match find_into(rest) {
            Some((before, targets, after)) => (
                format!("{before} {after}").trim().to_string(),
                Some(targets),
            ),
            None => (rest.to_string(), None),
        };
        let (name, query) = match find_kw(&rest, "FOR") {
            Some(at) => (
                slice(&rest, 0, at).trim().to_string(),
                Some(slice(&rest, at + 3, rest.len()).trim().to_string()),
            ),
            None => (rest.trim().to_string(), None),
        };
        // `FETCH NEXT FROM c` and friends put a direction before the cursor name; the name is the
        // last word either way.
        let name = name
            .split_whitespace()
            .next_back()
            .unwrap_or_default()
            .to_string();
        ProcStmt::Cursor {
            op,
            name,
            query,
            into,
            span,
        }
    }

    fn exit(&self, text: &str, span: Span, is_continue: bool) -> ProcStmt {
        let kw = if is_continue { 8 } else { 4 };
        let rest = after_kw(text, kw);
        let (label, when) = match rest.to_ascii_uppercase().find("WHEN ") {
            Some(at) => (
                (at > 0).then(|| rest[..at].trim().to_string()),
                Some(rest[at + 5..].trim().to_string()),
            ),
            None => ((!rest.is_empty()).then(|| rest.to_string()), None),
        };
        ProcStmt::Exit {
            label,
            when,
            is_continue,
            span,
        }
    }

    /// `IF cond THEN … [ELSIF cond THEN …] [ELSE …] END IF`
    fn if_stmt(&self, text: &'a str, span: Span) -> ProcStmt {
        let Some(inner) = strip_end(text, "IF") else {
            return self.unrecovered(text, span, "IF without a matching END IF");
        };
        let mut branches = Vec::new();
        let mut else_branch = None;
        let mut rest = after_kw(inner, 2);

        loop {
            let Some(then_at) = find_kw_outer(rest, "THEN") else {
                return self.unrecovered(text, span, "IF branch without THEN");
            };
            let cond = rest[..then_at].trim().to_string();
            let after = &rest[then_at + 4..];
            // Block depth, not just parentheses: the `ELSE` of a nested `IF`, or of a `CASE`
            // expression inside this branch, is not this statement's `ELSE`.
            let (body_text, tail) = split_at_kw(after, &["ELSIF", "ELSEIF", "ELSE"]);
            branches.push((cond, self.sub(body_text)));
            match tail {
                Some(("ELSE", more)) => {
                    else_branch = Some(self.sub(more));
                    break;
                }
                Some((_, more)) => rest = more,
                None => break,
            }
        }
        ProcStmt::If {
            branches,
            else_branch,
            span,
        }
    }

    /// `CASE [operand] WHEN … THEN … [ELSE …] END CASE`
    fn case_stmt(&self, text: &'a str, span: Span) -> ProcStmt {
        let Some(inner) = strip_end(text, "CASE") else {
            return self.unrecovered(text, span, "CASE without a matching END CASE");
        };
        let rest = after_kw(inner, 4);
        let Some(first_when) = find_kw_outer(rest, "WHEN") else {
            return self.unrecovered(text, span, "CASE without WHEN");
        };
        let operand = (first_when > 0).then(|| rest[..first_when].trim().to_string());
        let mut branches = Vec::new();
        let mut else_branch = None;
        let mut cursor = &rest[first_when + 4..];

        loop {
            let Some(then_at) = find_kw_outer(cursor, "THEN") else {
                return self.unrecovered(text, span, "CASE branch without THEN");
            };
            let label = cursor[..then_at].trim().to_string();
            let after = &cursor[then_at + 4..];
            let (body_text, tail) = split_at_statement_kw(after, &["WHEN", "ELSE"]);
            branches.push((label, self.sub(body_text)));
            match tail {
                Some(("ELSE", more)) => {
                    else_branch = Some(self.sub(more));
                    break;
                }
                Some((_, more)) => cursor = more,
                None => break,
            }
        }
        ProcStmt::Case {
            operand,
            branches,
            else_branch,
            span,
        }
    }

    fn loop_stmt(&self, text: &'a str, span: Span, label: Option<String>) -> ProcStmt {
        let Some(inner) = strip_end(text, "LOOP") else {
            return self.unrecovered(text, span, "LOOP without a matching END LOOP");
        };
        let head = first_word(inner);
        let Some(loop_at) = find_kw(inner, "LOOP") else {
            return self.unrecovered(text, span, "LOOP body not found");
        };
        let header = inner[..loop_at].trim();
        let body = &inner[loop_at + 4..];

        let kind = match head.as_str() {
            "LOOP" => LoopKind::Plain,
            "WHILE" => LoopKind::While {
                condition: header[5..].trim().to_string(),
            },
            "FOREACH" => {
                let h = header[7..].trim();
                match find_kw(h, "ARRAY") {
                    Some(at) => LoopKind::ForEach {
                        var: h[..at].trim().trim_end_matches("IN").trim().to_string(),
                        array: h[at + 5..].trim().to_string(),
                    },
                    None => {
                        return self.unrecovered(text, span, "FOREACH without ARRAY");
                    }
                }
            }
            "FOR" => {
                let h = header[3..].trim();
                let Some(in_at) = find_kw(h, "IN") else {
                    return self.unrecovered(text, span, "FOR without IN");
                };
                let var = h[..in_at].trim().to_string();
                let range = h[in_at + 2..].trim();
                let reverse = starts_with_kw(range, "REVERSE");
                let range = if reverse { range[7..].trim() } else { range };
                match find_kw_op(range, "..") {
                    Some(at) => LoopKind::ForRange {
                        var,
                        from: range[..at].trim().to_string(),
                        to: range[at + 2..].trim().to_string(),
                        reverse,
                    },
                    // `FOR r IN SELECT …` iterates a query.
                    None => LoopKind::ForQuery {
                        var,
                        sql: range
                            .trim_matches(|c| c == '(' || c == ')')
                            .trim()
                            .to_string(),
                    },
                }
            }
            _ => return self.unrecovered(text, span, "unrecognized loop form"),
        };

        ProcStmt::Loop {
            kind,
            body: self.sub(body),
            label,
            span,
        }
    }

    /// `[DECLARE …] BEGIN … [EXCEPTION WHEN … THEN …] END`
    fn block(&self, text: &'a str, span: Span) -> ProcStmt {
        let Some(inner) = strip_end(text, "") else {
            return self.unrecovered(text, span, "block without a matching END");
        };
        let (decl_text, after_decl) = if starts_with_kw(inner, "DECLARE") {
            let rest = after_kw(inner, 7);
            match find_kw_outer(rest, "BEGIN") {
                Some(at) => (&rest[..at], &rest[at + 5..]),
                None => return self.unrecovered(text, span, "DECLARE without BEGIN"),
            }
        } else {
            match find_kw_outer(inner, "BEGIN") {
                Some(at) => (&inner[..0], &inner[at + 5..]),
                None => return self.unrecovered(text, span, "block without BEGIN"),
            }
        };

        // `find_kw_outer`, not `find_kw`: an inner block's EXCEPTION belongs to the inner block.
        let (body_text, exception_text) = match find_kw_outer(after_decl, "EXCEPTION") {
            Some(at) => (&after_decl[..at], Some(&after_decl[at + 9..])),
            None => (after_decl, None),
        };

        ProcStmt::Block {
            declarations: self.declarations(decl_text),
            body: self.sub(body_text),
            exception: exception_text.map(|e| self.handlers(e)).unwrap_or_default(),
            span,
        }
    }

    /// `WHEN cond [OR cond …] THEN …`, repeated. Each handler's span covers its own `WHEN … ;`
    /// text, not the block around it.
    fn handlers(&self, text: &'a str) -> Vec<ExceptionHandler> {
        let mut out = Vec::new();
        let Some(first) = find_kw_outer(text, "WHEN") else {
            return out;
        };
        let mut cursor = &text[first..];
        loop {
            let after = &cursor[4..];
            let Some(then_at) = find_kw_outer(after, "THEN") else {
                break;
            };
            // `WHEN unique_violation OR SQLSTATE '23503' THEN`
            let mut conditions = Vec::new();
            let mut current: Vec<&str> = Vec::new();
            for w in after[..then_at].split_whitespace() {
                if w.eq_ignore_ascii_case("OR") {
                    conditions.push(current.join(" ").to_ascii_lowercase());
                    current.clear();
                } else {
                    current.push(w);
                }
            }
            conditions.push(current.join(" ").to_ascii_lowercase());
            conditions.retain(|c| !c.is_empty());

            let body_and_rest = &after[then_at + 4..];
            let (body, tail) = split_at_statement_kw(body_and_rest, &["WHEN"]);
            let end = (body.as_ptr() as usize - cursor.as_ptr() as usize) + body.trim_end().len();
            out.push(ExceptionHandler {
                conditions,
                body: self.sub(body),
                span: self.span_of(cursor[..end].trim_end()),
            });
            match tail {
                // `more` starts just after the next `WHEN`; step back onto it.
                Some((_, more)) => {
                    let at = more.as_ptr() as usize - cursor.as_ptr() as usize - 4;
                    cursor = &cursor[at..];
                }
                None => break,
            }
        }
        out
    }

    fn declarations(&self, text: &'a str) -> Vec<VarDecl> {
        split_slices(text)
            .into_iter()
            .filter_map(|d| {
                let mut parts = d.split_whitespace();
                let name = parts.next()?.to_string();
                let rest: Vec<&str> = parts.collect();
                if rest.is_empty() {
                    return None;
                }
                let constant = rest[0].eq_ignore_ascii_case("CONSTANT");
                let rest = if constant { &rest[1..] } else { &rest[..] };
                let joined = rest.join(" ");
                // `c CURSOR FOR SELECT …` declares a cursor, not a typed variable. The "type" is
                // the cursor's query, which is what a consumer needs.
                let (data_type, default) =
                    match joined.find(":=").or_else(|| find_kw(&joined, "DEFAULT")) {
                        Some(at) => {
                            let val = joined[at..]
                                .trim_start_matches(":=")
                                .trim_start_matches("DEFAULT")
                                .trim()
                                .to_string();
                            (joined[..at].trim().to_string(), Some(val))
                        }
                        None => (joined.trim().to_string(), None),
                    };
                Some(VarDecl {
                    name,
                    data_type,
                    default,
                    constant,
                    span: self.span_of(d),
                })
            })
            .collect()
    }

    fn unrecovered(&self, text: &str, span: Span, reason: &str) -> ProcStmt {
        ProcStmt::Unrecovered {
            raw: text.to_string(),
            reason: reason.to_string(),
            span,
        }
    }
}

/// Find a keyword at **block** depth zero: not inside a nested `BEGIN … END`, `IF … END IF`,
/// `LOOP … END LOOP` or `CASE … END CASE`.
///
/// The plain [`find_kw`] tracks parentheses only, which is right for `THEN` and `IN` but wrong for
/// `BEGIN` and `EXCEPTION`: an outer block searching for its own `EXCEPTION` finds the *inner*
/// block's, and ends up owning a handler that belongs to someone else. That produced two handlers
/// where the source has one, and it is the kind of error that silently reattaches error handling
/// to the wrong scope.
fn find_kw_outer(s: &str, kw: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut open: Vec<&'static str> = Vec::new();
    let mut parens = 0i32;
    while i < b.len() {
        let c = b[i];
        if let Some(next) = skip_quoted(s, i) {
            i = next;
            continue;
        }
        if c == b'(' {
            parens += 1;
        } else if c == b')' {
            parens -= 1;
        }

        let word_start = i == 0 || !is_word_byte(b[i - 1]);
        fn ends(b: &[u8], i: usize, n: usize) -> bool {
            i + n == b.len() || !is_word_byte(b[i + n])
        }

        if word_start {
            // The target, at depth zero.
            let n = kw.len();
            if depth == 0
                && parens == 0
                && i + n <= b.len()
                && b[i..i + n].eq_ignore_ascii_case(kw.as_bytes())
                && ends(b, i, n)
            {
                return Some(i);
            }
            // Openers and closers.
            let mut matched = 0usize;
            for opener in ["BEGIN", "CASE", "LOOP", "IF"] {
                let n = opener.len();
                if i + n <= b.len()
                    && b[i..i + n].eq_ignore_ascii_case(opener.as_bytes())
                    && ends(b, i, n)
                    && (opener != "IF" || at_statement_start(b, i))
                {
                    depth += 1;
                    open.push(opener);
                    matched = n;
                    break;
                }
            }
            if matched == 0
                && i + 3 <= b.len()
                && b[i..i + 3].eq_ignore_ascii_case(b"END")
                && ends(b, i, 3)
            {
                depth -= 1;
                // Same rule as the splitter: `END IF` is one closer.
                matched = 3 + end_tail_len(b, i + 3, open.pop());
            }
            if matched > 0 {
                i += matched;
                continue;
            }
        }
        i += 1;
    }
    None
}

/// Find a keyword at nesting depth zero, respecting strings and parentheses.
fn find_kw(s: &str, kw: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut parens = 0i32;
    while i < b.len() {
        let c = b[i];
        if let Some(next) = skip_quoted(s, i) {
            i = next;
            continue;
        }
        if c == b'(' {
            parens += 1;
        } else if c == b')' {
            parens -= 1;
        }
        if parens == 0
            && i + kw.len() <= b.len()
            && b[i..i + kw.len()].eq_ignore_ascii_case(kw.as_bytes())
            && (i == 0 || !is_word_byte(b[i - 1]))
            && (i + kw.len() == b.len() || !is_word_byte(b[i + kw.len()]))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Split at the first of `keywords` found at **block** depth zero, returning subslices: the text
/// before it, and the keyword with the text after it.
fn split_at_kw<'s>(
    s: &'s str,
    keywords: &[&'static str],
) -> (&'s str, Option<(&'static str, &'s str)>) {
    let mut best: Option<(usize, &'static str)> = None;
    for kw in keywords {
        if let Some(at) = find_kw_outer(s, kw)
            && best.is_none_or(|(b, _)| at < b)
        {
            best = Some((at, kw));
        }
    }
    match best {
        Some((at, kw)) => (&s[..at], Some((kw, &s[at + kw.len()..]))),
        None => (s, None),
    }
}

/// [`split_at_kw`] for keywords that only count at the **start of a statement**: a `CASE` branch's
/// `WHEN` or a handler's `WHEN`, but not the one in `EXIT WHEN done;` inside the branch body.
fn split_at_statement_kw<'s>(
    s: &'s str,
    keywords: &[&'static str],
) -> (&'s str, Option<(&'static str, &'s str)>) {
    let mut from = 0;
    loop {
        let (_, found) = split_at_kw(&s[from..], keywords);
        let Some((kw, more)) = found else {
            return (s, None);
        };
        let at = more.as_ptr() as usize - s.as_ptr() as usize - kw.len();
        let before = s[..at].trim_end();
        if before.is_empty() || before.ends_with(';') {
            return (&s[..at], Some((kw, more)));
        }
        from = at + kw.len();
    }
}

/// Strip a trailing `END [what] [label]`, returning the interior as a subslice of `text`.
fn strip_end<'s>(text: &'s str, what: &str) -> Option<&'s str> {
    fn last_word(t: &str) -> (&str, &str) {
        let at = t.rfind(char::is_whitespace).map_or(0, |a| a + 1);
        (t[..at].trim_end(), &t[at..])
    }
    let is_label = |w: &str| {
        !w.is_empty()
            && w.bytes().all(is_word_byte)
            && !w.eq_ignore_ascii_case("END")
            && !w.eq_ignore_ascii_case(what)
    };
    let t = text.trim_end().trim_end_matches(';').trim_end();
    let (mut before, mut w) = last_word(t);
    // `END LOOP outer;` and `END outer;` close a labelled loop or block.
    if is_label(w) {
        let (b, w2) = last_word(before);
        if w2.eq_ignore_ascii_case("END") || (!what.is_empty() && w2.eq_ignore_ascii_case(what)) {
            (before, w) = (b, w2);
        }
    }
    if !what.is_empty() && w.eq_ignore_ascii_case(what) {
        let (b, w2) = last_word(before);
        return w2.eq_ignore_ascii_case("END").then_some(b);
    }
    // A bare `END` is accepted for any construct, as it always was.
    w.eq_ignore_ascii_case("END").then_some(before)
}

/// `target := expr`, or `target = expr` — PL/pgSQL accepts both, and older code uses `=`.
fn split_assign(text: &str) -> Option<(String, String)> {
    if let Some(at) = find_kw_op(text, ":=") {
        let lhs = text[..at].trim();
        if lhs.is_empty() || lhs.contains(char::is_whitespace) {
            return None;
        }
        return Some((lhs.to_string(), text[at + 2..].trim().to_string()));
    }
    // `=` is also comparison, so the target must be exactly a variable reference — a name, a dotted
    // field, an array subscript — and the statement must not have matched any keyword already.
    let at = find_kw_op(text, "=")?;
    let lhs = text[..at].trim();
    let b = text.as_bytes();
    let is_target = !lhs.is_empty()
        && lhs.as_bytes()[0].is_ascii_alphabetic() | (lhs.as_bytes()[0] == b'_')
        && lhs
            .bytes()
            .all(|c| is_word_byte(c) || matches!(c, b'.' | b'[' | b']' | b'"'));
    let comparison =
        at > 0 && matches!(b[at - 1], b'<' | b'>' | b'!' | b':') || b.get(at + 1) == Some(&b'=');
    if !is_target || comparison {
        return None;
    }
    Some((lhs.to_string(), text[at + 1..].trim().to_string()))
}

fn find_kw_op(s: &str, op: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut parens = 0i32;
    while i < b.len() {
        if let Some(next) = skip_quoted(s, i) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' => parens += 1,
            b')' => parens -= 1,
            _ => {}
        }
        if parens == 0 && b[i..].starts_with(op.as_bytes()) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn split_label(text: &str) -> Option<(String, &str)> {
    let t = text.trim_start();
    let rest = t.strip_prefix("<<")?;
    let close = rest.find(">>")?;
    Some((rest[..close].trim().to_string(), rest[close + 2..].trim()))
}

/// `SELECT a, b INTO x, y FROM t` → the query without `INTO`, plus the targets.
fn find_into(text: &str) -> Option<(String, Vec<String>, String)> {
    let at = find_kw(text, "INTO")?;
    let after = &text[at + 4..];
    let after = after
        .trim_start()
        .strip_prefix("STRICT")
        .unwrap_or(after.trim_start());
    // The target list runs to the next clause keyword.
    let end = ["FROM", "WHERE", "USING", "RETURNING"]
        .iter()
        .filter_map(|k| find_kw(after, k))
        .min()
        .unwrap_or(after.len());
    let targets: Vec<String> = after[..end]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if targets.is_empty() {
        return None;
    }
    Some((
        text[..at].trim().to_string(),
        targets,
        after[end..].trim().to_string(),
    ))
}

/// Parse a function body into statements.
///
/// Comments are blanked first (see [`crate::lex::mask_comments`]), so no statement text carries
/// one and no keyword scan can be misled by one. Spans still index the original `body`.
pub fn parse_body(body: &str, origin: Origin) -> Vec<ProcStmt> {
    let masked = crate::lex::mask_comments(body);
    Parser::new(&masked, origin).parse_all()
}

/// Parse a whole `CREATE FUNCTION … AS $$ … $$` into a [`ProcedureIr`].
pub fn parse_function(src: &str) -> Result<ProcedureIr, crate::lex::LexError> {
    let toks = crate::lex::lex(src)?;
    let signature = parse_signature(src, &toks);
    // A routine in a language this crate does not parse gets its signature and an honest label,
    // never an empty body that would read as "nothing in it".
    if !signature.language.eq_ignore_ascii_case("plpgsql") {
        return Ok(ProcedureIr::signature_only(signature));
    }
    let stmts = match quoted_body(src, &toks) {
        Some((body, collapsed, start)) => {
            // Parsed unescaped, then every span mapped back onto the source as written: a
            // statement after N doubled quotes sits N bytes further on in the file.
            let mut stmts = parse_body(&body, Origin { offset: 0 });
            let map = |v: usize| start + v + collapsed.partition_point(|&p| p < v);
            for s in &mut stmts {
                s.spans_mut(&mut |sp| {
                    *sp = Span {
                        start: map(sp.start),
                        end: map(sp.end),
                    }
                });
            }
            stmts
        }
        None => {
            let Some((body, start, _end)) = crate::lex::function_body(src)? else {
                return Ok(ProcedureIr::signature_only(signature));
            };
            parse_body(&body, Origin { offset: start })
        }
    };
    // A body is one `BEGIN … END` block; lift its declarations so callers see them directly.
    if let [
        ProcStmt::Block {
            declarations,
            body,
            exception,
            span,
        },
    ] = stmts.as_slice()
        && exception.is_empty()
    {
        let _ = span;
        return Ok(ProcedureIr::new(
            signature,
            declarations.clone(),
            body.clone(),
        ));
    }
    Ok(ProcedureIr::new(signature, Vec::new(), stmts))
}

/// A body written the pre-8.0 way, `AS ' … '` with every quote inside doubled. PostgreSQL still
/// accepts it and real schemas still carry it; reading it as "no body" labelled such a routine
/// `Signature`, which is honest but wrong.
///
/// Returns the unescaped body, the body offsets at which a doubled quote collapsed to one, and the
/// source offset where the body starts — enough to map any span in the body back to the source.
fn quoted_body(src: &str, toks: &[crate::lex::Spanned]) -> Option<(String, Vec<usize>, usize)> {
    use crate::lex::Tok;
    let at = toks.windows(2).position(|w| {
        matches!(&w[0].tok, Tok::Word(a) if a.eq_ignore_ascii_case("AS"))
            && matches!(w[1].tok, Tok::Str(_) | Tok::Dollar { .. })
    })?;
    let t = &toks[at + 1];
    if !matches!(t.tok, Tok::Str(_)) {
        return None;
    }
    // A `C` routine's `AS 'module', 'symbol'` never gets here: only plpgsql bodies are parsed.
    let raw = &src.as_bytes()[t.start + 1..t.end - 1];
    let mut body = Vec::with_capacity(raw.len());
    let mut collapsed = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\'' && raw.get(i + 1) == Some(&b'\'') {
            collapsed.push(body.len());
            body.push(b'\'');
            i += 2;
        } else {
            body.push(raw[i]);
            i += 1;
        }
    }
    Some((
        String::from_utf8_lossy(&body).into_owned(),
        collapsed,
        t.start + 1,
    ))
}

/// Words that end a `RETURNS` type: the routine attributes that may follow it, in any order.
const ROUTINE_ATTRIBUTES: &[&str] = &[
    "LANGUAGE",
    "AS",
    "IMMUTABLE",
    "STABLE",
    "VOLATILE",
    "STRICT",
    "CALLED",
    "SECURITY",
    "EXTERNAL",
    "PARALLEL",
    "LEAKPROOF",
    "NOT",
    "COST",
    "ROWS",
    "SUPPORT",
    "SET",
    "WINDOW",
    "TRANSFORM",
    "BEGIN",
    "RETURN",
];

/// Read the routine header from tokens rather than raw text.
///
/// The body is a single `Dollar` token, so nothing inside it can be mistaken for a clause — a
/// column called `language` in the body was once read as the routine's language. Hand-written
/// schemas also put `LANGUAGE plpgsql;` *after* the body, quoted or not, which a text scan read as
/// the language `plpgsql;`.
fn parse_signature(src: &str, toks: &[crate::lex::Spanned]) -> ProcSignature {
    use crate::lex::Tok;
    let is_word = |t: &Tok, w: &str| matches!(t, Tok::Word(x) if x.eq_ignore_ascii_case(w));

    let mut name = String::new();
    let mut arguments = Vec::new();
    let mut i = toks
        .iter()
        .position(|t| is_word(&t.tok, "FUNCTION") || is_word(&t.tok, "PROCEDURE"))
        .map_or(toks.len(), |k| k + 1);

    // `schema.name`, either part possibly quoted.
    while let Some(t) = toks.get(i) {
        match &t.tok {
            Tok::Word(w) => name.push_str(w),
            Tok::Punct('.') => name.push('.'),
            _ => break,
        }
        i += 1;
    }

    // Arguments: split on top-level commas, so `numeric(10,2)` stays one argument.
    if matches!(toks.get(i).map(|t| &t.tok), Some(Tok::Punct('('))) {
        let mut depth = 0i32;
        let mut seg = toks[i].end;
        for t in &toks[i..] {
            match t.tok {
                Tok::Punct('(') => depth += 1,
                Tok::Punct(')') => {
                    depth -= 1;
                    if depth == 0 {
                        arguments.push(src[seg..t.start].trim().to_string());
                        i += 1;
                        break;
                    }
                }
                Tok::Punct(',') if depth == 1 => {
                    arguments.push(src[seg..t.start].trim().to_string());
                    seg = t.end;
                }
                _ => {}
            }
            i += 1;
        }
        arguments.retain(|a| !a.is_empty());
    }

    // The clauses after the argument list, at parenthesis depth zero.
    let mut returns = None;
    let mut language = None;
    let mut depth = 0i32;
    let mut k = i;
    while k < toks.len() {
        let t = &toks[k].tok;
        match t {
            Tok::Punct('(') => depth += 1,
            Tok::Punct(')') => depth -= 1,
            _ => {}
        }
        if depth == 0 && returns.is_none() && is_word(t, "RETURNS") {
            // `RETURNS NULL ON NULL INPUT` is a strictness clause, not a type.
            let null_clause = toks.get(k + 1).is_some_and(|n| is_word(&n.tok, "NULL"))
                && toks.get(k + 2).is_some_and(|n| is_word(&n.tok, "ON"));
            if !null_clause {
                let from = k + 1;
                let mut d = 0i32;
                let mut end = from;
                while let Some(n) = toks.get(end) {
                    match &n.tok {
                        Tok::Punct('(') => d += 1,
                        Tok::Punct(')') => d -= 1,
                        Tok::Punct(';') | Tok::Dollar { .. } | Tok::Str(_) if d == 0 => break,
                        Tok::Word(w)
                            if d == 0
                                && end > from
                                && ROUTINE_ATTRIBUTES.iter().any(|a| w.eq_ignore_ascii_case(a)) =>
                        {
                            break;
                        }
                        _ => {}
                    }
                    end += 1;
                }
                if end > from {
                    let text = &src[toks[from].start..toks[end - 1].end];
                    returns = Some(text.split_whitespace().collect::<Vec<_>>().join(" "));
                }
                k = end;
                continue;
            }
        }
        if depth == 0
            && language.is_none()
            && is_word(t, "LANGUAGE")
            && let Some(Tok::Word(l) | Tok::Str(l)) = toks.get(k + 1).map(|n| &n.tok)
        {
            language = Some(l.to_ascii_lowercase());
        }
        k += 1;
    }

    ProcSignature {
        name,
        arguments,
        returns,
        language: language.unwrap_or_else(|| "plpgsql".into()),
    }
}

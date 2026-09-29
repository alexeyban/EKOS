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
    src: &'a str,
    /// Statement-sized slices with their spans, produced by [`split_statements`].
    stmts: Vec<(String, Span)>,
    pos: usize,
}

/// Split a body into statements at top-level semicolons.
///
/// Semicolons inside strings, dollar quotes, comments and parentheses do not end a statement, and
/// neither does the one closing a nested `END;` — nesting is tracked by keyword depth so a block's
/// interior stays with it.
pub fn split_statements(body: &str, origin: Origin) -> Vec<(String, Span)> {
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut depth = 0i32;
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
        if c == b'\'' || c == b'"' {
            let q = c;
            i += 1;
            while i < b.len() {
                if b[i] == q {
                    if i + 1 < b.len() && b[i + 1] == q {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
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
        if c == b'$'
            && let Some((tag, after)) = crate::lex::probe_dollar(body, i)
        {
            let close = format!("${tag}$");
            match body[after..].find(&close) {
                Some(rel) => {
                    i = after + rel + close.len();
                    continue;
                }
                None => break,
            }
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

        if let Some((kw, n)) = keyword_at(i) {
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
                    let after = i + n;
                    let rest = &b[after..];
                    let skip = rest.iter().take_while(|c| c.is_ascii_whitespace()).count();
                    for tail in ["IF", "LOOP", "CASE"] {
                        let m = tail.len();
                        if after + skip + m <= b.len()
                            && b[after + skip..after + skip + m]
                                .eq_ignore_ascii_case(tail.as_bytes())
                            && (after + skip + m == b.len() || !is_word_byte(b[after + skip + m]))
                        {
                            consumed = n + skip + m;
                            break;
                        }
                    }
                }
                "ELSIF" | "ELSE" | "EXCEPTION" => {}
                // `CASE` appears both as a statement and as an expression (`SELECT CASE WHEN …`);
                // an expression `CASE` still has a matching `END`, so counting both keeps the
                // depth balanced.
                _ => depth += 1,
            }
            i += consumed;
            continue;
        }

        if c == b';' && depth <= 0 && parens <= 0 && !in_declare {
            let text = slice(body, start, i).trim().to_string();
            if !text.is_empty() {
                out.push((
                    text,
                    Span {
                        start: origin.offset + start,
                        end: origin.offset + i,
                    },
                ));
            }
            start = i + 1;
        }
        i += 1;
    }
    let tail = slice(body, start.min(body.len()), body.len()).trim();
    if !tail.is_empty() {
        out.push((
            tail.to_string(),
            Span {
                start: origin.offset + start,
                end: origin.offset + body.len(),
            },
        ));
    }
    out
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

/// Drop leading whitespace and comments, so classification sees the statement itself.
/// The text after a leading keyword of `n` bytes. The keyword is ASCII by construction, so the
/// offset is a boundary — but the *slice* has to come from the trimmed string, and doing it in one
/// place is how the rest of this module stays free of index arithmetic.
fn after_kw(text: &str, n: usize) -> &str {
    let t = text.trim_start();
    if t.len() <= n {
        return "";
    }
    t[n..].trim()
}

/// Drop leading whitespace and comments, so classification sees the statement itself.
fn strip_leading_trivia(s: &str) -> String {
    let mut rest = s.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = match r.find('\n') {
                Some(at) => r[at + 1..].trim_start(),
                None => "",
            };
            continue;
        }
        if let Some(r) = rest.strip_prefix("/*") {
            rest = match r.find("*/") {
                Some(at) => r[at + 2..].trim_start(),
                None => "",
            };
            continue;
        }
        return rest.to_string();
    }
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
        Self {
            stmts: split_statements(src, origin),
            src,
            pos: 0,
        }
    }

    fn parse_all(&mut self) -> Vec<ProcStmt> {
        let mut out = Vec::new();
        while self.pos < self.stmts.len() {
            let (text, span) = self.stmts[self.pos].clone();
            self.pos += 1;
            out.push(self.statement(&text, span));
        }
        out
    }

    fn statement(&mut self, text: &str, span: Span) -> ProcStmt {
        // A comment sits *inside* the statement text the splitter produced, because a comment does
        // not end a statement. Classifying without stripping it makes every commented statement
        // unrecoverable — the first word is `--`.
        let text = &strip_leading_trivia(text);
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
            "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "WITH" | "MERGE" | "CREATE" | "DROP"
            | "ALTER" | "TRUNCATE" | "REFRESH" | "COMMIT" | "ROLLBACK" | "SET" | "GET" => {
                self.sql(text, span)
            }
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

    fn sql(&mut self, text: &str, span: Span) -> ProcStmt {
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

    fn ret(&mut self, text: &str, span: Span) -> ProcStmt {
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

    fn raise(&mut self, text: &str, span: Span) -> ProcStmt {
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

    fn dynamic(&mut self, text: &str, span: Span) -> ProcStmt {
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
    fn cursor(&mut self, text: &str, span: Span, head: &str) -> ProcStmt {
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

    fn exit(&mut self, text: &str, span: Span, is_continue: bool) -> ProcStmt {
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
    fn if_stmt(&mut self, text: &str, span: Span) -> ProcStmt {
        let Some(inner) = strip_end(text, "IF") else {
            return self.unrecovered(text, span, "IF without a matching END IF");
        };
        let mut branches = Vec::new();
        let mut else_branch = None;
        let mut rest = inner.trim_start()[2..].trim().to_string();

        loop {
            let Some(then_at) = find_kw(&rest, "THEN") else {
                return self.unrecovered(text, span, "IF branch without THEN");
            };
            let cond = rest[..then_at].trim().to_string();
            let after = &rest[then_at + 4..];
            let (body_text, tail) = split_at_kw(after, &["ELSIF", "ELSEIF", "ELSE"]);
            let at = offset_of(&rest, &body_text, span.start);
            branches.push((cond, self.sub_at(&body_text, at)));
            match tail {
                Some((kw, more)) if kw == "ELSE" => {
                    let at = offset_of(&rest, &more, span.start);
                    else_branch = Some(self.sub_at(&more, at));
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
    fn case_stmt(&mut self, text: &str, span: Span) -> ProcStmt {
        let Some(inner) = strip_end(text, "CASE") else {
            return self.unrecovered(text, span, "CASE without a matching END CASE");
        };
        let rest = inner.trim_start()[4..].trim();
        let Some(first_when) = find_kw(rest, "WHEN") else {
            return self.unrecovered(text, span, "CASE without WHEN");
        };
        let operand = (first_when > 0).then(|| rest[..first_when].trim().to_string());
        let mut branches = Vec::new();
        let mut else_branch = None;
        let mut cursor = rest[first_when + 4..].to_string();

        loop {
            let Some(then_at) = find_kw(&cursor, "THEN") else {
                return self.unrecovered(text, span, "CASE branch without THEN");
            };
            let label = cursor[..then_at].trim().to_string();
            let after = &cursor[then_at + 4..];
            let (body_text, tail) = split_at_kw(after, &["WHEN", "ELSE"]);
            let at = offset_of(&cursor, &body_text, span.start);
            branches.push((label, self.sub_at(&body_text, at)));
            match tail {
                Some((kw, more)) if kw == "ELSE" => {
                    let at = offset_of(&cursor, &more, span.start);
                    else_branch = Some(self.sub_at(&more, at));
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

    fn loop_stmt(&mut self, text: &str, span: Span, label: Option<String>) -> ProcStmt {
        let Some(inner) = strip_end(text, "LOOP") else {
            return self.unrecovered(text, span, "LOOP without a matching END LOOP");
        };
        let head = first_word(&inner);
        let Some(loop_at) = find_kw(&inner, "LOOP") else {
            return self.unrecovered(text, span, "LOOP body not found");
        };
        let header = inner[..loop_at].trim();
        let body = inner[loop_at + 4..].to_string();

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
                match range.find("..") {
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
            body: {
                let at = offset_of(&inner, &body, span.start);
                self.sub_at(&body, at)
            },
            label,
            span,
        }
    }

    /// `[DECLARE …] BEGIN … [EXCEPTION WHEN … THEN …] END`
    fn block(&mut self, text: &str, span: Span) -> ProcStmt {
        let Some(inner) = strip_end(text, "") else {
            return self.unrecovered(text, span, "block without a matching END");
        };
        let (decl_text, after_decl) = if starts_with_kw(&inner, "DECLARE") {
            let rest = inner.trim_start()[7..].to_string();
            match find_kw_outer(&rest, "BEGIN") {
                Some(at) => (
                    slice(&rest, 0, at).to_string(),
                    slice(&rest, at + 5, rest.len()).to_string(),
                ),
                None => return self.unrecovered(text, span, "DECLARE without BEGIN"),
            }
        } else {
            match find_kw_outer(&inner, "BEGIN") {
                Some(at) => (
                    String::new(),
                    slice(&inner, at + 5, inner.len()).to_string(),
                ),
                None => return self.unrecovered(text, span, "block without BEGIN"),
            }
        };

        // `find_kw_outer`, not `find_kw`: an inner block's EXCEPTION belongs to the inner block.
        let (body_text, exception_text) = match find_kw_outer(&after_decl, "EXCEPTION") {
            Some(at) => (
                slice(&after_decl, 0, at).to_string(),
                Some(slice(&after_decl, at + 9, after_decl.len()).to_string()),
            ),
            None => (after_decl, None),
        };

        ProcStmt::Block {
            declarations: parse_declarations(&decl_text, span),
            body: {
                let at = offset_of(&inner, &body_text, span.start);
                self.sub_at(&body_text, at)
            },
            exception: exception_text
                .map(|e| parse_handlers(&e, span, self.src))
                .unwrap_or_default(),
            span,
        }
    }

    /// Parse an interior fragment.
    ///
    /// `offset` is where the fragment begins **in the original source**, not where its parent
    /// begins. Passing the parent's start shifts every nested span by however far into the parent
    /// the fragment sits — which still looks like a plausible offset, so the spans point at real
    /// text that is simply the wrong text. That is worse than an obviously broken span.
    fn sub_at(&mut self, text: &str, offset: usize) -> Vec<ProcStmt> {
        let mut p = Parser::new(self.src, Origin { offset });
        p.stmts = split_statements(text, Origin { offset });
        p.parse_all()
    }

    fn unrecovered(&self, text: &str, span: Span, reason: &str) -> ProcStmt {
        ProcStmt::Unrecovered {
            raw: text.to_string(),
            reason: reason.to_string(),
            span,
        }
    }
}

/// Where `fragment` sits in the original source, given that `parent` starts at `parent_at`.
///
/// The fragment is a slice of the parent by construction, so this is a search for a known
/// substring; falling back to the parent's own offset keeps a span plausible rather than absent
/// when a caller passes something reconstructed.
fn offset_of(parent: &str, fragment: &str, parent_at: usize) -> usize {
    parent
        .find(fragment)
        .map(|rel| parent_at + rel)
        .unwrap_or(parent_at)
}

fn parse_handlers(text: &str, span: Span, src: &str) -> Vec<ExceptionHandler> {
    let mut out = Vec::new();
    let mut cursor = text.to_string();
    while let Some(when_at) = find_kw(&cursor, "WHEN") {
        let after = &cursor[when_at + 4..];
        let Some(then_at) = find_kw(after, "THEN") else {
            break;
        };
        let conditions: Vec<String> = slice(after, 0, then_at)
            .split('|')
            .map(|s| {
                s.trim()
                    .trim_start_matches("OR")
                    .trim()
                    .to_ascii_lowercase()
            })
            .filter(|s| !s.is_empty())
            .collect();
        let body_and_rest = &after[then_at + 4..];
        let (body_text, tail) = split_at_kw(body_and_rest, &["WHEN"]);
        let mut p = Parser::new(src, Origin { offset: span.start });
        p.stmts = split_statements(&body_text, Origin { offset: span.start });
        out.push(ExceptionHandler {
            conditions,
            body: p.parse_all(),
            span,
        });
        match tail {
            Some((_, more)) => cursor = format!("WHEN {more}"),
            None => break,
        }
    }
    out
}

fn parse_declarations(text: &str, span: Span) -> Vec<VarDecl> {
    split_statements(text, Origin { offset: span.start })
        .into_iter()
        .filter_map(|(d, s)| {
            let mut parts = d.split_whitespace();
            let name = parts.next()?.to_string();
            let rest: Vec<&str> = parts.collect();
            if rest.is_empty() {
                return None;
            }
            let constant = rest[0].eq_ignore_ascii_case("CONSTANT");
            let rest = if constant { &rest[1..] } else { &rest[..] };
            let joined = rest.join(" ");
            // `c CURSOR FOR SELECT …` declares a cursor, not a typed variable. The "type" is the
            // cursor's query, which is what a consumer needs.
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
                span: s,
            })
        })
        .collect()
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
    let mut parens = 0i32;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != b'\'' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
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
                {
                    depth += 1;
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
                matched = 3;
                // Same rule as the splitter: `END IF` is one closer.
                let after = i + 3;
                let skip = b[after..]
                    .iter()
                    .take_while(|c| c.is_ascii_whitespace())
                    .count();
                for tail in ["IF", "LOOP", "CASE"] {
                    let m = tail.len();
                    if after + skip + m <= b.len()
                        && b[after + skip..after + skip + m].eq_ignore_ascii_case(tail.as_bytes())
                        && (after + skip + m == b.len() || !is_word_byte(b[after + skip + m]))
                    {
                        matched = 3 + skip + m;
                        break;
                    }
                }
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
        if c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != b'\'' {
                i += 1;
            }
            i += 1;
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

/// Split at the first of `keywords` found at depth zero.
fn split_at_kw(s: &str, keywords: &[&str]) -> (String, Option<(String, String)>) {
    let mut best: Option<(usize, &str)> = None;
    for kw in keywords {
        if let Some(at) = find_kw(s, kw)
            && best.is_none_or(|(b, _)| at < b)
        {
            best = Some((at, kw));
        }
    }
    match best {
        Some((at, kw)) => (
            slice(s, 0, at).to_string(),
            Some((
                kw.to_ascii_uppercase(),
                slice(s, at + kw.len(), s.len()).to_string(),
            )),
        ),
        None => (s.to_string(), None),
    }
}

/// Strip a trailing `END [what]`, returning the interior.
fn strip_end(text: &str, what: &str) -> Option<String> {
    let t = text.trim_end().trim_end_matches(';').trim_end();
    let upper = t.to_ascii_uppercase();
    let suffix = if what.is_empty() {
        "END".to_string()
    } else {
        format!("END {what}")
    };
    if upper.ends_with(&suffix) {
        Some(t[..t.len() - suffix.len()].to_string())
    } else if upper.ends_with("END") {
        Some(t[..t.len() - 3].to_string())
    } else {
        None
    }
}

fn split_assign(text: &str) -> Option<(String, String)> {
    let at = find_kw_op(text, ":=")?;
    let lhs = text[..at].trim();
    if lhs.is_empty() || lhs.contains(char::is_whitespace) {
        return None;
    }
    Some((lhs.to_string(), text[at + 2..].trim().to_string()))
}

fn find_kw_op(s: &str, op: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() && b[i] != b'\'' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if b[i..].starts_with(op.as_bytes()) {
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
pub fn parse_body(body: &str, origin: Origin) -> Vec<ProcStmt> {
    Parser::new(body, origin).parse_all()
}

/// Parse a whole `CREATE FUNCTION … AS $$ … $$` into a [`ProcedureIr`].
pub fn parse_function(src: &str) -> Result<ProcedureIr, crate::lex::LexError> {
    let signature = parse_signature(src);
    // A routine in a language this crate does not parse gets its signature and an honest label,
    // never an empty body that would read as "nothing in it".
    if !signature.language.eq_ignore_ascii_case("plpgsql") {
        return Ok(ProcedureIr::signature_only(signature));
    }
    let Some((body, start, _end)) = crate::lex::function_body(src)? else {
        return Ok(ProcedureIr::signature_only(signature));
    };
    let stmts = parse_body(&body, Origin { offset: start });
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

fn parse_signature(src: &str) -> ProcSignature {
    let upper = src.to_ascii_uppercase();
    let name = upper
        .find("FUNCTION")
        .or_else(|| upper.find("PROCEDURE"))
        .map(|at| {
            let after = &src[at..];
            let after = after.split_whitespace().nth(1).unwrap_or_default();
            after.split('(').next().unwrap_or_default().to_string()
        })
        .unwrap_or_default();
    let arguments = src
        .find('(')
        .and_then(|a| src[a..].find(')').map(|b| &src[a + 1..a + b]))
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let returns = find_kw(src, "RETURNS").map(|at| {
        src[at + 7..]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string()
    });
    let language = find_kw(src, "LANGUAGE")
        .map(|at| {
            src[at + 8..]
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
        })
        .unwrap_or_else(|| "plpgsql".into());
    ProcSignature {
        name,
        arguments,
        returns,
        language,
    }
}

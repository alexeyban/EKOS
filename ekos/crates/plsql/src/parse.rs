//! The PL/SQL front end (RFC 0210 §3.3): Oracle routine headers, declaration sections, triggers
//! and packages, read here; every `BEGIN … END` block handed to `ekos_plpgsql::parse_body` under
//! [`Flavor::PlSql`], so both languages produce one IR and one fidelity rule.
//!
//! Spans in every [`Routine`]'s IR are relative to the routine's own text, the convention
//! `ekos_plpgsql::parse_function` follows, so one analyzer reads both.

use crate::script::{
    Tok, TokKind, UnitKind, mask_comments, matching_paren, neutralize_quotes, tokens, units,
};
use ekos_plpgsql::{
    Flavor, Origin, ProcSignature, ProcStmt, ProcedureIr, Span, VarDecl, parse_body,
};

/// The table, timing and events a trigger is bound to — what its `CREATE TRIGGER` header says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerSpec {
    pub table: String,
    /// `BEFORE`, `AFTER` or `INSTEAD OF`.
    pub timing: String,
    /// `INSERT`, `UPDATE`, `UPDATE OF a, b`, `DELETE`.
    pub events: Vec<String>,
    /// `ROW` or `STATEMENT`.
    pub level: String,
    /// The `WHEN (…)` condition, without its parentheses.
    pub condition: Option<String>,
}

/// One routine found in a script, with its IR.
#[derive(Debug, Clone)]
pub struct Routine {
    /// Byte offset of the routine's text in the file.
    pub offset: usize,
    /// Byte length of the routine's text.
    pub len: usize,
    /// Bytes of the text before the body: the header that establishes the routine.
    pub header_len: usize,
    pub ir: ProcedureIr,
    /// Set for a trigger's body, which is a routine of its own in Oracle.
    pub trigger: Option<TriggerSpec>,
}

/// Everything a script holds that is PL/SQL.
#[derive(Debug, Clone, Default)]
pub struct ScriptRoutines {
    pub routines: Vec<Routine>,
    /// Anonymous `DECLARE`/`BEGIN` blocks: one-off scripts, counted, not recovered.
    pub anonymous_blocks: usize,
    /// PL/SQL units with no `/` line after them (read to the end of the file, as SQL*Plus does).
    pub unterminated: usize,
    /// Units that define something this front end does not read (`TYPE`, `JAVA SOURCE`), by kind.
    pub skipped: Vec<String>,
}

/// Every routine defined in an Oracle script: functions, procedures, triggers, package members.
pub fn read_script(src: &str) -> ScriptRoutines {
    let mut out = ScriptRoutines::default();
    for unit in units(src).into_iter().filter(|u| u.kind == UnitKind::PlSql) {
        if !unit.terminated {
            out.unterminated += 1;
        }
        // Same length as the source, so every offset still indexes the file.
        let text = mask_comments(&neutralize_quotes(&src[unit.start..unit.end]));
        match read_unit(&text) {
            UnitRead::Routines(rs) => out.routines.extend(rs.into_iter().map(|mut r| {
                r.offset += unit.start;
                r
            })),
            UnitRead::Anonymous => out.anonymous_blocks += 1,
            UnitRead::Skipped(kind) => out.skipped.push(kind),
        }
    }
    out
}

enum UnitRead {
    Routines(Vec<Routine>),
    Anonymous,
    Skipped(String),
}

/// One PL/SQL unit's text (quotes neutralized), starting at `CREATE`, `DECLARE` or `BEGIN`.
fn read_unit(text: &str) -> UnitRead {
    let toks = tokens(text);
    if toks
        .first()
        .is_some_and(|t| t.is("DECLARE") || t.is("BEGIN"))
    {
        return UnitRead::Anonymous;
    }
    let mut i = 1;
    if toks.get(i).is_some_and(|t| t.is("OR")) {
        i += 2;
    }
    while toks
        .get(i)
        .is_some_and(|t| t.is("EDITIONABLE") || t.is("NONEDITIONABLE"))
    {
        i += 1;
    }
    let what = word(toks.get(i));
    match what.as_str() {
        "FUNCTION" | "PROCEDURE" => {
            let reader = Reader { text, toks: &toks };
            UnitRead::Routines(reader.subprogram(i, 0, None).0)
        }
        "TRIGGER" => {
            let text = unbind_row_references(text);
            let toks = tokens(&text);
            let reader = Reader {
                text: &text,
                toks: &toks,
            };
            UnitRead::Routines(reader.trigger(i).into_iter().collect())
        }
        "PACKAGE" => {
            let reader = Reader { text, toks: &toks };
            UnitRead::Routines(reader.package(i))
        }
        other => UnitRead::Skipped(other.to_ascii_lowercase()),
    }
}

fn word(t: Option<&Tok>) -> String {
    match t.map(|t| &t.kind) {
        Some(TokKind::Word(w)) => w.clone(),
        _ => String::new(),
    }
}

/// `:NEW.col` / `:OLD.col` → ` NEW.col` / ` OLD.col`: Oracle's bind-style row references read the
/// way PL/pgSQL's `NEW`/`OLD` do, so the trigger facts RFC 0163 computes apply unchanged. Same
/// length, so no span moves.
fn unbind_row_references(text: &str) -> String {
    let mut b = text.as_bytes().to_vec();
    for i in 0..b.len() {
        if b[i] != b':' {
            continue;
        }
        let rest = &b[i + 1..];
        let is_ref = ["NEW", "OLD", "PARENT"].iter().any(|w| {
            rest.len() >= w.len()
                && rest[..w.len()].eq_ignore_ascii_case(w.as_bytes())
                && rest
                    .get(w.len())
                    .is_none_or(|c| !crate::script::is_ident_byte(*c))
        });
        if is_ref {
            b[i] = b' ';
        }
    }
    String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Squash runs of whitespace to one space.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Words that may follow a routine's `RETURN type` before `IS`/`AS`.
const ROUTINE_MODIFIERS: &[&str] = &[
    "AUTHID",
    "DETERMINISTIC",
    "PIPELINED",
    "PARALLEL_ENABLE",
    "RESULT_CACHE",
    "AGGREGATE",
    "ACCESSIBLE",
    "SHARING",
];

struct Reader<'a> {
    text: &'a str,
    toks: &'a [Tok],
}

/// A routine's name, arguments and return type, and where its header stops.
struct Header {
    name: String,
    arguments: Vec<String>,
    returns: Option<String>,
    /// The token after the header: `IS`/`AS`, or `;` for a declaration without a body.
    next: usize,
}

impl Reader<'_> {
    fn slice(&self, from: usize, to: usize) -> &str {
        &self.text[self.toks[from].start..self.toks[to].end]
    }

    /// The token index of the next `;` at parenthesis depth 0, from `from`.
    fn next_semicolon(&self, from: usize) -> Option<usize> {
        let mut depth = 0i32;
        for (j, t) in self.toks.iter().enumerate().skip(from) {
            if t.is_sym(b'(') {
                depth += 1;
            } else if t.is_sym(b')') {
                depth -= 1;
            } else if depth <= 0 && t.is_sym(b';') {
                return Some(j);
            }
        }
        None
    }

    /// A dotted name starting at `i`, as written (quotes dropped), and the index after it.
    fn name(&self, i: usize) -> (String, usize) {
        let mut parts = Vec::new();
        let mut j = i;
        while let Some(t) = self.toks.get(j) {
            let Some(part) = (match &t.kind {
                TokKind::Word(_) => Some(&self.text[t.start..t.end]),
                TokKind::Quoted(q) => Some(q.as_str()),
                _ => None,
            }) else {
                break;
            };
            parts.push(part.to_string());
            j += 1;
            if !self.toks.get(j).is_some_and(|t| t.is_sym(b'.')) {
                break;
            }
            j += 1;
        }
        (parts.join("."), j)
    }

    /// `FUNCTION|PROCEDURE name [(params)] [RETURN type] [modifiers]`, from the keyword at `at`.
    fn header(&self, at: usize) -> Header {
        let (name, mut j) = self.name(at + 1);
        let mut arguments = Vec::new();
        if self.toks.get(j).is_some_and(|t| t.is_sym(b'(')) {
            let close = matching_paren(self.toks, j).unwrap_or(self.toks.len() - 1);
            let inner = &self.text[self.toks[j].end..self.toks[close].start];
            arguments = split_top_level(inner)
                .into_iter()
                .map(|a| squash(&a))
                .collect();
            j = close + 1;
        }
        let mut returns = None;
        if self.toks.get(j).is_some_and(|t| t.is("RETURN")) {
            let from = j + 1;
            let mut k = from;
            while self.toks.get(k).is_some_and(|t| {
                !(t.is("IS")
                    || t.is("AS")
                    || t.is_sym(b';')
                    || ROUTINE_MODIFIERS.iter().any(|m| t.is(m)))
            }) {
                k += 1;
            }
            if k > from {
                returns = Some(squash(self.slice(from, k - 1)));
            }
            j = k;
        }
        while self
            .toks
            .get(j)
            .is_some_and(|t| !(t.is("IS") || t.is("AS") || t.is_sym(b';')))
        {
            j += 1;
        }
        Header {
            name,
            arguments,
            returns,
            next: j,
        }
    }

    /// A function or procedure from its keyword at `at`: the routine (and any routines nested in
    /// its declarations), and the index of the token that ends it. Spans are relative to `base`'s
    /// token start when `base` is the routine's own first token.
    fn subprogram(&self, at: usize, base_tok: usize, owner: Option<&str>) -> (Vec<Routine>, usize) {
        let h = self.header(at);
        let name = match owner {
            Some(o) => format!("{o}.{}", h.name),
            None => h.name.clone(),
        };
        let base = self.toks[base_tok].start;
        let signature = ProcSignature {
            name: name.clone(),
            arguments: h.arguments.clone(),
            returns: h.returns.clone(),
            language: "plsql".into(),
        };
        let header_len = self.toks.get(h.next).map_or(0, |t| t.start - base);
        let declaration_only = |end: usize, sig: ProcSignature| {
            let len = self.toks.get(end).map_or(self.text.len(), |t| t.end) - base;
            (
                vec![Routine {
                    offset: base,
                    len,
                    header_len: len,
                    ir: ProcedureIr::signature_only(sig),
                    trigger: None,
                }],
                end,
            )
        };
        if !self
            .toks
            .get(h.next)
            .is_some_and(|t| t.is("IS") || t.is("AS"))
        {
            return declaration_only(h.next, signature);
        }
        // `AS LANGUAGE JAVA NAME '…'` / `AS EXTERNAL …`: a call specification, no PL/SQL body.
        if let Some(t) = self.toks.get(h.next + 1)
            && (t.is("LANGUAGE") || t.is("EXTERNAL"))
        {
            let language = if t.is("EXTERNAL") {
                "c".into()
            } else {
                word(self.toks.get(h.next + 2)).to_ascii_lowercase()
            };
            let end = self.next_semicolon(h.next).unwrap_or(self.toks.len() - 1);
            return declaration_only(
                end,
                ProcSignature {
                    language,
                    ..signature
                },
            );
        }
        let (declarations, begin, mut nested) = self.declarations(h.next + 1, &name);
        let Some(begin) = begin else {
            let end = self.toks.len() - 1;
            return declaration_only(end, signature);
        };
        let (body, end) = self.body(begin, base);
        let len = self.toks.get(end).map_or(self.text.len(), |t| t.end) - base;
        let mut routines = vec![Routine {
            offset: base,
            len,
            header_len,
            ir: lifted(signature, rebase(declarations, base), body),
            trigger: None,
        }];
        routines.append(&mut nested);
        (routines, end)
    }

    /// `BEGIN … END [name];` from `begin`: its statements (spans relative to `base`) and the index
    /// of the last token it covers.
    fn body(&self, begin: usize, base: usize) -> (Vec<ProcStmt>, usize) {
        let end = block_end(self.toks, begin).unwrap_or(self.toks.len() - 1);
        // `END [label];`
        let mut last = end;
        if self.toks.get(last + 1).is_some_and(|t| t.ident().is_some()) {
            last += 1;
        }
        if self.toks.get(last + 1).is_some_and(|t| t.is_sym(b';')) {
            last += 1;
        }
        let text = &self.text[self.toks[begin].start..self.toks[last].end];
        let origin = Origin {
            offset: self.toks[begin].start - base,
            flavor: Flavor::PlSql,
        };
        (parse_body(text, origin), last)
    }

    /// The declaration section from `from` up to its `BEGIN`: variables, constants, cursors and
    /// exceptions as declarations (spans relative to the unit), nested subprograms as routines of
    /// their own named `owner.inner`. `None` for the `BEGIN` when the section never reaches one.
    fn declarations(
        &self,
        from: usize,
        owner: &str,
    ) -> (Vec<VarDecl>, Option<usize>, Vec<Routine>) {
        let mut decls = Vec::new();
        let mut nested = Vec::new();
        let mut i = from;
        while let Some(t) = self.toks.get(i) {
            if t.is("BEGIN") {
                return (decls, Some(i), nested);
            }
            if t.is("END") {
                break;
            }
            if t.is("PROCEDURE") || t.is("FUNCTION") {
                let (mut rs, end) = self.subprogram(i, i, Some(owner));
                nested.append(&mut rs);
                i = end + 1;
                continue;
            }
            let Some(end) = self.next_semicolon(i) else {
                break;
            };
            decls.extend(self.declaration(i, end));
            i = end + 1;
        }
        (decls, None, nested)
    }

    /// One declaration, tokens `from..end` (`end` is its `;`).
    fn declaration(&self, from: usize, end: usize) -> Option<VarDecl> {
        let first = &self.toks[from];
        let span = Span {
            start: first.start,
            end: self.toks[end].end,
        };
        if first.is("PRAGMA") || end <= from + 1 {
            return None;
        }
        if first.is("CURSOR") {
            // `CURSOR c [(params)] [RETURN t] IS query`: the query is what the cursor reads.
            let is = (from..end).find(|&k| self.toks[k].is("IS"))?;
            return Some(VarDecl {
                name: self.name(from + 1).0,
                data_type: "CURSOR".into(),
                default: Some(squash(self.slice(is + 1, end - 1))),
                constant: false,
                span,
            });
        }
        if first.is("TYPE") || first.is("SUBTYPE") {
            return Some(VarDecl {
                name: self.name(from + 1).0,
                data_type: squash(self.slice(from, end - 1)),
                default: None,
                constant: false,
                span,
            });
        }
        let (name, mut j) = self.name(from);
        let constant = self.toks.get(j).is_some_and(|t| t.is("CONSTANT"));
        if constant {
            j += 1;
        }
        let assign = (j..end).find(|&k| {
            let t = &self.toks[k];
            t.is("DEFAULT")
                || (t.is_sym(b':') && self.toks.get(k + 1).is_some_and(|n| n.is_sym(b'=')))
        });
        let type_end = assign.unwrap_or(end);
        let data_type = if type_end > j {
            squash(self.slice(j, type_end - 1))
        } else {
            String::new()
        };
        let default = assign.and_then(|k| {
            let from = if self.toks[k].is("DEFAULT") {
                k + 1
            } else {
                k + 2
            };
            (from < end).then(|| squash(self.slice(from, end - 1)))
        });
        Some(VarDecl {
            name,
            data_type: data_type.trim_end_matches("NOT NULL").trim().to_string(),
            default,
            constant,
            span,
        })
    }

    /// `TRIGGER name timing events ON table [REFERENCING …] [FOR EACH ROW] [WHEN (…)] body`.
    fn trigger(&self, at: usize) -> Option<Routine> {
        let (name, mut j) = self.name(at + 1);
        let timing = if self.toks.get(j).is_some_and(|t| t.is("INSTEAD")) {
            j += 2;
            "INSTEAD OF".to_string()
        } else {
            j += 1;
            word(self.toks.get(j - 1))
        };
        let on = (j..self.toks.len()).find(|&k| self.toks[k].is("ON"))?;
        let events = trigger_events(&self.toks[j..on], self.text);
        let (table, mut k) = self.name(on + 1);
        let mut level = "STATEMENT".to_string();
        let mut condition = None;
        while let Some(t) = self.toks.get(k) {
            if t.is("DECLARE") || t.is("BEGIN") || t.is("COMPOUND") || t.is("CALL") {
                break;
            }
            if t.is("ROW") {
                level = "ROW".into();
            }
            if t.is("WHEN") && self.toks.get(k + 1).is_some_and(|n| n.is_sym(b'(')) {
                let close = matching_paren(self.toks, k + 1)?;
                condition = Some(squash(
                    &self.text[self.toks[k + 1].end..self.toks[close].start],
                ));
                k = close;
            }
            k += 1;
        }
        let spec = TriggerSpec {
            table,
            timing,
            events,
            level,
            condition,
        };
        let signature = ProcSignature {
            name,
            arguments: Vec::new(),
            returns: Some("trigger".into()),
            language: "plsql".into(),
        };
        let header_len = self.toks.get(k).map_or(self.text.len(), |t| t.start);
        let start = self.toks.get(k)?;
        let (ir, end) = if start.is("DECLARE") || start.is("BEGIN") {
            let (decls, begin, _) = if start.is("DECLARE") {
                self.declarations(k + 1, &signature.name)
            } else {
                (Vec::new(), Some(k), Vec::new())
            };
            let begin = begin?;
            let (body, end) = self.body(begin, 0);
            (lifted(signature, decls, body), end)
        } else {
            // A compound trigger or `CALL p`: the header is recovered, the body is not.
            (ProcedureIr::signature_only(signature), self.toks.len() - 1)
        };
        Some(Routine {
            offset: 0,
            len: self.toks[end].end,
            header_len,
            ir,
            trigger: Some(spec),
        })
    }

    /// `PACKAGE [BODY] name IS|AS … END [name];`: every function and procedure in it, named
    /// `package.routine`. A specification's members are declarations (signature only); a body's
    /// are read like standalone routines.
    fn package(&self, at: usize) -> Vec<Routine> {
        let body = self.toks.get(at + 1).is_some_and(|t| t.is("BODY"));
        let (name, mut i) = self.name(at + if body { 2 } else { 1 });
        while self
            .toks
            .get(i)
            .is_some_and(|t| !(t.is("IS") || t.is("AS")))
        {
            i += 1;
        }
        i += 1;
        let mut out = Vec::new();
        while let Some(t) = self.toks.get(i) {
            // A body's initialisation section, or the package's own END.
            if t.is("BEGIN") || (t.is("END") && self.at_package_end(i)) {
                break;
            }
            if t.is("PROCEDURE") || t.is("FUNCTION") {
                let (mut rs, end) = self.subprogram(i, i, Some(&name));
                out.append(&mut rs);
                i = end + 1;
                continue;
            }
            i = self.next_semicolon(i).map_or(self.toks.len(), |e| e + 1);
        }
        out
    }

    /// Whether the `END` at `i` closes the package: nothing but an optional name and `;` follow.
    fn at_package_end(&self, i: usize) -> bool {
        let rest: Vec<&Tok> = self.toks[i + 1..].iter().collect();
        match rest.as_slice() {
            [] => true,
            [t] => t.is_sym(b';') || t.ident().is_some(),
            [a, b] => a.ident().is_some() && b.is_sym(b';'),
            _ => false,
        }
    }
}

/// The index of the `END` closing the block that opens at `begin`. `BEGIN`, `CASE`, `IF` and
/// `LOOP` open; `END` closes one, and the `IF`/`LOOP`/`CASE` after `END` belongs to it.
pub fn block_end(toks: &[Tok], begin: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = begin;
    while let Some(t) = toks.get(i) {
        if t.is("END") {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i);
            }
            let closes_named = toks
                .get(i + 1)
                .is_some_and(|n| n.is("IF") || n.is("LOOP") || n.is("CASE"));
            i += if closes_named { 2 } else { 1 };
            continue;
        }
        if t.is("BEGIN") || t.is("CASE") || t.is("IF") || t.is("LOOP") {
            depth += 1;
        }
        i += 1;
    }
    None
}

/// A routine whose body is one `BEGIN … END` without handlers is that block's statements, the
/// shape `ekos_plpgsql::parse_function` produces.
fn lifted(
    signature: ProcSignature,
    declarations: Vec<VarDecl>,
    body: Vec<ProcStmt>,
) -> ProcedureIr {
    if let [
        ProcStmt::Block {
            declarations: inner,
            body: stmts,
            exception,
            ..
        },
    ] = body.as_slice()
        && exception.is_empty()
    {
        let mut all = declarations;
        all.extend(inner.iter().cloned());
        return ProcedureIr::new(signature, all, stmts.clone());
    }
    ProcedureIr::new(signature, declarations, body)
}

/// Declaration spans made relative to the routine at `base`.
fn rebase(mut decls: Vec<VarDecl>, base: usize) -> Vec<VarDecl> {
    for d in &mut decls {
        d.span = Span {
            start: d.span.start - base,
            end: d.span.end - base,
        };
    }
    decls
}

/// `insert OR update of a, b OR DELETE` → `["INSERT", "UPDATE OF a, b", "DELETE"]`: keywords
/// upper-cased, column names as written (the shape the PostgreSQL trigger reader records).
fn trigger_events(toks: &[Tok], text: &str) -> Vec<String> {
    let mut events = Vec::new();
    let mut start: Option<usize> = None;
    let mut last = 0;
    let flush = |s: usize, last: usize, events: &mut Vec<String>| {
        let raw = squash(&text[toks[s].start..toks[last].end]);
        let mut words = raw.splitn(3, ' ');
        let mut event = words.next().unwrap_or_default().to_ascii_uppercase();
        if let Some(of) = words.next() {
            event.push(' ');
            event.push_str(&of.to_ascii_uppercase());
        }
        if let Some(cols) = words.next() {
            event.push(' ');
            event.push_str(cols);
        }
        events.push(event);
    };
    for (k, t) in toks.iter().enumerate() {
        if t.is("OR") {
            if let Some(s) = start.take() {
                flush(s, last, &mut events);
            }
            continue;
        }
        start.get_or_insert(k);
        last = k;
    }
    if let Some(s) = start {
        flush(s, last, &mut events);
    }
    events
}

/// `a NUMBER, b IN VARCHAR2 DEFAULT 'x,y'` split at top-level commas.
fn split_top_level(s: &str) -> Vec<String> {
    let toks = tokens(s);
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut from = 0;
    for t in &toks {
        if t.is_sym(b'(') {
            depth += 1;
        } else if t.is_sym(b')') {
            depth -= 1;
        } else if depth == 0 && t.is_sym(b',') {
            out.push(s[from..t.start].to_string());
            from = t.end;
        }
    }
    if !s[from..].trim().is_empty() {
        out.push(s[from..].to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_plpgsql::Fidelity;

    const INVOICE_OPEN: &str = "CREATE OR REPLACE FUNCTION invoiceOpen
(
    p_C_Invoice_ID              IN  NUMBER,
    p_C_InvoicePaySchedule_ID   IN  NUMBER
)
RETURN NUMBER
/* header comment with a ; and a quote ' */
AS
    v_Currency_ID       NUMBER(10);
    v_Precision         NUMBER := 0;
    CURSOR  Cur_Alloc   IS
        SELECT  a.AD_Client_ID, al.Amount
        FROM    C_AllocationLine al
          INNER JOIN C_AllocationHdr a ON (al.C_AllocationHdr_ID=a.C_AllocationHdr_ID)
        WHERE   al.C_Invoice_ID = p_C_Invoice_ID;
BEGIN
    BEGIN
        SELECT  MAX(C_Currency_ID)
          INTO  v_Currency_ID
        FROM    C_Invoice_v
        WHERE   C_Invoice_ID = p_C_Invoice_ID;
    EXCEPTION
        WHEN OTHERS THEN
            DBMS_OUTPUT.PUT_LINE('InvoiceOpen - ' || SQLERRM);
            RETURN NULL;
    END;
    FOR a IN Cur_Alloc LOOP
        v_Precision := v_Precision + a.Amount;
    END LOOP;
    IF (v_Precision = 0) THEN
        RAISE_APPLICATION_ERROR(-20000, 'none');
    ELSIF v_Precision > 1 THEN
        EXECUTE IMMEDIATE 'UPDATE x SET y = 1' USING v_Precision;
    END IF;
    RETURN v_Precision;
END invoiceOpen;
/
";

    #[test]
    fn a_function_is_read_whole() {
        let s = read_script(INVOICE_OPEN);
        assert_eq!(s.routines.len(), 1);
        let r = &s.routines[0];
        assert_eq!(r.ir.signature.name, "invoiceOpen");
        assert_eq!(
            r.ir.signature.arguments,
            [
                "p_C_Invoice_ID IN NUMBER",
                "p_C_InvoicePaySchedule_ID IN NUMBER"
            ]
        );
        assert_eq!(r.ir.signature.returns.as_deref(), Some("NUMBER"));
        assert_eq!(r.ir.signature.language, "plsql");
        let names: Vec<&str> = r.ir.declarations.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["v_Currency_ID", "v_Precision", "Cur_Alloc"]);
        assert_eq!(r.ir.declarations[1].default.as_deref(), Some("0"));
        assert!(
            r.ir.declarations[2]
                .default
                .as_deref()
                .unwrap()
                .starts_with("SELECT a.AD_Client_ID")
        );
        assert_eq!(r.ir.fidelity(), Fidelity::Statements, "{:#?}", r.ir.body);
        assert!(
            INVOICE_OPEN[r.offset..r.offset + r.len]
                .trim_end()
                .ends_with("END invoiceOpen;")
        );
        assert!(INVOICE_OPEN[..r.header_len].trim_end().ends_with("*/"));
    }

    #[test]
    fn plsql_statement_forms_are_recovered() {
        let r = &read_script(INVOICE_OPEN).routines[0];
        let mut kinds = Vec::new();
        for s in &r.ir.body {
            s.walk(&mut |x| {
                kinds.push(match x {
                    ProcStmt::Perform { .. } => "perform",
                    ProcStmt::Raise { .. } => "raise",
                    ProcStmt::DynamicExecute { .. } => "dynamic",
                    ProcStmt::Loop { .. } => "loop",
                    ProcStmt::Block { .. } => "block",
                    ProcStmt::Sql { .. } => "sql",
                    ProcStmt::If { .. } => "if",
                    ProcStmt::Assign { .. } => "assign",
                    ProcStmt::Return { .. } => "return",
                    _ => "other",
                })
            });
        }
        for k in [
            "perform", "raise", "dynamic", "loop", "block", "sql", "if", "assign", "return",
        ] {
            assert!(kinds.contains(&k), "{k} missing from {kinds:?}");
        }
    }

    #[test]
    fn spans_point_into_the_routine_text() {
        let r = &read_script(INVOICE_OPEN).routines[0];
        let text = &INVOICE_OPEN[r.offset..r.offset + r.len];
        let ProcStmt::Return { span, .. } = r.ir.body.last().unwrap() else {
            panic!("last statement is the RETURN")
        };
        assert!(text[span.start..span.end].starts_with("RETURN v_Precision"));
        let d = &r.ir.declarations[0];
        assert!(text[d.span.start..d.span.end].starts_with("v_Currency_ID"));
    }

    #[test]
    fn a_trigger_body_is_a_routine_with_its_binding() {
        let src = "CREATE OR REPLACE TRIGGER C_BPartner_Trg
BEFORE INSERT OR UPDATE OF Name, Value OR DELETE
ON C_BPartner
FOR EACH ROW
WHEN (new.IsActive = 'Y')
DECLARE
    v_x NUMBER;
BEGIN
    IF INSERTING THEN
        :new.Value := UPPER(:new.Name);
    END IF;
    INSERT INTO AD_ChangeLog (Record_ID) VALUES (:old.C_BPartner_ID);
END;
/
";
        let s = read_script(src);
        assert_eq!(s.routines.len(), 1);
        let r = &s.routines[0];
        let t = r.trigger.as_ref().unwrap();
        assert_eq!(t.table, "C_BPartner");
        assert_eq!(t.timing, "BEFORE");
        assert_eq!(t.events, ["INSERT", "UPDATE OF Name, Value", "DELETE"]);
        assert_eq!(t.level, "ROW");
        assert_eq!(t.condition.as_deref(), Some("new.IsActive = 'Y'"));
        assert_eq!(r.ir.signature.returns.as_deref(), Some("trigger"));
        assert_eq!(r.ir.declarations.len(), 1);
        assert_eq!(r.ir.fidelity(), Fidelity::Statements, "{:#?}", r.ir.body);
        let assigns_new = r.ir.body.iter().any(|s| {
            let mut hit = false;
            s.walk(&mut |x| {
                if let ProcStmt::Assign { target, .. } = x {
                    hit |= target.trim().eq_ignore_ascii_case("new.Value");
                }
            });
            hit
        });
        assert!(assigns_new, "{:#?}", r.ir.body);
    }

    #[test]
    fn packages_name_their_members() {
        let src = "CREATE OR REPLACE PACKAGE acct AS
  FUNCTION balance (p_id NUMBER) RETURN NUMBER;
  PROCEDURE post (p_id IN NUMBER);
END acct;
/
CREATE OR REPLACE PACKAGE BODY acct AS
  FUNCTION balance (p_id NUMBER) RETURN NUMBER IS
  BEGIN
    RETURN 1;
  END balance;
  PROCEDURE post (p_id IN NUMBER) IS
    v NUMBER;
  BEGIN
    UPDATE fact SET posted = 'Y' WHERE id = p_id;
  END post;
END acct;
/
";
        let s = read_script(src);
        let got: Vec<(String, Fidelity)> = s
            .routines
            .iter()
            .map(|r| (r.ir.signature.name.clone(), r.ir.fidelity()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("acct.balance".into(), Fidelity::Signature),
                ("acct.post".into(), Fidelity::Signature),
                ("acct.balance".into(), Fidelity::Statements),
                ("acct.post".into(), Fidelity::Statements),
            ]
        );
    }

    #[test]
    fn nested_subprograms_are_routines_of_their_own() {
        let src = "CREATE PROCEDURE outer_p IS
  v NUMBER;
  FUNCTION helper (x NUMBER) RETURN NUMBER IS
  BEGIN
    IF x > 0 THEN RETURN x; END IF;
    RETURN 0;
  END helper;
BEGIN
  v := helper(1);
  log_it(v);
END;
/
";
        let s = read_script(src);
        let names: Vec<&str> = s
            .routines
            .iter()
            .map(|r| r.ir.signature.name.as_str())
            .collect();
        assert_eq!(names, ["outer_p", "outer_p.helper"]);
        assert_eq!(
            s.routines[0].ir.fidelity(),
            Fidelity::Statements,
            "{:#?}",
            s.routines[0].ir.body
        );
        let helper = &s.routines[1];
        let text = &src[helper.offset..helper.offset + helper.len];
        assert!(
            text.starts_with("FUNCTION helper") && text.ends_with("END helper;"),
            "{text}"
        );
    }

    #[test]
    fn anonymous_blocks_and_call_specs() {
        let src = "BEGIN\n  NULL;\nEND;\n/\nCREATE FUNCTION j (x VARCHAR2) RETURN VARCHAR2 AS LANGUAGE JAVA NAME 'a.b(java.lang.String)';\n/\n";
        let s = read_script(src);
        assert_eq!(s.anonymous_blocks, 1);
        assert_eq!(s.routines.len(), 1);
        assert_eq!(s.routines[0].ir.signature.language, "java");
        assert_eq!(s.routines[0].ir.fidelity(), Fidelity::Signature);
    }

    #[test]
    fn goto_is_an_explicit_gap() {
        let src = "CREATE PROCEDURE g IS\nBEGIN\n  GOTO done;\n  <<done>>\n  NULL;\nEND;\n/\n";
        let r = &read_script(src).routines[0];
        assert!(
            matches!(r.ir.fidelity(), Fidelity::Partial { .. }),
            "{:#?}",
            r.ir.body
        );
    }
}

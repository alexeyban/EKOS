//! RFC 0209 §7 — SQL assembled in Java method bodies, folded, parsed, and attributed to its method.
//!
//! Legacy Java builds SQL in local `String` and `StringBuilder` variables from literals and
//! constants, then hands it to a data-access call. This scanner walks one method body's tokens in
//! order and keeps the text each local holds:
//!
//! - `String v = …;`, `v = …;`, `v += …;`
//! - `StringBuilder v = new StringBuilder(…)` (also `StringBuffer`), `v.append(…)` chains
//!
//! An expression folds `+`-joined parts: string literals, constants (resolved through the
//! caller-supplied [`ConstantLookup`] — the declaring type, its supertypes, a qualified
//! `I_C_Invoice.COLUMNNAME_GrandTotal`), tracked locals, `v.toString()`, and the first argument of
//! `…addAccessSQL(sql, …)` (a row-access filter added at run time). Any other part becomes the
//! placeholder identifier [`PLACEHOLDER`] and the statement is `partial`.
//!
//! At a recognised call — `DB.prepareStatement`, `DB.executeUpdate*`, `DB.getSQLValue*`, … — the
//! argument that folds to SQL is taken; `new Query(ctx, table, where, trx)` becomes
//! `SELECT * FROM table WHERE where`. Each statement is parsed (`statement_footprint_in`); a
//! statement that does not parse is kept with the parser's error, never guessed at.

use crate::java_syntax::{TokKind, Token};
use crate::plpgsql_footprint::{Footprint, statement_footprint_in};
use sqlparser::dialect::GenericDialect;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Stands in for a fragment that is not a literal or a known constant.
pub const PLACEHOLDER: &str = "ekos_expr";

/// Cap on the SQL text kept per statement.
const MAX_TEXT: usize = 4000;

/// `DB` methods whose SQL argument is the first or second argument.
const DB_SQL_METHODS: &[&str] = &[
    "prepareStatement",
    "prepareNormalStatement",
    "executeUpdate",
    "executeUpdateEx",
    "executeUpdateMultiple",
    "getSQLValue",
    "getSQLValueEx",
    "getSQLValueBD",
    "getSQLValueBDEx",
    "getSQLValueString",
    "getSQLValueStringEx",
    "getSQLValueTS",
    "getSQLValueTSEx",
    "getSQLValueObjectsEx",
    "getSQLArrayObjectsEx",
    "getIDsEx",
    "getKeyNamePairs",
    "getKeyNamePairsEx",
    "getValueNamePairs",
    "getRowSet",
];

/// Resolves a constant: `(None, "Table_Name")` from the method's own type and its supertypes,
/// `(Some("I_C_Invoice"), "COLUMNNAME_GrandTotal")` from a named type.
pub trait ConstantLookup {
    fn constant(&self, qualifier: Option<&str>, name: &str) -> Option<String>;
}

impl<F: Fn(Option<&str>, &str) -> Option<String>> ConstantLookup for F {
    fn constant(&self, qualifier: Option<&str>, name: &str) -> Option<String> {
        self(qualifier, name)
    }
}

/// One SQL statement a method hands to the database.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmbeddedSql {
    pub line: u32,
    /// `DB.prepareStatement`, `new Query`, …
    pub call: String,
    pub text: String,
    /// `parsed`, `partial` (placeholders in the text) or `unparsed` (parser error kept).
    pub status: &'static str,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub reads: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub inserts: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub updates: BTreeSet<String>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub deletes: BTreeSet<String>,
    /// RFC 0212: the columns each updated table's `SET` names.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub update_sets: BTreeMap<String, BTreeSet<String>>,
    /// RFC 0212: how each deleted table's rows are chosen (`all_rows`, `age`, `selective`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub delete_shapes: BTreeMap<String, BTreeSet<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Folded {
    text: String,
    partial: bool,
}

impl Folded {
    fn push(&mut self, other: Folded) {
        self.text.push_str(&other.text);
        self.partial |= other.partial;
    }
    fn placeholder() -> Self {
        Self {
            text: PLACEHOLDER.into(),
            partial: true,
        }
    }
}

struct Scanner<'a> {
    t: &'a [Token],
    /// Closing index of every `(` in the body.
    close: HashMap<usize, usize>,
    vars: HashMap<String, Folded>,
    lookup: &'a dyn ConstantLookup,
}

/// Every SQL statement in the body `tokens[open+1 .. close]`.
pub fn harvest(
    tokens: &[Token],
    (open, close): (usize, usize),
    lookup: &dyn ConstantLookup,
) -> Vec<EmbeddedSql> {
    let body = &tokens[open + 1..close.min(tokens.len())];
    let mut parens = HashMap::new();
    let mut stack = Vec::new();
    for (i, t) in body.iter().enumerate() {
        if t.is("(") {
            stack.push(i);
        } else if t.is(")")
            && let Some(o) = stack.pop()
        {
            parens.insert(o, i);
        }
    }
    let mut s = Scanner {
        t: body,
        close: parens,
        vars: HashMap::new(),
        lookup,
    };
    s.scan()
}

fn looks_like_sql(text: &str) -> bool {
    let head: String = text
        .trim_start()
        .trim_start_matches('(')
        .chars()
        .take(7)
        .collect::<String>()
        .to_ascii_uppercase();
    ["SELECT", "UPDATE", "DELETE", "INSERT", "WITH", "MERGE"]
        .iter()
        .any(|k| {
            head.starts_with(k)
                && head[k.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
}

impl Scanner<'_> {
    fn is(&self, i: usize, p: &str) -> bool {
        self.t.get(i).is_some_and(|t| t.is(p))
    }

    fn ident(&self, i: usize) -> Option<&str> {
        self.t
            .get(i)
            .filter(|t| t.kind == TokKind::Ident)
            .map(|t| t.text.as_str())
    }

    /// End of the expression starting at `i`: the first `,`, `;` or unmatched `)` at depth 0.
    fn expr_end(&self, mut i: usize) -> usize {
        let mut depth = 0i32;
        while let Some(t) = self.t.get(i) {
            if t.is("(") || t.is("[") || t.is("{") {
                depth += 1;
            } else if t.is(")") || t.is("]") || t.is("}") {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            } else if depth == 0 && (t.is(",") || t.is(";")) {
                return i;
            }
            i += 1;
        }
        i
    }

    /// Arguments of the call whose `(` is at `open`, as `[start, end)` ranges.
    fn args(&self, open: usize) -> Vec<(usize, usize)> {
        let Some(&close) = self.close.get(&open) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut i = open + 1;
        while i < close {
            let e = self.expr_end(i).min(close);
            out.push((i, e));
            i = e + 1;
        }
        out
    }

    /// Fold `[from, to)`: parts joined by top-level `+`.
    fn fold(&self, from: usize, to: usize) -> Folded {
        let mut out = Folded::default();
        let mut start = from;
        let mut depth = 0i32;
        for i in from..=to {
            let at_end = i == to;
            if !at_end {
                let t = &self.t[i];
                if t.is("(") || t.is("[") {
                    depth += 1;
                } else if t.is(")") || t.is("]") {
                    depth -= 1;
                }
            }
            if at_end || (depth == 0 && self.t[i].is("+")) {
                if start < i {
                    out.push(self.part(start, i));
                } else if !at_end {
                    // Unary `+`, or a leading `+`: nothing to fold.
                }
                start = i + 1;
            }
        }
        out
    }

    /// One `+`-operand.
    fn part(&self, from: usize, to: usize) -> Folded {
        let len = to - from;
        let t = &self.t[from];
        if len == 1 {
            return match t.kind {
                TokKind::Str => Folded {
                    text: t.text.clone(),
                    partial: false,
                },
                TokKind::Num => Folded {
                    text: t.text.trim_end_matches(['L', 'l']).to_string(),
                    partial: false,
                },
                TokKind::Char => Folded {
                    text: t.text.clone(),
                    partial: false,
                },
                TokKind::Ident => self.name(None, &t.text),
                TokKind::Punct => Folded::placeholder(),
            };
        }
        // `( … )`
        if t.is("(") && self.close.get(&from) == Some(&(to - 1)) {
            return self.fold(from + 1, to - 1);
        }
        // `Q.NAME` / `a.b.Q.NAME`
        if (3..=9).contains(&len)
            && (from..to).all(|i| {
                let k = i - from;
                if k.is_multiple_of(2) {
                    self.ident(i).is_some()
                } else {
                    self.is(i, ".")
                }
            })
            && !len.is_multiple_of(2)
        {
            let qualifier = &self.t[to - 3].text;
            return self.name(Some(qualifier), &self.t[to - 1].text);
        }
        // `v.toString()`
        if len == 5
            && let Some(v) = self.ident(from)
            && self.is(from + 1, ".")
            && self.is(from + 2, "toString")
            && self.is(from + 3, "(")
            && self.is(from + 4, ")")
            && let Some(f) = self.vars.get(v)
        {
            return f.clone();
        }
        // `….addAccessSQL(sql, …)` — the SQL is the first argument.
        if let Some(k) = (from..to).find(|&i| self.is(i, "addAccessSQL") && self.is(i + 1, "("))
            && self.close.get(&(k + 1)) == Some(&(to - 1))
            && let Some(&(s, e)) = self.args(k + 1).first()
        {
            return self.fold(s, e);
        }
        Folded::placeholder()
    }

    fn name(&self, qualifier: Option<&str>, name: &str) -> Folded {
        if qualifier.is_none()
            && let Some(v) = self.vars.get(name)
        {
            return v.clone();
        }
        match self.lookup.constant(qualifier, name) {
            Some(text) => Folded {
                text,
                partial: false,
            },
            None => Folded::placeholder(),
        }
    }

    fn scan(&mut self) -> Vec<EmbeddedSql> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.t.len() {
            if let Some(next) = self.local(i) {
                i = next;
            } else if let Some((next, st)) = self.db_call(i).or_else(|| self.new_query(i)) {
                out.extend(st);
                i = next;
            } else {
                i += 1;
            }
        }
        out
    }

    /// A statement that changes a tracked local's text; returns the index after it.
    fn local(&mut self, i: usize) -> Option<usize> {
        // `String v = …` / `StringBuilder v = new StringBuilder(…)…`
        if let Some(ty) = self.ident(i)
            && matches!(ty, "String" | "StringBuilder" | "StringBuffer")
            && let Some(v) = self.ident(i + 1)
            && self.is(i + 2, "=")
        {
            let v = v.to_string();
            let end = self.expr_end(i + 3);
            let value = self.initial_value(i + 3, end);
            self.vars.insert(v, value);
            return Some(end);
        }
        let v = self
            .ident(i)
            .filter(|v| self.vars.contains_key(*v))?
            .to_string();
        let (end, more) = if self.is(i + 1, "=") && !self.is(i + 2, "=") {
            // `v = …` replaces the text.
            let end = self.expr_end(i + 2);
            let value = self.initial_value(i + 2, end);
            self.vars.insert(v, value);
            return Some(end);
        } else if self.is(i + 1, "+") && self.is(i + 2, "=") {
            let end = self.expr_end(i + 3);
            (end, self.fold(i + 3, end))
        } else if self.is(i + 1, ".") && self.is(i + 2, "append") {
            let (more, end) = self.appends(i + 1);
            (end, more)
        } else {
            return None;
        };
        self.vars.get_mut(&v).expect("checked").push(more);
        Some(end)
    }

    /// `DB.<method>(…)` with a SQL argument among its first two.
    fn db_call(&self, i: usize) -> Option<(usize, Option<EmbeddedSql>)> {
        let m = self.ident(i + 2)?;
        if !(self.is(i, "DB")
            && self.is(i + 1, ".")
            && DB_SQL_METHODS.contains(&m)
            && self.is(i + 3, "("))
        {
            return None;
        }
        let st = self
            .args(i + 3)
            .iter()
            .take(2)
            .map(|&(s, e)| self.fold(s, e))
            .find(|f| looks_like_sql(&f.text))
            .map(|f| statement(self.t[i].line, format!("DB.{m}"), f));
        Some((i + 4, st))
    }

    /// `new Query(ctx, table, where, trx)` as `SELECT * FROM table WHERE where`.
    fn new_query(&self, i: usize) -> Option<(usize, Option<EmbeddedSql>)> {
        if !(self.is(i, "new") && self.is(i + 1, "Query") && self.is(i + 2, "(")) {
            return None;
        }
        let args = self.args(i + 2);
        if args.len() != 4 {
            return None;
        }
        let table = self.fold(args[1].0, args[1].1);
        let where_ = self.fold(args[2].0, args[2].1);
        let mut text = format!("SELECT * FROM {}", table.text);
        let empty_where = where_.text.trim().is_empty()
            || (args[2].1 == args[2].0 + 1 && self.is(args[2].0, "null"));
        if !empty_where {
            text.push_str(" WHERE ");
            text.push_str(&where_.text);
        }
        let f = Folded {
            text,
            partial: table.partial || (!empty_where && where_.partial),
        };
        Some((
            i + 3,
            Some(statement(self.t[i].line, "new Query".into(), f)),
        ))
    }

    /// The value of `new StringBuilder(…).append(…)…` or a plain expression.
    fn initial_value(&self, from: usize, to: usize) -> Folded {
        if self.is(from, "new")
            && matches!(self.ident(from + 1), Some("StringBuilder" | "StringBuffer"))
            && self.is(from + 2, "(")
        {
            let Some(&close) = self.close.get(&(from + 2)) else {
                return Folded::placeholder();
            };
            let mut value = if close == from + 3 {
                Folded::default()
            } else {
                let f = self.fold(from + 3, close);
                // `new StringBuilder(256)` is a capacity, not text.
                if close == from + 4 && self.t[from + 3].kind == TokKind::Num {
                    Folded::default()
                } else {
                    f
                }
            };
            if self.is(close + 1, ".") {
                let (more, _) = self.appends(close + 1);
                value.push(more);
            }
            return value;
        }
        self.fold(from, to)
    }

    /// `.append(a).append(b)…` starting at the `.` at `i`; returns the text and the index after.
    fn appends(&self, mut i: usize) -> (Folded, usize) {
        let mut out = Folded::default();
        while self.is(i, ".") && self.is(i + 1, "append") && self.is(i + 2, "(") {
            let Some(&close) = self.close.get(&(i + 2)) else {
                break;
            };
            if close > i + 3 {
                out.push(self.fold(i + 3, close));
            }
            i = close + 1;
        }
        (out, i)
    }
}

fn statement(line: u32, call: String, f: Folded) -> EmbeddedSql {
    let fp: Footprint = statement_footprint_in(&GenericDialect {}, &f.text);
    let clean = |set: BTreeSet<String>| -> BTreeSet<String> {
        set.into_iter()
            .filter(|n| !n.eq_ignore_ascii_case(PLACEHOLDER))
            .collect()
    };
    let error = fp.errors.first().cloned();
    let status = if error.is_some() {
        "unparsed"
    } else if f.partial {
        "partial"
    } else {
        "parsed"
    };
    let mut text = f.text;
    if text.len() > MAX_TEXT {
        let mut cut = MAX_TEXT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    EmbeddedSql {
        line,
        call,
        text,
        status,
        reads: clean(fp.reads),
        inserts: clean(fp.inserts),
        updates: clean(fp.updates),
        deletes: clean(fp.deletes),
        update_sets: fp
            .update_sets
            .into_iter()
            .filter(|(t, _)| !t.eq_ignore_ascii_case(PLACEHOLDER))
            .collect(),
        delete_shapes: fp
            .delete_shapes
            .into_iter()
            .filter(|(t, _)| !t.eq_ignore_ascii_case(PLACEHOLDER))
            .collect(),
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java_syntax::parse;

    fn lookup(q: Option<&str>, name: &str) -> Option<String> {
        match (q, name) {
            (None, "Table_Name") => Some("C_Invoice".into()),
            (None, "COLUMNNAME_C_BPartner_ID") => Some("C_BPartner_ID".into()),
            (Some("I_C_InvoiceLine"), "Table_Name") => Some("C_InvoiceLine".into()),
            (Some("I_C_InvoiceLine"), "COLUMNNAME_C_Invoice_ID") => Some("C_Invoice_ID".into()),
            _ => None,
        }
    }

    fn harvest_method(body: &str) -> Vec<EmbeddedSql> {
        let src = format!("class A {{ void m() {{ {body} }} }}");
        let f = parse(&src);
        let m = &f.types[0].methods[0];
        harvest(&f.tokens, m.body.unwrap(), &lookup)
    }

    #[test]
    fn literals_and_constants_fold_into_a_parsed_statement() {
        let s = harvest_method(
            r#"String sql = "SELECT C_DocType_ID FROM C_DocType "
                + "WHERE AD_Client_ID=? AND DocBaseType=?";
            int id = DB.getSQLValueEx(null, sql, getAD_Client_ID(), DocBaseType);"#,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].call, "DB.getSQLValueEx");
        assert_eq!(s[0].status, "parsed");
        assert_eq!(s[0].reads, BTreeSet::from(["c_doctype".to_string()]));
    }

    #[test]
    fn string_builder_chains_and_to_string_are_followed() {
        let s = harvest_method(
            r#"StringBuilder sql = new StringBuilder("UPDATE C_InvoiceLine ol")
                .append(" SET AD_Org_ID = ").append(getAD_Org_ID());
            sql.append(" WHERE C_Invoice_ID=").append(getC_Invoice_ID());
            int no = DB.executeUpdate(sql.toString(), get_TrxName());"#,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].status, "partial", "{}", s[0].text);
        assert_eq!(s[0].updates, BTreeSet::from(["c_invoiceline".to_string()]));
        assert!(s[0].text.contains("WHERE C_Invoice_ID=ekos_expr"));
    }

    #[test]
    fn delete_and_insert_are_split_by_operation() {
        let s = harvest_method(
            r#"String sql = "DELETE FROM " + Table_Name + " WHERE Processed='N'";
            DB.executeUpdateEx(sql, get_TrxName());
            sql = "INSERT INTO T_Report (AD_PInstance_ID) SELECT ? FROM dual";
            DB.executeUpdateEx(sql, null);"#,
        );
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].deletes, BTreeSet::from(["c_invoice".to_string()]));
        assert_eq!(s[0].status, "parsed");
        assert_eq!(s[1].inserts, BTreeSet::from(["t_report".to_string()]));
    }

    #[test]
    fn new_query_becomes_a_select_with_its_where_clause() {
        let s = harvest_method(
            r#"List<MInvoiceLine> list = new Query(getCtx(), I_C_InvoiceLine.Table_Name,
                I_C_InvoiceLine.COLUMNNAME_C_Invoice_ID + "=?", get_TrxName()).list();
            List<MInvoice> all = new Query(ctx, Table_Name, COLUMNNAME_C_BPartner_ID+"=?", trxName).list();"#,
        );
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].text,
            "SELECT * FROM C_InvoiceLine WHERE C_Invoice_ID=?"
        );
        assert_eq!(s[0].status, "parsed");
        assert_eq!(s[1].reads, BTreeSet::from(["c_invoice".to_string()]));
    }

    #[test]
    fn access_sql_wrapper_keeps_the_inner_statement() {
        let s = harvest_method(
            r#"String sql = MRole.getDefault().addAccessSQL("SELECT * FROM C_Invoice i WHERE i.IsPaid='N'",
                "i", MRole.SQL_FULLYQUALIFIED, MRole.SQL_RO);
            PreparedStatement p = DB.prepareStatement(sql, trxName);"#,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].status, "parsed");
        assert_eq!(s[0].reads, BTreeSet::from(["c_invoice".to_string()]));
    }

    #[test]
    fn a_table_from_an_unknown_expression_is_partial_and_never_named() {
        let s = harvest_method(
            r#"String sql = "SELECT COUNT(*) FROM " + tableName + " WHERE IsActive='Y'";
            int n = DB.getSQLValue(null, sql);"#,
        );
        assert_eq!(s[0].status, "partial");
        assert!(s[0].reads.is_empty(), "{:?}", s[0].reads);
    }

    #[test]
    fn a_call_with_no_sql_argument_records_nothing() {
        let s = harvest_method("int n = DB.getSQLValue(null, getSql(), 1);");
        assert!(s.is_empty());
    }

    #[test]
    fn a_statement_that_does_not_parse_keeps_its_error() {
        let s = harvest_method(r#"DB.executeUpdate("UPDATE SET WHERE", null);"#);
        assert_eq!(s[0].status, "unparsed");
        assert!(s[0].error.is_some());
    }
}

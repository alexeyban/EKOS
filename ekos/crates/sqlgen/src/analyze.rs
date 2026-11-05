//! One walk over a statement's AST: column references resolved through aliases, CTEs, derived
//! tables and correlated scopes, and the facts the checks need — comparisons with literals, join
//! conditions, divisions with their operand types, aggregates, `ORDER BY … LIMIT`.
//!
//! The walk is deliberately conservative: a reference it cannot resolve for sure (one that may
//! come from a CTE or a derived table) is left unresolved rather than reported.

use crate::schema::Schema;
use sqlparser::ast::{
    BinaryOperator, CastKind, DataType, Distinct, Expr, Function, FunctionArg, FunctionArgExpr,
    FunctionArguments, GroupByExpr, Ident, Query, Select, SelectItem, SetExpr, Statement,
    TableFactor, TableWithJoins, UnaryOperator, Value,
};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use std::collections::BTreeSet;

/// A column of a real table, `(table, column)` as the schema spells them.
pub type Col = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    In,
    /// `<`, `>`, `<=`, `>=`, `BETWEEN`.
    Range,
}

/// `column <op> 'literal'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub col: Col,
    pub op: CmpOp,
    pub literal: String,
}

/// A reference no table in scope has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub qualifier: Option<String>,
    pub column: String,
    /// The table the qualifier names, when it names one.
    pub table: Option<String>,
    /// Written in double quotes — SQLite reads such a name that is no column as a string.
    pub double_quoted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumKind {
    Int,
    Real,
    Unknown,
}

/// `ORDER BY col … LIMIT n` on a plain column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLimit {
    pub col: Col,
    pub ascending: bool,
    /// `NULLS FIRST/LAST` was written.
    pub explicit_nulls: bool,
    pub limit: u64,
}

/// What the top-level query looks like, for the probes.
#[derive(Debug, Clone, Default)]
pub struct TopLevel {
    /// An aggregate (`COUNT`, `SUM`, `AVG`, `TOTAL`) in the projection.
    pub aggregate: bool,
    /// `COUNT(DISTINCT …)` somewhere in the projection.
    pub count_distinct: bool,
    pub distinct: bool,
    pub group_by: bool,
    /// More than one relation in `FROM`.
    pub joined: bool,
    /// The first `FROM` relation: `(name or alias as written, real table)`.
    pub first: Option<(String, String)>,
    pub limit: Option<u64>,
    pub has_order: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub unknown_tables: Vec<String>,
    pub unresolved: Vec<Unresolved>,
    pub comparisons: Vec<Comparison>,
    /// `a.x = b.y` between two different real tables (ON or WHERE).
    pub joins: Vec<(Col, Col)>,
    /// Both operands and the division as written.
    pub divisions: Vec<(NumKind, NumKind, String)>,
    /// `(function, column)` for `AVG(col)`, `MIN(col)`, …
    pub aggregates: Vec<(String, Col)>,
    /// Columns the query filters so NULL (or a sentinel) cannot pass: `IS NOT NULL`, or any
    /// comparison.
    pub filtered: BTreeSet<Col>,
    pub order_limits: Vec<OrderLimit>,
    /// RFC 0243: the row-limit forms used anywhere: `LIMIT`, `FETCH`, `TOP`.
    pub row_limits: BTreeSet<&'static str>,
    /// RFC 0243: `ILIKE` is used.
    pub ilike: bool,
    /// RFC 0243: `"text"` read as a string literal (MySQL's default mode).
    pub double_quoted_strings: Vec<String>,
    pub top: TopLevel,
    /// RFC 0243: each real table (lower-case) as the query names it (`sqlgen.customers`), so a
    /// probe queries the same object in a schema-qualified database.
    pub written: std::collections::BTreeMap<String, String>,
    /// The parsed statement, for probes that rewrite it.
    pub query: Option<Box<Query>>,
}

/// Parses `sql` (one statement) and walks it. `Err` holds the parser's message.
pub fn analyze(sql: &str, dialect: &dyn Dialect, schema: &Schema) -> Result<Analysis, String> {
    let mut stmts = Parser::parse_sql(dialect, sql).map_err(|e| e.to_string())?;
    if stmts.len() != 1 {
        return Err(format!("expected one statement, found {}", stmts.len()));
    }
    let Statement::Query(q) = stmts.remove(0) else {
        return Err("not a query".into());
    };
    let mut w = Walker {
        schema,
        scopes: Vec::new(),
        ctes: BTreeSet::new(),
        out: Analysis::default(),
    };
    w.top_level(&q);
    w.query(&q);
    let mut out = w.out;
    out.query = Some(q);
    Ok(out)
}

/// One relation in a `FROM`: the name it is referred to by and the real table, if it is one.
#[derive(Debug, Clone)]
struct Source {
    name: String,
    table: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Scope {
    sources: Vec<Source>,
    /// Projection aliases (usable in ORDER BY / HAVING / WHERE in SQLite).
    aliases: BTreeSet<String>,
}

enum Resolved {
    Col(Col),
    /// From a CTE, a derived table or an alias: not checked.
    Opaque,
    Missing(Unresolved),
}

struct Walker<'a> {
    schema: &'a Schema,
    scopes: Vec<Scope>,
    ctes: BTreeSet<String>,
    out: Analysis,
}

const BARE_KEYWORDS: &[&str] = &[
    "current_date",
    "current_time",
    "current_timestamp",
    "true",
    "false",
    "null",
    "rowid",
    "oid",
    "_rowid_",
];

impl Walker<'_> {
    fn top_level(&mut self, q: &Query) {
        let SetExpr::Select(s) = q.body.as_ref() else {
            return;
        };
        let t = &mut self.out.top;
        t.distinct = matches!(s.distinct, Some(Distinct::Distinct));
        t.group_by = matches!(&s.group_by, GroupByExpr::Expressions(e, _) if !e.is_empty())
            || matches!(&s.group_by, GroupByExpr::All(_));
        t.joined = s.from.len() > 1 || s.from.iter().any(|f| !f.joins.is_empty());
        t.limit = q.limit.as_ref().and_then(literal_u64);
        t.has_order = q.order_by.as_ref().is_some_and(|o| !o.exprs.is_empty());
        if let Some(TableFactor::Table { name, alias, .. }) = s.from.first().map(|f| &f.relation)
            && let Some(real) = self.schema.table(&name.to_string())
        {
            let written = alias
                .as_ref()
                .map(|a| a.name.value.clone())
                .unwrap_or_else(|| name.to_string());
            t.first = Some((written, real.name.clone()));
        }
        for item in &s.projection {
            if let SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } = item {
                let (agg, cd) = aggregate_in(e);
                t.aggregate |= agg;
                t.count_distinct |= cd;
            }
        }
    }

    fn query(&mut self, q: &Query) {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                self.ctes.insert(cte.alias.name.value.to_lowercase());
                self.query(&cte.query);
            }
        }
        if q.limit.is_some() {
            self.out.row_limits.insert("LIMIT");
        }
        if q.fetch.is_some() {
            self.out.row_limits.insert("FETCH");
        }
        let scope = self.set_expr(&q.body);
        if let Some(order) = &q.order_by {
            if let Some(scope) = scope.clone() {
                self.scopes.push(scope);
            }
            let limit = q.limit.as_ref().and_then(literal_u64);
            for o in &order.exprs {
                self.expr(&o.expr);
                if let (Some(limit), Some(Resolved::Col(col))) = (limit, self.plain(&o.expr)) {
                    self.out.order_limits.push(OrderLimit {
                        col,
                        ascending: o.asc != Some(false),
                        explicit_nulls: o.nulls_first.is_some(),
                        limit,
                    });
                }
            }
            if scope.is_some() {
                self.scopes.pop();
            }
        }
    }

    /// Walks a query body; returns the scope of a plain `SELECT` for its `ORDER BY`.
    fn set_expr(&mut self, body: &SetExpr) -> Option<Scope> {
        match body {
            SetExpr::Select(s) => Some(self.select(s)),
            SetExpr::Query(q) => {
                self.query(q);
                None
            }
            SetExpr::SetOperation { left, right, .. } => {
                self.set_expr(left);
                self.set_expr(right);
                None
            }
            _ => None,
        }
    }

    fn select(&mut self, s: &Select) -> Scope {
        if s.top.is_some() {
            self.out.row_limits.insert("TOP");
        }
        let mut scope = Scope::default();
        let mut on_exprs: Vec<&Expr> = Vec::new();
        for twj in &s.from {
            self.from(twj, &mut scope, &mut on_exprs);
        }
        for item in &s.projection {
            if let SelectItem::ExprWithAlias { alias, .. } = item {
                scope.aliases.insert(alias.value.to_lowercase());
            }
        }
        self.scopes.push(scope);
        for item in &s.projection {
            if let SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } = item {
                self.expr(e);
            }
        }
        for e in on_exprs {
            self.expr(e);
        }
        if let Some(w) = &s.selection {
            self.expr(w);
        }
        if let GroupByExpr::Expressions(exprs, _) = &s.group_by {
            for e in exprs {
                self.expr(e);
            }
        }
        if let Some(h) = &s.having {
            self.expr(h);
        }
        self.scopes.pop().unwrap_or_default()
    }

    fn from<'q>(&mut self, twj: &'q TableWithJoins, scope: &mut Scope, on: &mut Vec<&'q Expr>) {
        self.relation(&twj.relation, scope);
        for j in &twj.joins {
            self.relation(&j.relation, scope);
            use sqlparser::ast::{JoinConstraint, JoinOperator as J};
            let c = match &j.join_operator {
                J::Inner(c)
                | J::LeftOuter(c)
                | J::RightOuter(c)
                | J::FullOuter(c)
                | J::Semi(c)
                | J::LeftSemi(c)
                | J::RightSemi(c)
                | J::Anti(c)
                | J::LeftAnti(c)
                | J::RightAnti(c) => Some(c),
                _ => None,
            };
            if let Some(JoinConstraint::On(e)) = c {
                on.push(e);
            }
        }
    }

    fn relation(&mut self, r: &TableFactor, scope: &mut Scope) {
        match r {
            TableFactor::Table { name, alias, .. } => {
                let written = name.to_string();
                let bare = name
                    .0
                    .last()
                    .map(|i| i.value.to_lowercase())
                    .unwrap_or_default();
                let real = if self.ctes.contains(&bare) {
                    None
                } else {
                    match self.schema.table(&written) {
                        Some(t) => {
                            self.out
                                .written
                                .entry(t.name.to_lowercase())
                                .or_insert_with(|| written.clone());
                            Some(t.name.clone())
                        }
                        None => {
                            self.out.unknown_tables.push(written.clone());
                            None
                        }
                    }
                };
                let refname = alias
                    .as_ref()
                    .map(|a| a.name.value.clone())
                    .unwrap_or_else(|| name.0.last().map(|i| i.value.clone()).unwrap_or_default());
                scope.sources.push(Source {
                    name: refname,
                    table: real,
                });
            }
            TableFactor::Derived {
                subquery, alias, ..
            } => {
                self.query(subquery);
                scope.sources.push(Source {
                    name: alias
                        .as_ref()
                        .map(|a| a.name.value.clone())
                        .unwrap_or_default(),
                    table: None,
                });
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => {
                let mut on = Vec::new();
                self.from(table_with_joins, scope, &mut on);
                let on: Vec<Expr> = on.into_iter().cloned().collect();
                self.scopes.push(scope.clone());
                for e in &on {
                    self.expr(e);
                }
                self.scopes.pop();
            }
            _ => scope.sources.push(Source {
                name: String::new(),
                table: None,
            }),
        }
    }

    /// Resolves a plain column reference (`c`, `q.c`); `None` when `e` is not one.
    fn plain(&self, e: &Expr) -> Option<Resolved> {
        match e {
            Expr::Nested(inner) => self.plain(inner),
            Expr::Identifier(id) => Some(self.bare(id)),
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                Some(self.qualified(&parts[parts.len() - 2].value, &parts[parts.len() - 1].value))
            }
            _ => None,
        }
    }

    /// An unqualified name: a projection alias, else a column of the innermost scope that has
    /// one; a scope with a CTE or derived source and no match makes it opaque.
    fn bare(&self, id: &Ident) -> Resolved {
        let name = id.value.to_lowercase();
        if BARE_KEYWORDS.contains(&name.as_str()) && id.quote_style.is_none() {
            return Resolved::Opaque;
        }
        for scope in self.scopes.iter().rev() {
            if scope.aliases.contains(&name) {
                return Resolved::Opaque;
            }
            if let Some(found) = self.in_scope(scope, &id.value) {
                return found;
            }
        }
        Resolved::Missing(Unresolved {
            qualifier: None,
            column: id.value.clone(),
            table: None,
            double_quoted: id.quote_style == Some('"'),
        })
    }

    /// The first real table of `scope` with column `name`; opaque when none has it but a
    /// source's columns are unknown; `None` to look further out.
    fn in_scope(&self, scope: &Scope, name: &str) -> Option<Resolved> {
        let found = scope.sources.iter().find_map(|s| {
            let t = s.table.as_ref()?;
            self.schema
                .column(t, name)
                .map(|c| (t.clone(), c.name.clone()))
        });
        match found {
            Some(col) => Some(Resolved::Col(col)),
            None if scope.sources.iter().any(|s| s.table.is_none()) => Some(Resolved::Opaque),
            None => None,
        }
    }

    /// `q.c`: the source named `q` in the innermost scope that has one.
    fn qualified(&self, q: &str, c: &str) -> Resolved {
        let source = self.scopes.iter().rev().find_map(|scope| {
            scope
                .sources
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(q))
        });
        let missing = |table: Option<String>| {
            Resolved::Missing(Unresolved {
                qualifier: Some(q.to_string()),
                column: c.to_string(),
                table,
                double_quoted: false,
            })
        };
        match source.map(|s| &s.table) {
            None => missing(None),
            Some(None) => Resolved::Opaque,
            Some(Some(t)) => match self.schema.column(t, c) {
                Some(col) => Resolved::Col((t.clone(), col.name.clone())),
                None => missing(Some(t.clone())),
            },
        }
    }

    fn col(&self, e: &Expr) -> Option<Col> {
        match self.plain(e)? {
            Resolved::Col(c) => Some(c),
            _ => None,
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => self.reference(e),
            Expr::BinaryOp { left, op, right } => {
                self.binary(left, op, right);
                self.exprs([left.as_ref(), right.as_ref()]);
            }
            Expr::IsNotNull(inner) => {
                self.filter(inner);
                self.expr(inner);
            }
            Expr::InList {
                expr,
                list,
                negated,
            } => {
                if !negated {
                    self.literals(expr, list.iter(), CmpOp::In);
                }
                self.expr(expr);
                self.exprs(list.iter());
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                self.literals(expr, [low.as_ref(), high.as_ref()], CmpOp::Range);
                self.exprs([expr.as_ref(), low.as_ref(), high.as_ref()]);
            }
            Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => {
                self.out.ilike |= matches!(e, Expr::ILike { .. });
                self.filter(expr);
                self.exprs([expr.as_ref(), pattern.as_ref()]);
            }
            Expr::InSubquery { expr, subquery, .. } => {
                self.expr(expr);
                self.query(subquery);
            }
            Expr::Exists { subquery, .. } | Expr::Subquery(subquery) => self.query(subquery),
            Expr::Function(f) => self.function(f),
            Expr::Nested(inner)
            | Expr::UnaryOp { expr: inner, .. }
            | Expr::IsNull(inner)
            | Expr::IsTrue(inner)
            | Expr::IsFalse(inner)
            | Expr::IsNotTrue(inner)
            | Expr::IsNotFalse(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Collate { expr: inner, .. }
            | Expr::Substring { expr: inner, .. }
            | Expr::Trim { expr: inner, .. } => self.expr(inner),
            Expr::Case {
                operand,
                conditions,
                results,
                else_result,
            } => self.exprs(
                operand
                    .as_deref()
                    .into_iter()
                    .chain(conditions)
                    .chain(results)
                    .chain(else_result.as_deref()),
            ),
            Expr::Tuple(items) => self.exprs(items.iter()),
            Expr::Value(Value::DoubleQuotedString(s)) => {
                self.out.double_quoted_strings.push(s.clone())
            }
            _ => {}
        }
    }

    fn exprs<'e>(&mut self, items: impl IntoIterator<Item = &'e Expr>) {
        for e in items {
            self.expr(e);
        }
    }

    /// Records a reference no scope has.
    fn reference(&mut self, e: &Expr) {
        if let Some(Resolved::Missing(u)) = self.plain(e)
            && !self.out.unresolved.contains(&u)
        {
            self.out.unresolved.push(u);
        }
    }

    /// `e` is a column the query filters on.
    fn filter(&mut self, e: &Expr) {
        if let Some(c) = self.col(e) {
            self.out.filtered.insert(c);
        }
    }

    /// `col IN ('a', 'b')` / `col BETWEEN 'a' AND 'b'`: each string literal is a comparison.
    fn literals<'e>(&mut self, e: &Expr, items: impl IntoIterator<Item = &'e Expr>, op: CmpOp) {
        let Some(c) = self.col(e) else {
            return;
        };
        for lit in items.into_iter().filter_map(string_literal) {
            self.out.comparisons.push(Comparison {
                col: c.clone(),
                op,
                literal: lit,
            });
        }
        self.out.filtered.insert(c);
    }

    fn function(&mut self, f: &Function) {
        let name = f.name.to_string().to_lowercase();
        let args = function_args(&f.args);
        if matches!(name.as_str(), "avg" | "min" | "max" | "sum" | "total")
            && args.len() == 1
            && let Some(c) = self.col(args[0])
        {
            self.out.aggregates.push((name, c));
        }
        if let FunctionArguments::Subquery(q) = &f.args {
            self.query(q);
        }
        self.exprs(args);
        if let Some(filter) = &f.filter {
            self.expr(filter);
        }
    }

    fn binary(&mut self, left: &Expr, op: &BinaryOperator, right: &Expr) {
        use BinaryOperator as B;
        match op {
            B::Eq | B::NotEq | B::Lt | B::LtEq | B::Gt | B::GtEq => {
                let (l, r) = (self.col(left), self.col(right));
                for (c, other) in [(&l, right), (&r, left)] {
                    if let Some(c) = c {
                        self.out.filtered.insert(c.clone());
                        if let Some(lit) = string_literal(other) {
                            let op = match op {
                                B::Eq => CmpOp::Eq,
                                B::NotEq => continue,
                                _ => CmpOp::Range,
                            };
                            self.out.comparisons.push(Comparison {
                                col: c.clone(),
                                op,
                                literal: lit,
                            });
                        }
                    }
                }
                if *op == B::Eq
                    && let (Some(a), Some(b)) = (l, r)
                    && !a.0.eq_ignore_ascii_case(&b.0)
                {
                    let pair = if a <= b { (a, b) } else { (b, a) };
                    if !self.out.joins.contains(&pair) {
                        self.out.joins.push(pair);
                    }
                }
            }
            B::Divide => {
                let (a, b) = (self.kind(left), self.kind(right));
                self.out.divisions.push((a, b, format!("{left} / {right}")));
            }
            _ => {}
        }
    }

    /// Whether an expression is integer- or real-valued, as far as the schema and the
    /// expression's own form tell.
    fn kind(&self, e: &Expr) -> NumKind {
        use NumKind::*;
        match e {
            Expr::Nested(inner) => self.kind(inner),
            // A scalar subquery has its one column's kind (`(SELECT COUNT(*) FROM t)`).
            Expr::Subquery(q) => match q.body.as_ref() {
                SetExpr::Select(s) => match s.projection.as_slice() {
                    [SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. }] => {
                        self.kind(e)
                    }
                    _ => NumKind::Unknown,
                },
                _ => NumKind::Unknown,
            },
            Expr::Value(Value::Number(n, _)) => {
                if n.contains(['.', 'e', 'E']) {
                    Real
                } else {
                    Int
                }
            }
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => match self.col(e) {
                Some((t, c)) => match self.schema.column(&t, &c) {
                    Some(col) if col.is_integer() => Int,
                    Some(col) if col.is_real() => Real,
                    _ => Unknown,
                },
                None => Unknown,
            },
            Expr::UnaryOp {
                op: UnaryOperator::Minus | UnaryOperator::Plus,
                expr,
            } => self.kind(expr),
            Expr::Cast {
                data_type,
                kind: CastKind::Cast | CastKind::DoubleColon | CastKind::TryCast,
                ..
            } => cast_kind(data_type),
            Expr::BinaryOp { left, op, right } => {
                let (a, b) = (self.kind(left), self.kind(right));
                match op {
                    BinaryOperator::Plus
                    | BinaryOperator::Minus
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo => match (a, b) {
                        (Int, Int) => Int,
                        (Real, _) | (_, Real) => Real,
                        _ => Unknown,
                    },
                    _ => Unknown,
                }
            }
            Expr::Case {
                results,
                else_result,
                ..
            } => {
                let mut kinds: Vec<NumKind> = results.iter().map(|r| self.kind(r)).collect();
                if let Some(e) = else_result {
                    kinds.push(self.kind(e));
                }
                if kinds.contains(&Real) {
                    Real
                } else if !kinds.is_empty() && kinds.iter().all(|k| *k == Int) {
                    Int
                } else {
                    Unknown
                }
            }
            Expr::Function(f) => {
                let name = f.name.to_string().to_lowercase();
                let args = function_args(&f.args);
                match name.as_str() {
                    "count" | "length" | "instr" => Int,
                    "avg" | "total" | "round" | "julianday" => Real,
                    "sum" | "max" | "min" | "abs" if args.len() == 1 => self.kind(args[0]),
                    _ => Unknown,
                }
            }
            _ => Unknown,
        }
    }
}

fn cast_kind(t: &DataType) -> NumKind {
    let s = t.to_string().to_uppercase();
    if s.contains("INT") {
        NumKind::Int
    } else if ["REAL", "FLOA", "DOUB", "NUMERIC", "DECIMAL"]
        .iter()
        .any(|k| s.contains(k))
    {
        NumKind::Real
    } else {
        NumKind::Unknown
    }
}

fn function_args(args: &FunctionArguments) -> Vec<&Expr> {
    match args {
        FunctionArguments::List(list) => list
            .args
            .iter()
            .filter_map(|a| match a {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(e))
                | FunctionArg::Named {
                    arg: FunctionArgExpr::Expr(e),
                    ..
                }
                | FunctionArg::ExprNamed {
                    arg: FunctionArgExpr::Expr(e),
                    ..
                } => Some(e),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether `e` holds an aggregate (outside subqueries), and whether one is `COUNT(DISTINCT …)`.
fn aggregate_in(e: &Expr) -> (bool, bool) {
    match e {
        Expr::Function(f) => {
            let name = f.name.to_string().to_lowercase();
            let windowed = f.over.is_some();
            let is_agg = !windowed && matches!(name.as_str(), "count" | "sum" | "avg" | "total");
            let distinct = name == "count"
                && matches!(&f.args, FunctionArguments::List(l)
                    if matches!(l.duplicate_treatment, Some(sqlparser::ast::DuplicateTreatment::Distinct)));
            let (mut a, mut d) = (is_agg, distinct);
            for arg in function_args(&f.args) {
                let (x, y) = aggregate_in(arg);
                a |= x;
                d |= y;
            }
            (a, d)
        }
        Expr::BinaryOp { left, right, .. } => {
            let (a, b) = (aggregate_in(left), aggregate_in(right));
            (a.0 || b.0, a.1 || b.1)
        }
        Expr::Nested(i) | Expr::Cast { expr: i, .. } | Expr::UnaryOp { expr: i, .. } => {
            aggregate_in(i)
        }
        Expr::Case {
            conditions,
            results,
            else_result,
            ..
        } => {
            let mut out = (false, false);
            for x in conditions
                .iter()
                .chain(results)
                .chain(else_result.as_deref())
            {
                let r = aggregate_in(x);
                out = (out.0 || r.0, out.1 || r.1);
            }
            out
        }
        _ => (false, false),
    }
}

fn string_literal(e: &Expr) -> Option<String> {
    match e {
        Expr::Nested(inner) => string_literal(inner),
        Expr::Value(Value::SingleQuotedString(s)) => Some(s.clone()),
        _ => None,
    }
}

fn literal_u64(e: &Expr) -> Option<u64> {
    match e {
        Expr::Value(Value::Number(n, _)) => n.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::fixture;
    use sqlparser::dialect::SQLiteDialect;

    fn run(sql: &str) -> Analysis {
        analyze(sql, &SQLiteDialect {}, &fixture()).unwrap()
    }

    #[test]
    fn aliases_and_qualifiers_resolve_to_real_columns() {
        let a = run(
            "SELECT c.name FROM cards AS c JOIN rulings r ON r.card_id = c.id \
                     WHERE c.rarity = 'Rare' AND r.text IS NOT NULL",
        );
        assert!(a.unresolved.is_empty(), "{:?}", a.unresolved);
        assert_eq!(
            a.joins,
            vec![(
                ("cards".to_string(), "id".to_string()),
                ("rulings".to_string(), "card_id".to_string())
            )]
        );
        assert_eq!(a.comparisons[0].literal, "Rare");
        assert!(a.filtered.contains(&("rulings".into(), "text".into())));
        assert!(a.top.joined);
    }

    #[test]
    fn unknown_names_are_reported_but_cte_and_alias_names_are_not() {
        let a = run("WITH x AS (SELECT name AS n FROM cards) SELECT n, zzz FROM x");
        assert!(
            a.unresolved.is_empty(),
            "CTE columns are opaque: {:?}",
            a.unresolved
        );
        let a = run("SELECT nam, c.colour, \"Fireball\" FROM cards c");
        let names: Vec<&str> = a.unresolved.iter().map(|u| u.column.as_str()).collect();
        assert_eq!(names, vec!["nam", "colour", "Fireball"]);
        let a = run("SELECT nam FROM cards c JOIN nowhere n ON 1 = 1");
        assert_eq!(a.unknown_tables, vec!["nowhere"]);
        assert!(
            a.unresolved.is_empty(),
            "columns may come from the unknown table"
        );
        let a = run("SELECT name FROM cards WHERE name = \"Fireball\"");
        assert!(a.unresolved[0].double_quoted);
        let a = run("SELECT name AS n FROM cards ORDER BY n LIMIT 1");
        assert!(a.unresolved.is_empty());
    }

    #[test]
    fn correlated_subquery_sees_the_outer_scope() {
        let a = run("SELECT name FROM cards c WHERE EXISTS \
                     (SELECT 1 FROM rulings WHERE rulings.card_id = c.id)");
        assert!(a.unresolved.is_empty(), "{:?}", a.unresolved);
    }

    #[test]
    fn division_kinds_follow_types_casts_and_aggregates() {
        let a = run(
            "SELECT SUM(CASE WHEN power > 1 THEN 1 ELSE 0 END) / COUNT(id), \
                     CAST(COUNT(id) AS REAL) / COUNT(*), power / 2.0 FROM cards",
        );
        let kinds: Vec<(NumKind, NumKind)> = a.divisions.iter().map(|d| (d.0, d.1)).collect();
        assert_eq!(
            kinds,
            vec![
                (NumKind::Int, NumKind::Int),
                (NumKind::Real, NumKind::Int),
                (NumKind::Int, NumKind::Real)
            ]
        );
    }

    #[test]
    fn top_level_shape_and_order_limit() {
        let a = run("SELECT COUNT(DISTINCT c.id) FROM cards c JOIN rulings r ON r.card_id = c.id");
        assert!(a.top.aggregate && a.top.count_distinct && a.top.joined);
        assert_eq!(a.top.first, Some(("c".into(), "cards".into())));
        let a = run("SELECT name FROM cards ORDER BY power LIMIT 1");
        assert_eq!(a.order_limits[0].col, ("cards".into(), "power".into()));
        assert!(a.order_limits[0].ascending);
        assert_eq!(a.top.limit, Some(1));
    }
}

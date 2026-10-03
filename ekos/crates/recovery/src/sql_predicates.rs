//! RFC 0170 — the business-meaning traces a SQL statement leaves: every comparison of a column
//! against a **literal** in a `WHERE`, `HAVING`, `JOIN … ON`, `CASE` or `CHECK`.
//!
//! `WHERE status IN (1, 3)` is where "active" is defined when nobody wrote it down; `CASE WHEN
//! status = 3 THEN 'suspended'` is where a code's meaning is. This module reads both from the
//! `sqlparser` AST — never from regexes over text — and **normalizes** them, so `status IN (1,3)`,
//! `status = 1 OR status = 3` and `3 = status OR status = 1` are one predicate.
//!
//! Only column-vs-literal comparisons are kept. A comparison against a column, a PL/pgSQL variable
//! or a parameter (`id = in_id`) is plumbing, not a business rule, and contributes nothing.
//!
//! Columns are resolved to the relation they belong to through the enclosing `SELECT`'s `FROM`
//! aliases (or the `UPDATE`/`DELETE` target). An unqualified column resolves only when exactly one
//! relation is in scope; anything else keeps `relation: None` rather than guessing.

use sqlparser::ast::{
    BinaryOperator, Expr, FromTable, Ident, JoinConstraint, JoinOperator, ObjectName, Query,
    Select, SetExpr, Statement, TableFactor, TableWithJoins, UnaryOperator, Value, Visit, Visitor,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

/// Bumped whenever this module's output changes for the same SQL. Every pass that records
/// predicates folds it into its cache key, so an extractor change can never be served stale.
/// `2` = RFC 0170: filters through CTEs and derived tables restated on base tables.
pub const PREDICATES_VERSION: &str = "predicates/2";

pub use ekos_kir::predicates::{Clause, NEW_ROW, OLD_ROW, PredicateSite, canonical_text};

/// Every predicate in a parsed statement (nested queries included).
pub fn statement_predicates(stmt: &Statement) -> Vec<PredicateSite> {
    let mut c = Collector::default();
    let _ = stmt.visit(&mut c);
    c.finish()
}

/// Every predicate in a parsed query — a view's body.
pub fn query_predicates(query: &Query) -> Vec<PredicateSite> {
    let mut c = Collector::default();
    let _ = query.visit(&mut c);
    c.finish()
}

/// The predicates of a `CHECK (expr)` on `table`: every conjunct, its columns resolved to `table`.
pub fn check_predicates(table: &str, expr: &Expr) -> Vec<PredicateSite> {
    let scope = Scope::single(table);
    let mut out = Vec::new();
    conjuncts(expr, &scope, Clause::Check, &mut out);
    out
}

/// The `predicates` property an object carries: `sites` with lines made absolute in the file, given
/// the line the parsed text starts on. A site with no recorded line takes `base`.
pub fn predicates_json(sites: &[PredicateSite], base: u32) -> serde_json::Value {
    let rebased: Vec<PredicateSite> = sites
        .iter()
        .map(|s| PredicateSite {
            line: if s.line == 0 {
                u64::from(base)
            } else {
                u64::from(base) + s.line - 1
            },
            ..s.clone()
        })
        .collect();
    serde_json::to_value(rebased).unwrap_or_default()
}

/// The predicates of a PL/pgSQL condition (`IF NEW.status = 3 THEN`): only `NEW.`/`OLD.`-qualified
/// columns resolve — to [`NEW_ROW`]/[`OLD_ROW`] — because an unqualified name in a condition is far
/// more often a variable than a column. Unparseable conditions yield nothing.
pub fn condition_predicates(cond: &str) -> Vec<PredicateSite> {
    use sqlparser::dialect::PostgreSqlDialect;
    let Ok(stmts) = sqlparser::parser::Parser::parse_sql(
        &PostgreSqlDialect {},
        &format!("SELECT 1 WHERE {cond}"),
    ) else {
        return Vec::new();
    };
    let Some(Statement::Query(q)) = stmts.first() else {
        return Vec::new();
    };
    let SetExpr::Select(sel) = &*q.body else {
        return Vec::new();
    };
    let Some(expr) = &sel.selection else {
        return Vec::new();
    };
    let mut scope = Scope::default();
    scope.aliases.insert("new".into(), NEW_ROW.into());
    scope.aliases.insert("old".into(), OLD_ROW.into());
    let mut out = Vec::new();
    conjuncts(expr, &scope, Clause::Condition, &mut out);
    out.retain(|p| p.relation.is_some());
    out
}

// ── Literals ────────────────────────────────────────────────────────────────────────────────

/// A literal's normalized text, or `None` for anything that is not a constant. Public for the
/// seed-row reader (`sql_analyzer`), so a seeded code and a compared code normalize alike.
pub fn literal(e: &Expr) -> Option<String> {
    match e {
        Expr::Value(v) => match v {
            Value::Number(n, _) => Some(n.clone()),
            Value::SingleQuotedString(s)
            | Value::EscapedStringLiteral(s)
            | Value::UnicodeStringLiteral(s)
            | Value::NationalStringLiteral(s) => Some(format!("'{}'", s.replace('\'', "''"))),
            Value::DollarQuotedString(d) => Some(format!("'{}'", d.value.replace('\'', "''"))),
            Value::Boolean(b) => Some(b.to_string()),
            Value::Null => Some("null".into()),
            _ => None,
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => match literal(expr) {
            Some(n) if n.starts_with(|c: char| c.is_ascii_digit()) => Some(format!("-{n}")),
            _ => None,
        },
        // `'x'::text`, `CAST(1 AS int)` — the literal is what matters.
        Expr::Cast { expr, .. } => literal(expr),
        Expr::Nested(inner) => literal(inner),
        _ => None,
    }
}

// ── Scope: which relation a column belongs to ───────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct Scope {
    /// alias (or bare relation name) → relation name.
    aliases: BTreeMap<String, String>,
    /// Distinct relations in scope.
    relations: BTreeSet<String>,
}

impl Scope {
    fn single(table: &str) -> Self {
        let mut s = Scope::default();
        s.add(table, None);
        s
    }

    fn add(&mut self, relation: &str, alias: Option<&Ident>) {
        let relation = relation.to_string();
        let bare = relation.rsplit('.').next().unwrap_or(&relation).to_string();
        self.aliases.insert(bare, relation.clone());
        self.aliases.insert(relation.clone(), relation.clone());
        if let Some(a) = alias {
            self.aliases.insert(fold(a), relation.clone());
        }
        self.relations.insert(relation);
    }

    fn add_from(&mut self, from: &[TableWithJoins]) {
        for t in from {
            self.add_factor(&t.relation);
            for j in &t.joins {
                self.add_factor(&j.relation);
            }
        }
    }

    fn add_factor(&mut self, f: &TableFactor) {
        match f {
            TableFactor::Table {
                name: n,
                alias,
                args: None,
                ..
            } => self.add(&object_name(n), alias.as_ref().map(|a| &a.name)),
            // A table function, a derived table or a join group names no base relation, but its
            // alias still shadows: a column qualified by it must not resolve elsewhere.
            // A derived table (`FROM (SELECT …) x`): its columns are mapped back to their base
            // relation through `Collector::derived`, under a marker no real table can be named.
            TableFactor::Derived { alias: Some(a), .. } => {
                let marker = derived_marker(&fold(&a.name));
                self.aliases.insert(fold(&a.name), marker.clone());
                self.relations.insert(marker);
            }
            TableFactor::Table { alias, .. }
            | TableFactor::Derived { alias, .. }
            | TableFactor::Function { alias, .. }
            | TableFactor::UNNEST { alias, .. } => {
                if let Some(a) = alias {
                    self.aliases.insert(fold(&a.name), String::new());
                }
                self.relations.insert(String::new());
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => self.add_from(std::slice::from_ref(table_with_joins)),
            _ => {
                self.relations.insert(String::new());
            }
        }
    }

    /// The relation a column reference belongs to, and the column.
    fn resolve(&self, e: &Expr) -> Option<Col> {
        match e {
            Expr::Identifier(i) => {
                let (relation, scope) = if self.relations.len() == 1 {
                    let only = self
                        .relations
                        .iter()
                        .next()
                        .filter(|r| !r.is_empty())
                        .cloned();
                    (only, Vec::new())
                } else {
                    (
                        None,
                        self.relations
                            .iter()
                            .filter(|r| !r.is_empty())
                            .cloned()
                            .collect(),
                    )
                };
                Some(Col {
                    relation,
                    scope,
                    column: fold(i),
                    line: i.span.start.line,
                })
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let col = &parts[parts.len() - 1];
                let qualifier = parts[..parts.len() - 1]
                    .iter()
                    .map(fold)
                    .collect::<Vec<_>>()
                    .join(".");
                let relation = self
                    .aliases
                    .get(&qualifier)
                    .filter(|r| !r.is_empty())
                    .cloned();
                Some(Col {
                    relation,
                    scope: Vec::new(),
                    column: fold(col),
                    line: col.span.start.line,
                })
            }
            Expr::Nested(inner) => self.resolve(inner),
            // `status::int = 3`: the column is still `status`.
            Expr::Cast { expr, .. } => self.resolve(expr),
            _ => None,
        }
    }
}

/// A resolved column reference.
struct Col {
    relation: Option<String>,
    scope: Vec<String>,
    column: String,
    line: u64,
}

fn fold(i: &Ident) -> String {
    if i.quote_style.is_some() {
        i.value.clone()
    } else {
        i.value.to_ascii_lowercase()
    }
}

fn object_name(n: &ObjectName) -> String {
    n.0.iter().map(fold).collect::<Vec<_>>().join(".")
}

// ── Atoms ───────────────────────────────────────────────────────────────────────────────────

fn site(col: Col, op: &str, values: Vec<String>, clause: Clause) -> PredicateSite {
    let Col {
        relation,
        scope,
        column,
        line,
    } = col;
    PredicateSite {
        relation,
        scope,
        column,
        op: op.to_string(),
        values,
        clause,
        top_level: false,
        subquery: false,
        label: None,
        line,
    }
}

fn flip(op: &BinaryOperator) -> Option<&'static str> {
    Some(match op {
        BinaryOperator::Lt => ">",
        BinaryOperator::LtEq => ">=",
        BinaryOperator::Gt => "<",
        BinaryOperator::GtEq => "<=",
        BinaryOperator::Eq => "=",
        BinaryOperator::NotEq => "<>",
        _ => return None,
    })
}

fn op_text(op: &BinaryOperator) -> Option<&'static str> {
    Some(match op {
        BinaryOperator::Lt => "<",
        BinaryOperator::LtEq => "<=",
        BinaryOperator::Gt => ">",
        BinaryOperator::GtEq => ">=",
        BinaryOperator::Eq => "=",
        BinaryOperator::NotEq => "<>",
        _ => return None,
    })
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// One expression as a single normalized atom, if it is one.
fn atom(e: &Expr, scope: &Scope, clause: Clause) -> Option<PredicateSite> {
    match e {
        Expr::Nested(inner) => atom(inner, scope, clause),
        // A bare boolean column in a boolean position: `WHERE a.approved`.
        Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
            Some(site(scope.resolve(e)?, "is_true", vec![], clause))
        }
        Expr::BinaryOp { left, op, right } => {
            let (col, lit, op) = match (scope.resolve(left), literal(right)) {
                (Some(c), Some(l)) => (c, l, op_text(op)?),
                _ => (scope.resolve(right)?, literal(left)?, flip(op)?),
            };
            Some(match op {
                "=" => site(col, "in", vec![lit], clause),
                "<>" => site(col, "not_in", vec![lit], clause),
                cmp => site(col, cmp, vec![lit], clause),
            })
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let col = scope.resolve(expr)?;
            let values: Option<Vec<String>> = list.iter().map(literal).collect();
            let op = if *negated { "not_in" } else { "in" };
            Some(site(col, op, sorted(values?), clause))
        }
        Expr::Between {
            expr,
            negated,
            low,
            high,
        } => {
            let col = scope.resolve(expr)?;
            let op = if *negated { "not_between" } else { "between" };
            Some(site(col, op, vec![literal(low)?, literal(high)?], clause))
        }
        Expr::Like {
            negated,
            expr,
            pattern,
            ..
        }
        | Expr::ILike {
            negated,
            expr,
            pattern,
            ..
        } => {
            let col = scope.resolve(expr)?;
            let op = if *negated { "not_like" } else { "like" };
            Some(site(col, op, vec![literal(pattern)?], clause))
        }
        Expr::IsNull(x) => Some(site(scope.resolve(x)?, "is_null", vec![], clause)),
        Expr::IsNotNull(x) => Some(site(scope.resolve(x)?, "is_not_null", vec![], clause)),
        Expr::IsTrue(x) => Some(site(scope.resolve(x)?, "is_true", vec![], clause)),
        Expr::IsFalse(x) => Some(site(scope.resolve(x)?, "is_false", vec![], clause)),
        // Not `is_false`/`is_true`: `NULL IS NOT TRUE` holds, `NULL IS FALSE` does not.
        Expr::IsNotTrue(x) => Some(site(scope.resolve(x)?, "is_not_true", vec![], clause)),
        Expr::IsNotFalse(x) => Some(site(scope.resolve(x)?, "is_not_false", vec![], clause)),
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr,
        } => {
            // `NOT approved` on a bare boolean column.
            if let Some(col) = scope.resolve(expr) {
                return Some(site(col, "is_false", vec![], clause));
            }
            let a = atom(expr, scope, clause)?;
            let negated = match a.op.as_str() {
                "in" => "not_in",
                "not_in" => "in",
                "is_null" => "is_not_null",
                "is_not_null" => "is_null",
                "is_true" => "is_not_true",
                "is_false" => "is_not_false",
                "is_not_true" => "is_true",
                "is_not_false" => "is_false",
                "like" => "not_like",
                "not_like" => "like",
                "between" => "not_between",
                "not_between" => "between",
                "<" => ">=",
                "<=" => ">",
                ">" => "<=",
                ">=" => "<",
                _ => return None,
            };
            Some(PredicateSite {
                op: negated.into(),
                ..a
            })
        }
        _ => None,
    }
}

/// `a = 1 OR a = 3 OR a IN (5)` on one column → `a IN (1, 3, 5)`.
fn or_of_equalities(e: &Expr, scope: &Scope, clause: Clause) -> Option<PredicateSite> {
    let mut parts = Vec::new();
    flatten(e, &BinaryOperator::Or, &mut parts);
    if parts.len() < 2 {
        return None;
    }
    let mut merged: Option<PredicateSite> = None;
    for p in parts {
        let a = atom(p, scope, clause)?;
        if a.op != "in" {
            return None;
        }
        merged = Some(match merged {
            None => a,
            Some(m) if m.relation == a.relation && m.column == a.column => {
                let mut values = m.values.clone();
                values.extend(a.values);
                PredicateSite {
                    values: sorted(values),
                    line: m.line.min(a.line),
                    ..m
                }
            }
            Some(_) => return None,
        });
    }
    merged
}

fn flatten<'a>(e: &'a Expr, with: &BinaryOperator, out: &mut Vec<&'a Expr>) {
    match e {
        Expr::BinaryOp { left, op, right } if op == with => {
            flatten(left, with, out);
            flatten(right, with, out);
        }
        Expr::Nested(inner) if matches!(&**inner, Expr::BinaryOp { op, .. } if op == with) => {
            flatten(inner, with, out)
        }
        _ => out.push(e),
    }
}

/// Every atom in a boolean expression. Top-level `AND` conjuncts are marked `top_level`; atoms
/// reached through `OR`/`NOT` are kept (they still say what values a column takes) but are not.
fn conjuncts(e: &Expr, scope: &Scope, clause: Clause, out: &mut Vec<PredicateSite>) {
    let mut parts = Vec::new();
    flatten(e, &BinaryOperator::And, &mut parts);
    for p in parts {
        if let Some(a) = atom(p, scope, clause).or_else(|| or_of_equalities(p, scope, clause)) {
            out.push(PredicateSite {
                top_level: true,
                ..a
            });
        } else {
            nested_atoms(p, scope, clause, out);
        }
    }
}

/// Atoms inside `OR`/`AND`/`NOT` trees below the top level. Subqueries are left to the visitor,
/// which reaches them with their own scope.
fn nested_atoms(e: &Expr, scope: &Scope, clause: Clause, out: &mut Vec<PredicateSite>) {
    if let Some(a) = atom(e, scope, clause).or_else(|| or_of_equalities(e, scope, clause)) {
        out.push(a);
        return;
    }
    match e {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And | BinaryOperator::Or,
            right,
        } => {
            nested_atoms(left, scope, clause, out);
            nested_atoms(right, scope, clause, out);
        }
        Expr::Nested(inner)
        | Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr: inner,
        } => nested_atoms(inner, scope, clause, out),
        _ => {}
    }
}

/// The `CASE` branches that test a column against literals, each labelled with its literal result.
fn case_sites(e: &Expr, scope: &Scope, out: &mut Vec<PredicateSite>) {
    let Expr::Case {
        operand,
        conditions,
        results,
        ..
    } = e
    else {
        return;
    };
    for (cond, result) in conditions.iter().zip(results) {
        let label = literal(result).filter(|l| l.starts_with('\''));
        let found = match operand {
            // `CASE status WHEN 1 THEN …`
            Some(op) => scope.resolve(op).and_then(|col| {
                literal(cond).map(|v| vec![site(col, "in", vec![v], Clause::Case)])
            }),
            None => {
                let mut v = Vec::new();
                conjuncts(cond, scope, Clause::Case, &mut v);
                Some(v)
            }
        };
        for mut s in found.unwrap_or_default() {
            s.top_level = false;
            s.label = label.clone();
            out.push(s);
        }
    }
}

// ── The walk ────────────────────────────────────────────────────────────────────────────────

/// The relation name a derived table's alias stands for, unmistakable for a real table.
fn derived_marker(alias: &str) -> String {
    format!("\u{1}{alias}")
}

/// What a CTE's or derived table's output columns are, in terms of base relations: an output
/// name → `(relation, column)` for a plain (possibly aliased) column reference, plus the one
/// relation a `SELECT *` passes through. Computed columns are absent: a filter on one is about the
/// computation, not a stored column.
#[derive(Debug, Clone, Default)]
struct ColMap {
    cols: BTreeMap<String, (String, String)>,
    star: Option<String>,
}

/// `(relation, column)` through any number of CTE/derived layers, or `None` when it ends in a
/// computed column. A relation that is not mapped is a base relation and is returned as is.
fn chase(
    derived: &BTreeMap<String, ColMap>,
    relation: &str,
    column: &str,
) -> Option<(String, String)> {
    let (mut r, mut c) = (relation.to_string(), column.to_string());
    for _ in 0..16 {
        let Some(m) = derived.get(&r) else {
            return Some((r, c));
        };
        match (m.cols.get(&c), &m.star) {
            (Some((r2, c2)), _) => (r, c) = (r2.clone(), c2.clone()),
            (None, Some(star)) => r = star.clone(),
            (None, None) => return None,
        }
    }
    None
}

/// The [`ColMap`] of a query whose body is one `SELECT`.
fn projection_map(q: &Query, derived: &BTreeMap<String, ColMap>) -> Option<ColMap> {
    use sqlparser::ast::SelectItem;
    let SetExpr::Select(sel) = &*q.body else {
        return None;
    };
    let mut scope = Scope::default();
    scope.add_from(&sel.from);
    let mut m = ColMap::default();
    for item in &sel.projection {
        let (expr, name) = match item {
            SelectItem::UnnamedExpr(e) => (e, None),
            SelectItem::ExprWithAlias { expr, alias } => (expr, Some(fold(alias))),
            SelectItem::Wildcard(_) => {
                let real: Vec<&String> = scope.relations.iter().filter(|r| !r.is_empty()).collect();
                if real.len() == 1 && scope.relations.len() == 1 {
                    m.star = Some(real[0].clone());
                }
                continue;
            }
            SelectItem::QualifiedWildcard(n, _) => {
                if let Some(r) = scope.aliases.get(&object_name(n)).filter(|r| !r.is_empty()) {
                    m.star.get_or_insert_with(|| r.clone());
                }
                continue;
            }
        };
        if let Some(col) = scope.resolve(expr)
            && let Some(rel) = col.relation
            && let Some(base) = chase(derived, &rel, &col.column)
        {
            m.cols.insert(name.unwrap_or(col.column), base);
        }
    }
    Some(m)
}

/// RFC 0170: a query's output columns in terms of base relations — `is_closed` ← `(oe, closed)` —
/// through its CTEs and derived tables. Computed outputs are absent. For a dbt model this is its
/// column lineage: a filter on a model column can be restated on the source column it carries.
pub fn output_lineage(query: &Query) -> BTreeMap<String, (String, String)> {
    let mut c = Collector::default();
    c.learn_derived(query);
    projection_map(query, &c.derived)
        .map(|m| m.cols)
        .unwrap_or_default()
}

#[derive(Default)]
struct Collector {
    /// CTE names and derived-table markers → their column maps.
    derived: BTreeMap<String, ColMap>,
    scopes: Vec<Scope>,
    /// Queries (and `UPDATE`/`DELETE` statements) currently open: past the first, a filter is a
    /// subquery's.
    depth: usize,
    out: Vec<PredicateSite>,
}

impl Collector {
    fn finish(self) -> Vec<PredicateSite> {
        self.out
    }

    /// Append `found`, marking each as a subquery's when this is not the outermost level.
    fn take(&mut self, found: Vec<PredicateSite>) {
        let subquery = self.depth > 0;
        let found: Vec<PredicateSite> = found
            .into_iter()
            .map(|s| self.through_derived(PredicateSite { subquery, ..s }))
            .collect();
        self.out.extend(found);
    }

    /// A site on a CTE or derived table, restated on the base relation its column comes from —
    /// or left without a relation when the column is computed.
    fn through_derived(&self, mut s: PredicateSite) -> PredicateSite {
        if let Some(r) = s.relation.clone()
            && self.derived.contains_key(&r)
        {
            match chase(&self.derived, &r, &s.column) {
                Some((r2, c2)) => {
                    s.relation = Some(r2);
                    s.column = c2;
                }
                None => s.relation = None,
            }
        }
        if !s.scope.is_empty() {
            let col = s.column.clone();
            s.scope = s
                .scope
                .iter()
                .filter_map(|r| {
                    if self.derived.contains_key(r) {
                        chase(&self.derived, r, &col)
                            .filter(|(_, c2)| *c2 == col)
                            .map(|(r2, _)| r2)
                    } else {
                        Some(r.clone())
                    }
                })
                .collect();
        }
        s
    }

    /// Record the column maps of a query's CTEs and of the derived tables in its `FROM`s.
    fn learn_derived(&mut self, q: &Query) {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                if let Some(m) = projection_map(&cte.query, &self.derived) {
                    self.derived.insert(fold(&cte.alias.name), m);
                }
            }
        }
        let mut found = Vec::new();
        selects(&q.body, &mut found);
        for sel in found {
            for t in &sel.from {
                let factors =
                    std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation));
                for f in factors {
                    if let TableFactor::Derived {
                        subquery,
                        alias: Some(a),
                        ..
                    } = f
                        && let Some(m) = projection_map(subquery, &self.derived)
                    {
                        self.derived.insert(derived_marker(&fold(&a.name)), m);
                    }
                }
            }
        }
    }

    fn select(&mut self, s: &Select, scope: &Scope) {
        let mut found = Vec::new();
        if let Some(w) = &s.selection {
            conjuncts(w, scope, Clause::Where, &mut found);
        }
        if let Some(h) = &s.having {
            conjuncts(h, scope, Clause::Having, &mut found);
        }
        for t in &s.from {
            for j in &t.joins {
                if let Some(JoinConstraint::On(on)) = join_constraint(&j.join_operator) {
                    conjuncts(on, scope, Clause::JoinOn, &mut found);
                }
            }
        }
        self.take(found);
    }
}

fn is_filtered_dml(s: &Statement) -> bool {
    matches!(s, Statement::Update { .. } | Statement::Delete(_))
}

fn join_constraint(op: &JoinOperator) -> Option<&JoinConstraint> {
    match op {
        JoinOperator::Inner(c)
        | JoinOperator::LeftOuter(c)
        | JoinOperator::RightOuter(c)
        | JoinOperator::FullOuter(c)
        | JoinOperator::Semi(c)
        | JoinOperator::LeftSemi(c)
        | JoinOperator::RightSemi(c)
        | JoinOperator::Anti(c)
        | JoinOperator::LeftAnti(c)
        | JoinOperator::RightAnti(c) => Some(c),
        _ => None,
    }
}

/// The `SELECT`s directly in a query body (through set operations, not into subqueries).
fn selects<'a>(body: &'a SetExpr, out: &mut Vec<&'a Select>) {
    match body {
        SetExpr::Select(s) => out.push(s),
        SetExpr::SetOperation { left, right, .. } => {
            selects(left, out);
            selects(right, out);
        }
        _ => {}
    }
}

impl Visitor for Collector {
    type Break = ();

    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
        self.learn_derived(q);
        let mut found = Vec::new();
        selects(&q.body, &mut found);
        let mut scope = Scope::default();
        for s in &found {
            let mut own = Scope::default();
            own.add_from(&s.from);
            self.select(s, &own);
            scope.add_from(&s.from);
        }
        self.scopes.push(scope);
        self.depth += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _q: &Query) -> ControlFlow<()> {
        self.scopes.pop();
        self.depth -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, s: &Statement) -> ControlFlow<()> {
        let mut scope = Scope::default();
        let selection = match s {
            Statement::Update {
                table,
                from,
                selection,
                ..
            } => {
                scope.add_from(std::slice::from_ref(table));
                if let Some(f) = from {
                    scope.add_from(std::slice::from_ref(f));
                }
                selection.as_ref()
            }
            Statement::Delete(d) => {
                let (FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from)) = &d.from;
                scope.add_from(from);
                if let Some(using) = &d.using {
                    scope.add_from(using);
                }
                d.selection.as_ref()
            }
            _ => None,
        };
        if let Some(w) = selection {
            let mut found = Vec::new();
            conjuncts(w, &scope, Clause::Where, &mut found);
            self.take(found);
        }
        if is_filtered_dml(s) {
            self.depth += 1;
        }
        self.scopes.push(scope);
        ControlFlow::Continue(())
    }

    fn post_visit_statement(&mut self, s: &Statement) -> ControlFlow<()> {
        self.scopes.pop();
        if is_filtered_dml(s) {
            self.depth -= 1;
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        if matches!(e, Expr::Case { .. }) {
            let scope = self.scopes.last().cloned().unwrap_or_default();
            let mut found = Vec::new();
            case_sites(e, &scope, &mut found);
            // A `CASE` is read in the query that holds it, one level in from its `pre_visit_query`.
            let subquery = self.depth > 1;
            self.out
                .extend(found.into_iter().map(|s| PredicateSite { subquery, ..s }));
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::dialect::PostgreSqlDialect;
    use sqlparser::parser::Parser;

    fn preds(sql: &str) -> Vec<PredicateSite> {
        let stmts = Parser::parse_sql(&PostgreSqlDialect {}, sql).expect("parses");
        stmts.iter().flat_map(statement_predicates).collect()
    }

    fn canon(sql: &str) -> Vec<String> {
        preds(sql).iter().map(PredicateSite::canonical).collect()
    }

    #[test]
    fn spellings_of_the_same_predicate_normalize_to_one() {
        let a = canon("SELECT * FROM customer c WHERE c.status IN (3, 1)");
        let b = canon("SELECT * FROM customer c WHERE (c.status = 1 OR 3 = c.status)");
        let c = canon("SELECT * FROM customer WHERE status = 1 OR status = 3");
        assert_eq!(a, vec!["customer.status IN (1, 3)"]);
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn only_literal_comparisons_count_and_columns_resolve_through_aliases() {
        let p = preds(
            "SELECT a.id FROM ar a JOIN entity_credit_account eca ON eca.id = a.entity_credit_account \
             AND eca.entity_class = 2 WHERE a.approved AND a.amount_bc > 0 AND a.id = in_id \
             AND NOT a.on_hold AND a.reversed_by IS NULL",
        );
        let c: Vec<String> = p.iter().map(PredicateSite::canonical).collect();
        assert_eq!(
            c,
            vec![
                "ar.approved IS TRUE",
                "ar.amount_bc > 0",
                "ar.on_hold IS FALSE",
                "ar.reversed_by IS NULL",
                "entity_credit_account.entity_class IN (2)",
            ],
            "{p:?}"
        );
        assert!(p.iter().all(|s| s.top_level));
        assert_eq!(p[4].clause, Clause::JoinOn);
    }

    #[test]
    fn an_ambiguous_unqualified_column_is_not_guessed() {
        let p = preds("SELECT * FROM a, b WHERE status = 3");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].relation, None);
        assert_eq!(p[0].canonical(), "status IN (3)");
    }

    #[test]
    fn case_branches_carry_their_label() {
        let p = preds(
            "SELECT CASE WHEN o.status = 1 THEN 'active' WHEN o.status = 3 THEN 'suspended' \
             ELSE 'other' END, CASE o.kind WHEN 'S' THEN 'sale' END FROM orders o",
        );
        let labelled: Vec<(String, Option<String>)> =
            p.iter().map(|s| (s.canonical(), s.label.clone())).collect();
        assert_eq!(
            labelled,
            vec![
                ("orders.status IN (1)".into(), Some("'active'".into())),
                ("orders.status IN (3)".into(), Some("'suspended'".into())),
                ("orders.kind IN ('S')".into(), Some("'sale'".into())),
            ]
        );
        assert!(p.iter().all(|s| s.clause == Clause::Case && !s.top_level));
    }

    #[test]
    fn subqueries_resolve_in_their_own_scope_and_dml_filters_count() {
        let p = preds(
            "UPDATE invoice SET paid = true WHERE status <> 'void' AND id IN \
             (SELECT i.id FROM invoice_line i WHERE i.qty >= 1)",
        );
        let c: Vec<String> = p.iter().map(PredicateSite::canonical).collect();
        assert_eq!(
            c,
            vec!["invoice.status NOT IN ('void')", "invoice_line.qty >= 1"]
        );
        assert_eq!(
            p.iter().map(|s| s.subquery).collect::<Vec<_>>(),
            vec![false, true]
        );

        let p = preds(
            "SELECT * FROM ar a WHERE a.approved AND EXISTS \
             (SELECT 1 FROM acc_trans t WHERE t.trans_id = a.id AND t.cleared IS FALSE)",
        );
        assert_eq!(
            p.iter().map(|s| s.subquery).collect::<Vec<_>>(),
            vec![false, true]
        );

        let p = preds("DELETE FROM sessions WHERE last_used < '2020-01-01'::date");
        assert_eq!(p[0].canonical(), "sessions.last_used < '2020-01-01'");
    }

    #[test]
    fn atoms_under_an_or_are_kept_but_not_top_level() {
        let p = preds("SELECT * FROM t WHERE t.a = 1 AND (t.b = 2 OR t.c IS NULL)");
        let top: Vec<_> = p
            .iter()
            .filter(|s| s.top_level)
            .map(|s| s.canonical())
            .collect();
        let nested: Vec<_> = p
            .iter()
            .filter(|s| !s.top_level)
            .map(|s| s.canonical())
            .collect();
        assert_eq!(top, vec!["t.a IN (1)"]);
        assert_eq!(nested, vec!["t.b IN (2)", "t.c IS NULL"]);
    }

    #[test]
    fn check_constraints_resolve_to_their_table() {
        let stmts = Parser::parse_sql(
            &PostgreSqlDialect {},
            "SELECT 1 WHERE amount >= 0 AND kind IN ('A', 'L', 'Q')",
        )
        .unwrap();
        let Statement::Query(q) = &stmts[0] else {
            panic!()
        };
        let SetExpr::Select(s) = &*q.body else {
            panic!()
        };
        let p = check_predicates("account", s.selection.as_ref().unwrap());
        let c: Vec<String> = p.iter().map(PredicateSite::canonical).collect();
        assert_eq!(
            c,
            vec!["account.amount >= 0", "account.kind IN ('A', 'L', 'Q')"]
        );
        assert!(p.iter().all(|s| s.clause == Clause::Check));
    }

    /// Three-valued logic: `IS NOT TRUE` admits NULL, `IS FALSE` does not.
    #[test]
    fn is_not_true_is_not_is_false() {
        let c =
            canon("SELECT * FROM t WHERE t.a IS NOT TRUE AND t.b IS FALSE AND NOT (t.c IS TRUE)");
        assert_eq!(
            c,
            vec!["t.a IS NOT TRUE", "t.b IS FALSE", "t.c IS NOT TRUE"]
        );
    }

    #[test]
    fn conditions_resolve_new_and_old_only() {
        let c: Vec<String> = condition_predicates(
            "NEW.status = 3 AND OLD.approved IS NOT TRUE AND in_flag AND new.amount > 0",
        )
        .iter()
        .map(PredicateSite::canonical)
        .collect();
        assert_eq!(
            c,
            vec![
                "$new.status IN (3)",
                "$old.approved IS NOT TRUE",
                "$new.amount > 0"
            ]
        );
        assert!(condition_predicates("not valid sql ((").is_empty());
    }

    #[test]
    fn filters_through_ctes_and_derived_tables_land_on_the_base_table() {
        let c = canon(
            "WITH orders AS (SELECT o.id, o.closed AS is_closed, o.oe_class_id AS class, \
               o.amount * 2 AS doubled FROM oe o), \
             open_orders AS (SELECT * FROM orders WHERE NOT is_closed) \
             SELECT * FROM open_orders x JOIN (SELECT a.id, a.category AS cat FROM account a) acc \
               ON acc.id = x.id \
             WHERE x.class = 1 AND acc.cat IN ('A', 'L') AND x.doubled > 10",
        );
        assert_eq!(
            c,
            vec![
                "oe.oe_class_id IN (1)",
                "account.category IN ('A', 'L')",
                "doubled > 10",
                "oe.closed IS FALSE",
            ],
            "computed columns keep no relation; the CTE's own filter is on oe too"
        );
    }

    /// Passes whose `version()` is a literal must spell the current predicates version, so bumping
    /// one without the other fails here instead of serving stale caches.
    #[test]
    fn literal_pass_versions_track_the_predicates_version() {
        for src in [
            include_str!("sql_transform_analyzer.rs"),
            include_str!("dbt_analyzer.rs"),
            include_str!("perl_analyzer.rs"),
        ] {
            assert!(
                src.contains(&format!("+{PREDICATES_VERSION}\"")),
                "a pass version does not include {PREDICATES_VERSION}"
            );
        }
    }

    #[test]
    fn output_lineage_follows_aliases_and_ctes() {
        let stmts = Parser::parse_sql(
            &PostgreSqlDialect {},
            "WITH o AS (SELECT x.closed AS is_closed, x.id FROM oe x) \
             SELECT o.is_closed AS done, o.id, o.id + 1 AS next FROM o",
        )
        .unwrap();
        let Statement::Query(q) = &stmts[0] else {
            panic!()
        };
        let l = output_lineage(q);
        assert_eq!(
            l.get("done"),
            Some(&("oe".to_string(), "closed".to_string()))
        );
        assert_eq!(l.get("id"), Some(&("oe".to_string(), "id".to_string())));
        assert!(!l.contains_key("next"));
    }

    #[test]
    fn lines_are_recorded() {
        let p = preds("SELECT *\nFROM t\nWHERE t.a = 1\n  AND t.b = 'x'");
        assert_eq!(p.iter().map(|s| s.line).collect::<Vec<_>>(), vec![3, 4]);
    }
}

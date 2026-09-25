//! RFC 0157/0159 — what the workload actually does.
//!
//! `pg_stat_statements` holds the normalized text of every statement the server has executed, with
//! how many times each ran. That is the difference between a target design derived from evidence
//! and one derived from the schema: it says which columns people *filter on*, which is what
//! ClickHouse's `ORDER BY` should lead with, and it exposes joins that exist only in queries the
//! running application issues — never in the repository.
//!
//! Two things it is not:
//!
//! - **Not a complete picture.** The view is capped (`pg_stat_statements.max`) and is reset by
//!   `pg_stat_statements_reset()` and by some upgrade paths. A column absent from it may simply
//!   have aged out, so its absence is never evidence of anything.
//! - **Not guaranteed free of literals.** Normalization replaces constants with `$1`, but utility
//!   statements and some shapes retain text. Every query string therefore goes through RFC 0043
//!   redaction before it is used or stored.

use crate::PgError;
use crate::catalog::CatalogSource;
use ekos_common::redaction::{RedactionConfig, redact};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    Expr, JoinConstraint, JoinOperator, Query, Select, SetExpr, Statement, TableFactor,
    TableWithJoins,
};
use std::collections::BTreeMap;

/// One normalized statement shape and how much it ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryShape {
    pub text: String,
    pub calls: u64,
}

/// A column observed in a filter predicate, with how many calls it appeared in.
pub type FilterCounts = BTreeMap<(String, String), u64>;

/// A join predicate observed in the live workload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkloadJoin {
    pub left_table: String,
    pub left_column: String,
    pub right_table: String,
    pub right_column: String,
}

/// Read `pg_stat_statements`, redacting every query before it goes anywhere.
///
/// Returns an empty list when the extension is not installed — which is a normal state, not an
/// error, and the caller reports it as "no workload evidence" rather than as a failure.
pub fn harvest(
    src: &dyn CatalogSource,
    redaction: &RedactionConfig,
    limit: u32,
) -> Result<Vec<QueryShape>, PgError> {
    let installed =
        src.rows("SELECT count(*) FROM pg_extension WHERE extname = 'pg_stat_statements'")?;
    if installed
        .first()
        .and_then(|r| r.first())
        .map(String::as_str)
        != Some("1")
    {
        return Ok(Vec::new());
    }

    let rows = src.rows(&format!(
        "SELECT query, calls FROM pg_stat_statements \
         WHERE query IS NOT NULL ORDER BY calls DESC LIMIT {limit}"
    ))?;
    Ok(rows
        .into_iter()
        .filter(|r| r.len() >= 2)
        .map(|r| QueryShape {
            text: redact(&r[0], redaction),
            calls: r[1].trim().parse().unwrap_or(0),
        })
        .collect())
}

/// Resolve `alias -> table` for one `FROM` clause, and record every table in scope.
fn table_scope(from: &[TableWithJoins]) -> (BTreeMap<String, String>, Vec<String>) {
    let mut aliases = BTreeMap::new();
    let mut tables = Vec::new();
    let mut note = |factor: &TableFactor| {
        if let TableFactor::Table { name, alias, .. } = factor {
            let table = name
                .0
                .last()
                .map(|i| i.value.to_ascii_lowercase())
                .unwrap_or_default();
            if table.is_empty() {
                return;
            }
            tables.push(table.clone());
            aliases.insert(table.clone(), table.clone());
            if let Some(a) = alias {
                aliases.insert(a.name.value.to_ascii_lowercase(), table);
            }
        }
    };
    for t in from {
        note(&t.relation);
        for j in &t.joins {
            note(&j.relation);
        }
    }
    tables.sort();
    tables.dedup();
    (aliases, tables)
}

/// Resolve a column reference to `(table, column)`.
///
/// An **unqualified** column is only attributed when exactly one table is in scope. Attributing it
/// to the wrong table would poison the `ORDER BY` derivation with a column that table does not have,
/// and a wrong ordering is worse than a defaulted one because it looks derived.
fn resolve(
    expr: &Expr,
    aliases: &BTreeMap<String, String>,
    tables: &[String],
) -> Option<(String, String)> {
    match expr {
        Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
            let column = parts.last()?.value.to_ascii_lowercase();
            let qualifier = parts[parts.len() - 2].value.to_ascii_lowercase();
            aliases.get(&qualifier).map(|t| (t.clone(), column))
        }
        Expr::Identifier(i) if tables.len() == 1 => {
            Some((tables[0].clone(), i.value.to_ascii_lowercase()))
        }
        _ => None,
    }
}

/// Is this operator a filter worth ordering by?
///
/// Equality and range comparisons skip granules; `<>` and `LIKE '%x'` do not, and ordering by a
/// column only ever used with them buys nothing.
fn is_skippable(op: &sqlparser::ast::BinaryOperator) -> bool {
    use sqlparser::ast::BinaryOperator as B;
    matches!(op, B::Eq | B::Lt | B::LtEq | B::Gt | B::GtEq)
}

fn walk_predicate(
    expr: &Expr,
    aliases: &BTreeMap<String, String>,
    tables: &[String],
    filters: &mut Vec<(String, String)>,
) {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            use sqlparser::ast::BinaryOperator as B;
            if matches!(op, B::And | B::Or) {
                walk_predicate(left, aliases, tables, filters);
                walk_predicate(right, aliases, tables, filters);
                return;
            }
            if !is_skippable(op) {
                return;
            }
            // `a.x = b.y` is a join, not a filter, and is collected separately.
            let l = resolve(left, aliases, tables);
            let r = resolve(right, aliases, tables);
            match (l, r) {
                (Some(c), None) => filters.push(c),
                (None, Some(c)) => filters.push(c),
                _ => {}
            }
        }
        Expr::Between { expr, .. } | Expr::InList { expr, .. } | Expr::InSubquery { expr, .. } => {
            if let Some(c) = resolve(expr, aliases, tables) {
                filters.push(c);
            }
        }
        Expr::Nested(inner) => walk_predicate(inner, aliases, tables, filters),
        _ => {}
    }
}

fn equi_join_pairs(
    expr: &Expr,
    aliases: &BTreeMap<String, String>,
    tables: &[String],
    out: &mut Vec<WorkloadJoin>,
) {
    if let Expr::BinaryOp { left, op, right } = expr {
        use sqlparser::ast::BinaryOperator as B;
        if matches!(op, B::And) {
            equi_join_pairs(left, aliases, tables, out);
            equi_join_pairs(right, aliases, tables, out);
            return;
        }
        if !matches!(op, B::Eq) {
            return;
        }
        // `a.x = a.x` is a tautology, not a join.
        if let (Some(l), Some(r)) = (
            resolve(left, aliases, tables),
            resolve(right, aliases, tables),
        ) && l != r
        {
            out.push(WorkloadJoin {
                left_table: l.0,
                left_column: l.1,
                right_table: r.0,
                right_column: r.1,
            });
        }
    }
}

fn scan_select(
    select: &Select,
    filters: &mut Vec<(String, String)>,
    joins: &mut Vec<WorkloadJoin>,
) {
    let (aliases, tables) = table_scope(&select.from);
    if let Some(w) = &select.selection {
        walk_predicate(w, &aliases, &tables, filters);
        // A `WHERE a.x = b.y` is an implicit join, and older application SQL is full of them.
        equi_join_pairs(w, &aliases, &tables, joins);
    }
    for t in &select.from {
        for j in &t.joins {
            let constraint = match &j.join_operator {
                JoinOperator::Inner(c)
                | JoinOperator::LeftOuter(c)
                | JoinOperator::RightOuter(c)
                | JoinOperator::FullOuter(c) => Some(c),
                _ => None,
            };
            if let Some(JoinConstraint::On(e)) = constraint {
                equi_join_pairs(e, &aliases, &tables, joins);
            }
        }
    }
}

fn scan_query(q: &Query, filters: &mut Vec<(String, String)>, joins: &mut Vec<WorkloadJoin>) {
    match q.body.as_ref() {
        SetExpr::Select(s) => scan_select(s, filters, joins),
        SetExpr::Query(inner) => scan_query(inner, filters, joins),
        SetExpr::SetOperation { left, right, .. } => {
            if let SetExpr::Select(s) = left.as_ref() {
                scan_select(s, filters, joins);
            }
            if let SetExpr::Select(s) = right.as_ref() {
                scan_select(s, filters, joins);
            }
        }
        _ => {}
    }
}

/// Extract filter and join predicates from a set of query shapes.
///
/// A statement that does not parse is skipped, not counted as evidence of anything — the view holds
/// utility statements, extension-specific syntax and truncated text, and none of that is a signal.
pub fn analyze(shapes: &[QueryShape]) -> (FilterCounts, Vec<WorkloadJoin>) {
    let dialect = sqlparser::dialect::PostgreSqlDialect {};
    let mut filters: FilterCounts = BTreeMap::new();
    let mut joins = Vec::new();

    for shape in shapes {
        let Ok(statements) = sqlparser::parser::Parser::parse_sql(&dialect, &shape.text) else {
            continue;
        };
        for st in statements {
            let mut f = Vec::new();
            let mut j = Vec::new();
            match st {
                Statement::Query(q) => scan_query(&q, &mut f, &mut j),
                _ => continue,
            }
            f.sort();
            f.dedup();
            for key in f {
                // Weighted by calls: a predicate in a query run a million times outranks one in a
                // query run twice, which is the whole reason for reading this view.
                *filters.entry(key).or_insert(0) += shape.calls;
            }
            joins.extend(j);
        }
    }
    joins.sort();
    joins.dedup();
    (filters, joins)
}

/// The filter columns for one table, most-used first — the shape `ORDER BY` derivation wants.
pub fn filters_for(counts: &FilterCounts, table: &str) -> Vec<(String, u64)> {
    let bare = table
        .rsplit('.')
        .next()
        .unwrap_or(table)
        .to_ascii_lowercase();
    let mut out: Vec<(String, u64)> = counts
        .iter()
        .filter(|((t, _), _)| *t == bare)
        .map(|((_, c), n)| (c.clone(), *n))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shapes(pairs: &[(&str, u64)]) -> Vec<QueryShape> {
        pairs
            .iter()
            .map(|(t, c)| QueryShape {
                text: t.to_string(),
                calls: *c,
            })
            .collect()
    }

    #[test]
    fn a_qualified_filter_is_attributed_to_its_table() {
        let (f, _) = analyze(&shapes(&[(
            "SELECT * FROM orders o WHERE o.tenant_id = $1 AND o.created_at > $2",
            500,
        )]));
        assert_eq!(f[&("orders".into(), "tenant_id".into())], 500);
        assert_eq!(f[&("orders".into(), "created_at".into())], 500);
    }

    #[test]
    fn calls_accumulate_across_shapes_so_frequency_ranks() {
        let (f, _) = analyze(&shapes(&[
            ("SELECT * FROM orders WHERE tenant_id = $1", 900),
            (
                "SELECT * FROM orders WHERE tenant_id = $1 AND status = $2",
                100,
            ),
        ]));
        let ranked = filters_for(&f, "public.orders");
        assert_eq!(ranked[0], ("tenant_id".into(), 1000));
        assert_eq!(ranked[1], ("status".into(), 100));
    }

    /// The rule that keeps a bad `ORDER BY` from looking derived: an unqualified column with more
    /// than one table in scope is ambiguous and is dropped.
    #[test]
    fn an_ambiguous_unqualified_column_is_not_attributed() {
        let (f, _) = analyze(&shapes(&[(
            "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id WHERE status = $1",
            10,
        )]));
        assert!(
            !f.keys().any(|(_, c)| c == "status"),
            "an unqualified column with two tables in scope must be dropped: {f:?}"
        );
    }

    #[test]
    fn an_unqualified_column_with_one_table_in_scope_is_attributed() {
        let (f, _) = analyze(&shapes(&[("SELECT * FROM orders WHERE status = $1", 10)]));
        assert_eq!(f[&("orders".into(), "status".into())], 10);
    }

    /// Ordering by a column only ever used with `<>` or `LIKE '%x'` buys nothing, because neither
    /// skips granules.
    #[test]
    fn only_granule_skipping_operators_count_as_filters() {
        let (f, _) = analyze(&shapes(&[(
            "SELECT * FROM orders WHERE status <> $1 AND note LIKE $2 AND created_at >= $3",
            10,
        )]));
        assert!(f.contains_key(&("orders".into(), "created_at".into())));
        assert!(!f.contains_key(&("orders".into(), "status".into())));
        assert!(!f.contains_key(&("orders".into(), "note".into())));
    }

    #[test]
    fn between_and_in_count_as_filters() {
        let (f, _) = analyze(&shapes(&[(
            "SELECT * FROM orders WHERE created_at BETWEEN $1 AND $2",
            7,
        )]));
        assert_eq!(f[&("orders".into(), "created_at".into())], 7);
        let (g, _) = analyze(&shapes(&[("SELECT * FROM orders WHERE id IN ($1, $2)", 3)]));
        assert_eq!(g[&("orders".into(), "id".into())], 3);
    }

    /// The join a repository never sees, because it only exists in a query the application issues.
    #[test]
    fn an_explicit_join_is_recovered_with_both_sides_resolved() {
        let (_, j) = analyze(&shapes(&[(
            "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id",
            10,
        )]));
        assert_eq!(
            j,
            vec![WorkloadJoin {
                left_table: "orders".into(),
                left_column: "customer_id".into(),
                right_table: "customers".into(),
                right_column: "id".into(),
            }]
        );
    }

    /// Older application SQL is full of implicit joins in the `WHERE` clause.
    #[test]
    fn an_implicit_join_in_the_where_clause_is_recovered_too() {
        let (_, j) = analyze(&shapes(&[(
            "SELECT * FROM orders o, customers c WHERE o.customer_id = c.id AND c.active = $1",
            10,
        )]));
        assert_eq!(j.len(), 1);
        assert_eq!(j[0].left_table, "orders");
        assert_eq!(j[0].right_table, "customers");
    }

    /// An equi-join predicate is a join, not a filter — counting it as both would rank join keys
    /// above the columns people actually filter on.
    #[test]
    fn a_join_predicate_is_not_also_counted_as_a_filter() {
        let (f, _) = analyze(&shapes(&[(
            "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id WHERE o.tenant_id = $1",
            10,
        )]));
        assert!(
            !f.contains_key(&("orders".into(), "customer_id".into())),
            "{f:?}"
        );
        assert!(f.contains_key(&("orders".into(), "tenant_id".into())));
    }

    /// The view holds utility statements, truncated text and extension syntax. None of it is a
    /// signal, and none of it may abort the harvest.
    #[test]
    fn unparseable_statements_are_skipped_not_counted() {
        let (f, j) = analyze(&shapes(&[
            ("VACUUM ANALYZE orders", 5),
            ("this is not sql at all", 5),
            ("SELECT * FROM orders WHERE tenant_id = $1", 5),
        ]));
        assert_eq!(f.len(), 1);
        assert!(j.is_empty());
    }

    #[test]
    fn a_self_join_on_the_same_column_is_not_recorded() {
        let (_, j) = analyze(&shapes(&[(
            "SELECT * FROM orders a JOIN orders b ON a.id = b.id",
            5,
        )]));
        assert!(j.is_empty(), "{j:?}");
    }

    #[test]
    fn filters_for_matches_a_qualified_table_name() {
        let mut counts: FilterCounts = BTreeMap::new();
        counts.insert(("orders".into(), "tenant_id".into()), 10);
        counts.insert(("customers".into(), "id".into()), 99);
        assert_eq!(
            filters_for(&counts, "public.orders"),
            vec![("tenant_id".to_string(), 10)]
        );
    }
}

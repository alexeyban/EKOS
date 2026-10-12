//! Deterministic lowering of a PostgreSQL view to a ClickHouse view.
//!
//! One walker reads the parsed statement and writes ClickHouse SQL. It is a whitelist: every
//! `match` ends in an arm that records a [`Blocker`], so a construct with no rule here stops the
//! lowering instead of being copied across with a different meaning.
//!
//! The differences it handles are the ones where the same text runs on both engines and returns
//! something else:
//!
//! | PostgreSQL | ClickHouse default | What is emitted |
//! |---|---|---|
//! | `sum`/`min`/`max` over no rows is `NULL` | `0` or an empty value | `sumOrNull` … |
//! | an outer join fills the missing side with `NULL` | with type defaults | `SETTINGS join_use_nulls = 1` |
//! | a name in `WHERE` is the column | the `SELECT` alias of that name | `SETTINGS prefer_column_name_to_alias = 1` |
//! | `ORDER BY x DESC` puts `NULL` first | last | `DESC NULLS FIRST` |
//! | `UNION` removes duplicates | needs a setting | `UNION DISTINCT` |
//! | `CAST(NULL AS integer)` is `NULL` | an error | `CAST(… AS Nullable(Int32))` |
//! | `length` counts characters | bytes | `char_length` |
//! | unquoted names fold to lower case | names are case-sensitive | folded, then quoted |
//! | `u.id` is output as `id` | as `u.id` when `id` is ambiguous | `u.id AS id` |
//!
//! Where the right answer depends on something this layer cannot see — operand types for `/`, a
//! refresh schedule for a materialized view — it blocks and says so.
//!
//! The round trip: the emitted text is parsed with the ClickHouse grammar and lowered again by the
//! same walker reading it as ClickHouse. It must come back byte-identical. That proves the text
//! parses in the target dialect and that the parser reads the structure the walker meant to write
//! (precedence, quoting, clause order). It does not prove the two engines agree on a result — that
//! is V5's job.

use ekos_migrate_target_clickhouse::{ColumnEvidence, Lossiness, map_column};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    BinaryOperator, CastKind, CreateTableOptions, DataType, Distinct, DuplicateTreatment, Expr,
    Function, FunctionArg, FunctionArgExpr, FunctionArguments, GroupByExpr, Ident, JoinConstraint,
    JoinOperator, ObjectName, Query, Select, SelectItem, SetExpr, SetOperator, SetQuantifier,
    Statement, TableFactor, TableWithJoins, UnaryOperator, Value,
};
use sqlparser::dialect::{ClickHouseDialect, PostgreSqlDialect};
use sqlparser::parser::Parser;
use std::collections::BTreeSet;

/// One reason a view was not lowered. `code` is stable; `detail` names the construct.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Blocker {
    pub code: String,
    pub detail: String,
}

/// A lowered view: the statement, and what was changed to keep its meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lowered {
    pub sql: String,
    /// One line per kind of rewrite applied, sorted.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Lowering {
    Lowered(Lowered),
    /// Sorted and deduplicated. Never empty.
    Blocked {
        blockers: Vec<Blocker>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LowerOptions {
    /// The ClickHouse database the migrated tables live in. A source schema qualifier is replaced
    /// by it; without one, names are emitted unqualified.
    pub database: Option<String>,
}

pub const PARSE: &str = "LOGIC.PARSE";
pub const NOT_A_VIEW: &str = "LOGIC.NOT_A_VIEW";
pub const MATVIEW: &str = "LOGIC.CH.MATERIALIZED_VIEW";
pub const VIEW_SHAPE: &str = "LOGIC.CH.VIEW_SHAPE";
pub const QUERY_SHAPE: &str = "LOGIC.CH.QUERY_SHAPE";
pub const EXPRESSION: &str = "LOGIC.CH.EXPRESSION";
pub const FUNCTION: &str = "LOGIC.CH.FUNCTION";
pub const DIVISION: &str = "LOGIC.CH.DIVISION_TYPES";
pub const NULL_SEMANTICS: &str = "LOGIC.CH.NULL_SEMANTICS";
pub const TYPE: &str = "LOGIC.CH.TYPE";
pub const OUTPUT_NAME: &str = "LOGIC.CH.OUTPUT_NAME";
pub const ROUND_TRIP: &str = "LOGIC.ROUND_TRIP";

/// Which grammar the statement being walked was parsed with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The source. Rewrites are applied.
    Postgres,
    /// Text this module emitted, read back. Only the target spellings are accepted.
    ClickHouse,
}

/// Source function → target function. An aggregate maps to its `-OrNull` form so an empty input
/// is `NULL` on both engines.
const FUNCTIONS: [(&str, &str); 10] = [
    ("count", "count"),
    ("sum", "sumOrNull"),
    ("min", "minOrNull"),
    ("max", "maxOrNull"),
    ("coalesce", "coalesce"),
    ("nullif", "nullIf"),
    ("lower", "lower"),
    ("upper", "upper"),
    ("abs", "abs"),
    ("length", "char_length"),
];
/// Every type name a lowered cast can carry, as ClickHouse spells it.
const TARGET_TYPES: [&str; 14] = [
    "Int16",
    "Int32",
    "Int64",
    "Bool",
    "Float32",
    "Float64",
    "UUID",
    "String",
    "Date32",
    "DateTime64",
    "Decimal32",
    "Decimal64",
    "Decimal128",
    "Decimal256",
];
const CONCAT: &str = "concat";
/// Not a rewrite: the two engines differ and the text cannot say so, so the reviewer is told.
const WILDCARD_NOTE: &str = "* is expanded when the view is queried; PostgreSQL fixed the column \
     list when the view was created, so a column added to the table later appears here";
const JOIN_USE_NULLS: &str = "join_use_nulls";
const PREFER_COLUMN: &str = "prefer_column_name_to_alias";

/// A backtick is doubled, the one spelling both ClickHouse and the parser used for the round trip
/// read as a literal backtick.
fn quote_ident(s: &str) -> String {
    format!("`{}`", s.replace('\\', "\\\\").replace('`', "``"))
}

fn quote_string(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The variant name of a node, for a blocker that has no better description.
fn variant<T: std::fmt::Debug>(node: &T) -> String {
    let text = format!("{node:?}");
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn clip(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 120 {
        format!("{}…", one_line.chars().take(120).collect::<String>())
    } else {
        one_line
    }
}

/// The name PostgreSQL gives an unaliased select-list expression, where that is a rule this
/// module can state: a column is its last identifier, a function call its name, `CASE` is `case`,
/// and a cast of a column keeps the column's name.
fn output_name(e: &Expr) -> Option<String> {
    let folded = |i: &Ident| match i.quote_style {
        None => i.value.to_lowercase(),
        Some(_) => i.value.clone(),
    };
    match e {
        Expr::Identifier(i) => Some(folded(i)),
        Expr::CompoundIdentifier(parts) => parts.last().map(folded),
        Expr::Function(f) => f.name.0.last().map(|i| i.value.to_lowercase()),
        Expr::Case { .. } => Some("case".into()),
        Expr::Cast { expr, .. } => match expr.as_ref() {
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => output_name(expr),
            _ => None,
        },
        Expr::Nested(inner) => output_name(inner),
        _ => None,
    }
}

struct Walker<'a> {
    side: Side,
    options: &'a LowerOptions,
    blockers: BTreeSet<Blocker>,
    notes: BTreeSet<String>,
}

/// What one `SELECT` block needs set to keep its meaning.
#[derive(Default, Clone, Copy)]
struct Needs {
    join_use_nulls: bool,
    prefer_column: bool,
}

impl Needs {
    fn merge(&mut self, other: Needs) {
        self.join_use_nulls |= other.join_use_nulls;
        self.prefer_column |= other.prefer_column;
    }

    fn clause(self) -> String {
        let mut parts = Vec::new();
        if self.join_use_nulls {
            parts.push(format!("{JOIN_USE_NULLS} = 1"));
        }
        if self.prefer_column {
            parts.push(format!("{PREFER_COLUMN} = 1"));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" SETTINGS {}", parts.join(", "))
        }
    }
}

impl Walker<'_> {
    fn block(&mut self, code: &str, detail: impl Into<String>) -> String {
        self.blockers.insert(Blocker {
            code: code.to_string(),
            detail: detail.into(),
        });
        String::new()
    }

    fn note(&mut self, text: &str) {
        if self.side == Side::Postgres {
            self.notes.insert(text.to_string());
        }
    }

    /// PostgreSQL folds an unquoted name to lower case; ClickHouse compares names as written.
    fn name(&self, ident: &Ident) -> String {
        match (self.side, ident.quote_style) {
            (Side::Postgres, None) => ident.value.to_lowercase(),
            _ => ident.value.clone(),
        }
    }

    fn ident(&self, ident: &Ident) -> String {
        quote_ident(&self.name(ident))
    }

    fn table_name(&mut self, name: &ObjectName) -> String {
        let parts: Vec<String> = name.0.iter().map(|i| self.name(i)).collect();
        let Some(table) = parts.last() else {
            return self.block(QUERY_SHAPE, "a table with no name");
        };
        match self.side {
            Side::ClickHouse => parts
                .iter()
                .map(|p| quote_ident(p))
                .collect::<Vec<_>>()
                .join("."),
            Side::Postgres => {
                if parts.len() > 2 {
                    return self.block(QUERY_SHAPE, format!("a database-qualified table: {name}"));
                }
                if parts.len() == 2 {
                    self.note("a schema qualifier was replaced by the target database");
                }
                match &self.options.database {
                    Some(db) => format!("{}.{}", quote_ident(db), quote_ident(table)),
                    None => quote_ident(table),
                }
            }
        }
    }

    /// What a query's own `SETTINGS` clause records. A source may not carry one.
    fn settings_needs(&mut self, q: &Query) -> Needs {
        let mut needs = Needs::default();
        if let Some(settings) = &q.settings {
            if self.side == Side::Postgres {
                self.block(QUERY_SHAPE, "a SETTINGS clause in the source");
            }
            for s in settings {
                match (s.key.value.as_str(), s.value.to_string().as_str()) {
                    (JOIN_USE_NULLS, "1") => needs.join_use_nulls = true,
                    (PREFER_COLUMN, "1") => needs.prefer_column = true,
                    (key, _) => {
                        self.block(QUERY_SHAPE, format!("an unexpected setting: {key}"));
                    }
                }
            }
        }
        needs
    }

    /// ` ORDER BY …` with every key's direction and NULL placement stated.
    fn order_by(&mut self, order: &sqlparser::ast::OrderBy) -> String {
        if order.interpolate.is_some() {
            self.block(QUERY_SHAPE, "ORDER BY … INTERPOLATE");
        }
        let mut keys = Vec::new();
        for o in &order.exprs {
            if o.with_fill.is_some() {
                self.block(QUERY_SHAPE, "ORDER BY … WITH FILL");
            }
            let descending = o.asc == Some(false);
            // PostgreSQL: NULL sorts as the largest value. ClickHouse: last either way.
            let nulls_first = match (o.nulls_first, self.side) {
                (Some(explicit), _) => explicit,
                (None, Side::Postgres) => descending,
                (None, Side::ClickHouse) => false,
            };
            if o.nulls_first.is_none() && descending {
                self.note("ORDER BY … DESC states NULLS FIRST, PostgreSQL's default");
            }
            keys.push(format!(
                "{} {} NULLS {}",
                self.expr(&o.expr),
                if descending { "DESC" } else { "ASC" },
                if nulls_first { "FIRST" } else { "LAST" }
            ));
        }
        format!(" ORDER BY {}", keys.join(", "))
    }

    fn query(&mut self, q: &Query, top: bool) -> (String, Needs) {
        if q.with.is_some() {
            self.block(QUERY_SHAPE, "WITH (common table expressions)");
        }
        if q.fetch.is_some() || !q.locks.is_empty() || q.for_clause.is_some() {
            self.block(QUERY_SHAPE, "FETCH / FOR UPDATE / FOR … clause");
        }
        if !q.limit_by.is_empty() || q.format_clause.is_some() {
            self.block(QUERY_SHAPE, "LIMIT BY / FORMAT clause");
        }
        let mut needs = self.settings_needs(q);
        let simple =
            q.order_by.is_none() && q.limit.is_none() && q.offset.is_none() && q.settings.is_none();
        let (mut sql, body_needs) = self.set_expr(&q.body, top && simple);
        // Read back, the clause is the record of what the block needs; the source computes it.
        if self.side == Side::Postgres {
            needs = body_needs;
        }
        if let Some(order) = &q.order_by {
            sql.push_str(&self.order_by(order));
        }
        if let Some(limit) = &q.limit {
            sql.push_str(&format!(" LIMIT {}", self.count_literal(limit, "LIMIT")));
        }
        if let Some(offset) = &q.offset {
            sql.push_str(&format!(
                " OFFSET {}",
                self.count_literal(&offset.value, "OFFSET")
            ));
        }
        if top || q.settings.is_some() {
            sql.push_str(&needs.clause());
        }
        (sql, needs)
    }

    fn count_literal(&mut self, e: &Expr, what: &str) -> String {
        match e {
            Expr::Value(Value::Number(n, _)) if n.chars().all(|c| c.is_ascii_digit()) => n.clone(),
            other => self.block(QUERY_SHAPE, format!("{what} {other} is not a plain number")),
        }
    }

    /// `own_settings`: this expression is the whole body of the top-level query, so its settings
    /// are written once by the caller. A member of a set operation carries its own.
    fn set_expr(&mut self, body: &SetExpr, own_settings: bool) -> (String, Needs) {
        match body {
            SetExpr::Select(select) => self.select(select),
            SetExpr::Query(inner) => {
                let (sql, needs) = self.query(inner, false);
                (format!("({sql})"), needs)
            }
            SetExpr::SetOperation {
                op: SetOperator::Union,
                set_quantifier,
                left,
                right,
            } => {
                let keyword = match set_quantifier {
                    SetQuantifier::All => "UNION ALL",
                    SetQuantifier::Distinct => "UNION DISTINCT",
                    SetQuantifier::None => {
                        self.note("UNION is written UNION DISTINCT");
                        "UNION DISTINCT"
                    }
                    other => {
                        self.block(QUERY_SHAPE, format!("UNION {other}"));
                        "UNION ALL"
                    }
                };
                let l = self.member(left);
                let r = self.member(right);
                let _ = own_settings;
                (format!("{l} {keyword} {r}"), Needs::default())
            }
            SetExpr::SetOperation { op, .. } => (
                self.block(QUERY_SHAPE, format!("{op} set operation")),
                Needs::default(),
            ),
            other => (
                self.block(QUERY_SHAPE, format!("{} as a query body", variant(other))),
                Needs::default(),
            ),
        }
    }

    /// One side of a set operation. Its settings cannot be written after the whole `UNION` — there
    /// they would bind to the last block only — so a block that needs any is parenthesized with
    /// its own clause.
    fn member(&mut self, body: &SetExpr) -> String {
        match body {
            SetExpr::Select(select) => {
                let (sql, needs) = self.select(select);
                let clause = needs.clause();
                if clause.is_empty() {
                    sql
                } else {
                    format!("({sql}{clause})")
                }
            }
            SetExpr::Query(inner) if self.side == Side::ClickHouse => {
                let (sql, _) = self.query(inner, false);
                format!("({sql})")
            }
            other => self.set_expr(other, false).0,
        }
    }

    fn select(&mut self, s: &Select) -> (String, Needs) {
        let mut needs = Needs::default();
        if s.top.is_some()
            || s.into.is_some()
            || !s.lateral_views.is_empty()
            || s.prewhere.is_some()
            || !s.cluster_by.is_empty()
            || !s.distribute_by.is_empty()
            || !s.sort_by.is_empty()
            || !s.named_window.is_empty()
            || s.qualify.is_some()
            || s.value_table_mode.is_some()
            || s.connect_by.is_some()
        {
            self.block(
                QUERY_SHAPE,
                "TOP / INTO / PREWHERE / WINDOW / QUALIFY or a similar clause",
            );
        }
        let mut sql = String::from("SELECT ");
        match &s.distinct {
            None => {}
            Some(Distinct::Distinct) => sql.push_str("DISTINCT "),
            Some(Distinct::On(_)) => {
                self.block(QUERY_SHAPE, "DISTINCT ON");
            }
        }
        let joined = s.from.len() > 1 || s.from.iter().any(|t| !t.joins.is_empty());
        let mut items = Vec::new();
        for item in &s.projection {
            if joined
                && self.side == Side::Postgres
                && matches!(
                    item,
                    SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..)
                )
            {
                self.block(
                    OUTPUT_NAME,
                    format!(
                        "{item} over a join: ClickHouse names a column `t.c` when another joined \
                         table has a `c` too, and which columns those are is not known here. \
                         List the columns"
                    ),
                );
            }
            items.push(match item {
                // PostgreSQL names an output column after its last identifier (`u.id` is `id`).
                // ClickHouse keeps the qualifier when the bare name is ambiguous, so the name is
                // stated. Found by V5 on LedgerSMB's `user_listable`.
                // A bare column already has the same name on both.
                SelectItem::UnnamedExpr(e @ Expr::Identifier(_)) => self.expr(e),
                SelectItem::UnnamedExpr(e) if self.side == Side::Postgres => match output_name(e) {
                    Some(name) => {
                        // An alias equal to its own column's name shadows nothing.
                        if !matches!(e, Expr::CompoundIdentifier(_)) {
                            needs.prefer_column = true;
                        }
                        self.note(
                            "an unaliased select-list item is given the column name PostgreSQL \
                             gives it",
                        );
                        format!("{} AS {}", self.expr(e), quote_ident(&name))
                    }
                    None => {
                        self.block(
                            OUTPUT_NAME,
                            format!(
                                "an unnamed expression in the select list: {} — the engines \
                                     name its column differently. Give it an alias in the source",
                                clip(&e.to_string())
                            ),
                        );
                        self.expr(e)
                    }
                },
                SelectItem::UnnamedExpr(e) => self.expr(e),
                SelectItem::ExprWithAlias { expr, alias } => {
                    needs.prefer_column = true;
                    format!("{} AS {}", self.expr(expr), self.ident(alias))
                }
                SelectItem::Wildcard(o) if o.to_string().is_empty() => {
                    self.note(WILDCARD_NOTE);
                    "*".to_string()
                }
                SelectItem::QualifiedWildcard(name, o) if o.to_string().is_empty() => {
                    self.note(WILDCARD_NOTE);
                    let parts: Vec<String> = name.0.iter().map(|i| self.ident(i)).collect();
                    format!("{}.*", parts.join("."))
                }
                other => self.block(QUERY_SHAPE, format!("select item {other}")),
            });
        }
        if needs.prefer_column {
            self.note(
                "prefer_column_name_to_alias = 1: a name in WHERE means the column, not a SELECT alias",
            );
        }
        sql.push_str(&items.join(", "));
        if !s.from.is_empty() {
            let mut from = Vec::new();
            for twj in &s.from {
                from.push(self.table_with_joins(twj, &mut needs));
            }
            sql.push_str(&format!(" FROM {}", from.join(", ")));
        }
        if let Some(w) = &s.selection {
            sql.push_str(&format!(" WHERE {}", self.expr(w)));
        }
        match &s.group_by {
            GroupByExpr::Expressions(exprs, modifiers) => {
                if !modifiers.is_empty() {
                    self.block(QUERY_SHAPE, "GROUP BY … WITH ROLLUP / CUBE / TOTALS");
                }
                if !exprs.is_empty() {
                    let keys: Vec<String> = exprs.iter().map(|e| self.expr(e)).collect();
                    sql.push_str(&format!(" GROUP BY {}", keys.join(", ")));
                }
            }
            GroupByExpr::All(_) => {
                self.block(QUERY_SHAPE, "GROUP BY ALL");
            }
        }
        if let Some(h) = &s.having {
            sql.push_str(&format!(" HAVING {}", self.expr(h)));
        }
        (sql, needs)
    }

    fn table_with_joins(&mut self, twj: &TableWithJoins, needs: &mut Needs) -> String {
        let mut sql = self.table_factor(&twj.relation, needs);
        for join in &twj.joins {
            if join.global {
                self.block(QUERY_SHAPE, "GLOBAL JOIN");
            }
            let (keyword, constraint) = match &join.join_operator {
                JoinOperator::Inner(c) => ("INNER JOIN", Some(c)),
                JoinOperator::LeftOuter(c) => ("LEFT OUTER JOIN", Some(c)),
                JoinOperator::RightOuter(c) => ("RIGHT OUTER JOIN", Some(c)),
                JoinOperator::FullOuter(c) => ("FULL OUTER JOIN", Some(c)),
                JoinOperator::CrossJoin => ("CROSS JOIN", None),
                other => {
                    self.block(QUERY_SHAPE, format!("{} join", variant(other)));
                    ("INNER JOIN", None)
                }
            };
            if keyword.contains("OUTER") {
                needs.join_use_nulls = true;
                self.note("join_use_nulls = 1: an outer join fills the missing side with NULL");
            }
            let right = self.table_factor(&join.relation, needs);
            let on = match constraint {
                Some(JoinConstraint::On(e)) => format!(" ON {}", self.expr(e)),
                Some(JoinConstraint::Using(cols)) => {
                    let cols: Vec<String> = cols.iter().map(|c| self.ident(c)).collect();
                    format!(" USING ({})", cols.join(", "))
                }
                None => String::new(),
                Some(other) => self.block(QUERY_SHAPE, format!("{} join", variant(other))),
            };
            sql.push_str(&format!(" {keyword} {right}{on}"));
        }
        sql
    }

    fn table_factor(&mut self, factor: &TableFactor, needs: &mut Needs) -> String {
        match factor {
            TableFactor::Table {
                name,
                alias,
                args: None,
                with_hints,
                version: None,
                with_ordinality: false,
                partitions,
                json_path: None,
            } if with_hints.is_empty() && partitions.is_empty() => {
                let mut sql = self.table_name(name);
                if let Some(a) = alias {
                    if !a.columns.is_empty() {
                        self.block(QUERY_SHAPE, "a table alias with a column list");
                    }
                    sql.push_str(&format!(" AS {}", self.ident(&a.name)));
                }
                sql
            }
            TableFactor::Derived {
                lateral: false,
                subquery,
                alias: Some(alias),
            } if alias.columns.is_empty() => {
                let (inner, inner_needs) = self.query(subquery, false);
                needs.merge(inner_needs);
                format!("({inner}) AS {}", self.ident(&alias.name))
            }
            other => self.block(
                QUERY_SHAPE,
                format!("{} in FROM: {}", variant(other), clip(&other.to_string())),
            ),
        }
    }

    fn expr(&mut self, e: &Expr) -> String {
        match e {
            Expr::Identifier(i) => self.ident(i),
            Expr::CompoundIdentifier(parts) => {
                // `schema.table.column`: the schema went with the table's own qualifier.
                let keep = if self.side == Side::Postgres && parts.len() > 2 {
                    &parts[parts.len() - 2..]
                } else {
                    &parts[..]
                };
                keep.iter()
                    .map(|i| self.ident(i))
                    .collect::<Vec<_>>()
                    .join(".")
            }
            Expr::Value(v) => self.value(v),
            Expr::Nested(inner) => self.expr(inner),
            Expr::UnaryOp { op, expr } => {
                let inner = self.expr(expr);
                match op {
                    UnaryOperator::Not => format!("(NOT {inner})"),
                    UnaryOperator::Minus => format!("(-{inner})"),
                    UnaryOperator::Plus => inner,
                    other => self.block(EXPRESSION, format!("unary operator {other}")),
                }
            }
            Expr::BinaryOp { left, op, right } => self.binary(left, op, right),
            Expr::IsNull(inner) => format!("({} IS NULL)", self.expr(inner)),
            Expr::IsNotNull(inner) => format!("({} IS NOT NULL)", self.expr(inner)),
            Expr::Between {
                expr,
                negated,
                low,
                high,
            } => format!(
                "({} {}BETWEEN {} AND {})",
                self.expr(expr),
                if *negated { "NOT " } else { "" },
                self.expr(low),
                self.expr(high)
            ),
            Expr::InList {
                expr,
                list,
                negated,
            } => {
                // `x NOT IN (1, NULL)` keeps no row in PostgreSQL and every other row in
                // ClickHouse; only a list of non-null literals means the same on both.
                let literal = |e: &Expr| {
                    matches!(
                        e,
                        Expr::Value(Value::Number(..) | Value::SingleQuotedString(_))
                    )
                };
                if !list.iter().all(literal) {
                    self.block(
                        NULL_SEMANTICS,
                        format!(
                            "IN over something other than non-null literals: {}",
                            clip(&e.to_string())
                        ),
                    );
                }
                let items: Vec<String> = list.iter().map(|i| self.expr(i)).collect();
                format!(
                    "({} {}IN ({}))",
                    self.expr(expr),
                    if *negated { "NOT " } else { "" },
                    items.join(", ")
                )
            }
            Expr::Like {
                negated,
                any: false,
                expr,
                pattern,
                escape_char: None,
            } => format!(
                "({} {}LIKE {})",
                self.expr(expr),
                if *negated { "NOT " } else { "" },
                self.expr(pattern)
            ),
            Expr::ILike {
                negated,
                any: false,
                expr,
                pattern,
                escape_char: None,
            } => format!(
                "({} {}ILIKE {})",
                self.expr(expr),
                if *negated { "NOT " } else { "" },
                self.expr(pattern)
            ),
            Expr::Case {
                operand,
                conditions,
                results,
                else_result,
            } => {
                let mut sql = String::from("CASE");
                if let Some(o) = operand {
                    sql.push_str(&format!(" {}", self.expr(o)));
                }
                for (c, r) in conditions.iter().zip(results) {
                    sql.push_str(&format!(" WHEN {} THEN {}", self.expr(c), self.expr(r)));
                }
                if let Some(e) = else_result {
                    sql.push_str(&format!(" ELSE {}", self.expr(e)));
                }
                sql.push_str(" END");
                sql
            }
            Expr::Cast {
                kind: CastKind::Cast | CastKind::DoubleColon,
                expr,
                data_type,
                format: None,
            } => {
                let ty = self.cast_type(data_type);
                format!("CAST({} AS {ty})", self.expr(expr))
            }
            Expr::Function(f) => self.function(f),
            Expr::IsTrue(_) | Expr::IsFalse(_) | Expr::IsNotTrue(_) | Expr::IsNotFalse(_) => self
                .block(
                    NULL_SEMANTICS,
                    format!("IS [NOT] TRUE / FALSE: {}", clip(&e.to_string())),
                ),
            Expr::Subquery(_) | Expr::Exists { .. } | Expr::InSubquery { .. } => self.block(
                EXPRESSION,
                format!("a subquery in an expression: {}", clip(&e.to_string())),
            ),
            other => self.block(
                EXPRESSION,
                format!("{}: {}", variant(other), clip(&other.to_string())),
            ),
        }
    }

    fn value(&mut self, v: &Value) -> String {
        match v {
            Value::Number(n, false) => n.clone(),
            Value::SingleQuotedString(s) => quote_string(s),
            Value::Boolean(b) => b.to_string(),
            Value::Null => "NULL".to_string(),
            other => self.block(EXPRESSION, format!("literal {other}")),
        }
    }

    fn binary(&mut self, left: &Expr, op: &BinaryOperator, right: &Expr) -> String {
        let l = self.expr(left);
        let r = self.expr(right);
        let symbol = match op {
            BinaryOperator::And => "AND",
            BinaryOperator::Or => "OR",
            BinaryOperator::Eq => "=",
            BinaryOperator::NotEq => "<>",
            BinaryOperator::Lt => "<",
            BinaryOperator::LtEq => "<=",
            BinaryOperator::Gt => ">",
            BinaryOperator::GtEq => ">=",
            BinaryOperator::Plus => "+",
            BinaryOperator::Minus => "-",
            BinaryOperator::Multiply => "*",
            BinaryOperator::StringConcat => {
                self.note("a || b is written concat(a, b)");
                return format!("{CONCAT}({l}, {r})");
            }
            BinaryOperator::Divide => {
                return self.block(
                    DIVISION,
                    format!(
                        "{left} / {right}: PostgreSQL divides two integers as integers, ClickHouse \
                         as floats, and the operand types are not known here"
                    ),
                );
            }
            other => return self.block(EXPRESSION, format!("operator {other}")),
        };
        format!("({l} {symbol} {r})")
    }

    fn cast_type(&mut self, ty: &DataType) -> String {
        if self.side == Side::ClickHouse {
            return match ty {
                // The parser prints some ClickHouse types in upper case; each is written back the
                // way ClickHouse spells it, and a type this module never emits is refused.
                DataType::Nullable(inner) => {
                    let printed = inner.to_string().replace(' ', "");
                    let (head, params) = match printed.split_once('(') {
                        Some((h, rest)) => (h, format!("({rest}")),
                        None => (printed.as_str(), String::new()),
                    };
                    match TARGET_TYPES.iter().find(|t| t.eq_ignore_ascii_case(head)) {
                        Some(spelling) => format!("Nullable({spelling}{params})"),
                        None => self.block(TYPE, format!("an unexpected cast type: {printed}")),
                    }
                }
                other => self.block(TYPE, format!("a cast to a non-Nullable type: {other}")),
            };
        }
        let source = ty.to_string().to_lowercase();
        let mapping = map_column("", &source, true, &ColumnEvidence::default());
        if mapping.lossiness == Lossiness::Lossy || mapping.target_type.contains('\'') {
            return self.block(TYPE, format!("a cast to {source}: {}", mapping.rationale));
        }
        if !mapping.target_type.starts_with("Nullable(") {
            return self.block(
                TYPE,
                format!("a cast to {source} did not map to a Nullable type"),
            );
        }
        self.note("a cast targets Nullable(T), so a NULL input stays NULL");
        mapping.target_type.replace(' ', "")
    }

    fn function(&mut self, f: &Function) -> String {
        let written = f.name.to_string();
        let target = match self.side {
            Side::Postgres => FUNCTIONS
                .iter()
                .find(|(source, _)| written.eq_ignore_ascii_case(source))
                .map(|(_, target)| *target),
            Side::ClickHouse => FUNCTIONS
                .iter()
                .map(|(_, target)| *target)
                .chain([CONCAT])
                .find(|target| written == *target),
        };
        let Some(target) = target else {
            return self.block(FUNCTION, format!("{written}() has no lowering rule"));
        };
        if f.over.is_some()
            || f.filter.is_some()
            || f.null_treatment.is_some()
            || !f.within_group.is_empty()
            || !matches!(f.parameters, FunctionArguments::None)
        {
            return self.block(
                FUNCTION,
                format!("{written}() with OVER / FILTER / WITHIN GROUP"),
            );
        }
        if self.side == Side::Postgres && !written.eq_ignore_ascii_case(target) {
            self.note(match target {
                "char_length" => "length() is written char_length(): characters, not bytes",
                "nullIf" => "nullif() is written nullIf()",
                _ => "sum / min / max use the -OrNull form: NULL over no rows, as in PostgreSQL",
            });
        }
        let FunctionArguments::List(list) = &f.args else {
            return self.block(FUNCTION, format!("{written} without an argument list"));
        };
        if !list.clauses.is_empty() {
            return self.block(
                FUNCTION,
                format!("{written}() with ORDER BY / LIMIT inside"),
            );
        }
        let distinct = match list.duplicate_treatment {
            Some(DuplicateTreatment::Distinct) => "DISTINCT ",
            _ => "",
        };
        let mut args = Vec::new();
        for a in &list.args {
            args.push(match a {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => self.expr(e),
                FunctionArg::Unnamed(FunctionArgExpr::Wildcard) if target == "count" => {
                    "*".to_string()
                }
                other => self.block(FUNCTION, format!("argument {other} of {written}()")),
            });
        }
        format!("{target}({distinct}{})", args.join(", "))
    }

    fn create_view(&mut self, stmt: &Statement) -> Option<String> {
        let Statement::CreateView {
            materialized,
            name,
            columns,
            query,
            options,
            cluster_by,
            comment,
            with_no_schema_binding,
            temporary,
            to,
            ..
        } = stmt
        else {
            self.block(NOT_A_VIEW, format!("a {} statement", variant(stmt)));
            return None;
        };
        if *materialized {
            self.block(
                MATVIEW,
                "a ClickHouse MATERIALIZED VIEW runs on insert into its source table: it never \
                 sees existing rows, updates or deletes. A PostgreSQL materialized view is a \
                 stored result refreshed on demand. The equivalent is a refreshable materialized \
                 view, which needs a schedule and a target table — a decision, not a lowering",
            );
        }
        if *temporary {
            self.block(VIEW_SHAPE, "a TEMPORARY view");
        }
        if !columns.is_empty() {
            self.block(
                VIEW_SHAPE,
                "a view column list: ClickHouse needs a type per listed column",
            );
        }
        if !matches!(options, CreateTableOptions::None)
            || !cluster_by.is_empty()
            || comment.is_some()
            || *with_no_schema_binding
            || to.is_some()
        {
            self.block(VIEW_SHAPE, "view options (WITH (…), TO, COMMENT)");
        }
        let view = self.table_name(name);
        let (query, _) = self.query(query, true);
        Some(format!("CREATE VIEW {view} AS {query}"))
    }
}

fn walk(
    sql: &str,
    side: Side,
    options: &LowerOptions,
) -> Result<(String, BTreeSet<String>), BTreeSet<Blocker>> {
    let mut walker = Walker {
        side,
        options,
        blockers: BTreeSet::new(),
        notes: BTreeSet::new(),
    };
    let parsed = match side {
        Side::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        Side::ClickHouse => Parser::parse_sql(&ClickHouseDialect {}, sql),
    };
    let out = match parsed.as_deref() {
        Ok([stmt]) => walker.create_view(stmt),
        Ok(stmts) => {
            walker.block(
                NOT_A_VIEW,
                format!("{} statements, expected one", stmts.len()),
            );
            None
        }
        Err(e) => {
            walker.block(PARSE, e.to_string());
            None
        }
    };
    match out {
        Some(sql) if walker.blockers.is_empty() => Ok((sql, walker.notes)),
        _ => Err(walker.blockers),
    }
}

/// Lower one `CREATE [OR REPLACE] VIEW` definition, as written in the source.
pub fn lower_view(definition: &str, options: &LowerOptions) -> Lowering {
    let blocked = |blockers: BTreeSet<Blocker>| Lowering::Blocked {
        blockers: blockers.into_iter().collect(),
    };
    let (sql, notes) = match walk(definition, Side::Postgres, options) {
        Ok(ok) => ok,
        Err(blockers) => return blocked(blockers),
    };
    // The emitted statement must mean, to the target's own parser, what the walker wrote.
    let failed = |detail: String| {
        blocked(BTreeSet::from([Blocker {
            code: ROUND_TRIP.to_string(),
            detail,
        }]))
    };
    match walk(&sql, Side::ClickHouse, options) {
        Ok((again, _)) if again == sql => Lowering::Lowered(Lowered {
            sql,
            notes: notes.into_iter().collect(),
        }),
        Ok((again, _)) => failed(format!(
            "the emitted statement reads back differently — emitted: {sql} — read back: {again}"
        )),
        Err(blockers) => failed(format!(
            "the emitted statement is not accepted when read back — emitted: {sql} — {}",
            blockers
                .iter()
                .map(|b| format!("{}: {}", b.code, b.detail))
                .collect::<Vec<_>>()
                .join("; ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowered(sql: &str) -> Lowered {
        match lower_view(sql, &LowerOptions::default()) {
            Lowering::Lowered(l) => l,
            Lowering::Blocked { blockers } => panic!("blocked: {blockers:#?}"),
        }
    }

    fn codes(sql: &str) -> Vec<String> {
        match lower_view(sql, &LowerOptions::default()) {
            Lowering::Lowered(l) => panic!("lowered: {}", l.sql),
            Lowering::Blocked { blockers } => {
                assert!(!blockers.is_empty());
                blockers.into_iter().map(|b| b.code).collect()
            }
        }
    }

    #[test]
    fn a_plain_view_lowers_and_quotes_every_name() {
        let l = lowered(
            "CREATE VIEW Open_Items AS SELECT i.id, i.Amount FROM public.items i WHERE i.closed = false",
        );
        assert_eq!(
            l.sql,
            "CREATE VIEW `open_items` AS SELECT `i`.`id` AS `id`, `i`.`amount` AS `amount` \
             FROM `items` AS `i` WHERE (`i`.`closed` = false)"
        );
        assert_eq!(
            l.notes,
            [
                "a schema qualifier was replaced by the target database",
                "an unaliased select-list item is given the column name PostgreSQL gives it"
            ]
        );
        let star = lowered("CREATE VIEW v AS SELECT t.* FROM t");
        assert!(star.notes[0].starts_with("* is expanded when the view is queried"));
    }

    #[test]
    fn the_target_database_qualifies_every_table() {
        let options = LowerOptions {
            database: Some("lsmb".into()),
        };
        let Lowering::Lowered(l) = lower_view("CREATE VIEW v AS SELECT * FROM public.t", &options)
        else {
            panic!("blocked");
        };
        assert_eq!(l.sql, "CREATE VIEW `lsmb`.`v` AS SELECT * FROM `lsmb`.`t`");
    }

    #[test]
    fn a_quoted_name_keeps_its_case_and_a_backtick_is_escaped() {
        let l = lowered("CREATE VIEW v AS SELECT \"Mixed\", \"odd`name\" FROM t");
        assert!(l.sql.contains("`Mixed`"), "{}", l.sql);
        assert!(l.sql.contains("`odd``name`"), "{}", l.sql);
    }

    #[test]
    fn aggregates_keep_null_over_no_rows() {
        let l = lowered(
            "CREATE VIEW v AS SELECT account, sum(amount), max(day), count(*), \
             count(DISTINCT payer) AS payers FROM t GROUP BY account HAVING sum(amount) > 0",
        );
        assert!(
            l.sql.contains(
                "`account`, sumOrNull(`amount`) AS `sum`, maxOrNull(`day`) AS `max`, count(*) AS `count`, count(DISTINCT `payer`) AS `payers`"
            ),
            "{}",
            l.sql
        );
        assert!(
            l.sql.contains("HAVING (sumOrNull(`amount`) > 0)"),
            "{}",
            l.sql
        );
        assert!(l.notes.iter().any(|n| n.contains("-OrNull")));
    }

    #[test]
    fn an_outer_join_and_an_alias_each_set_what_they_need() {
        let l = lowered(
            "CREATE VIEW v AS SELECT e.id, c.name AS company FROM entity e \
             LEFT JOIN company c ON c.entity_id = e.id",
        );
        assert!(
            l.sql
                .ends_with("SETTINGS join_use_nulls = 1, prefer_column_name_to_alias = 1"),
            "{}",
            l.sql
        );
        assert!(
            l.sql
                .contains("LEFT OUTER JOIN `company` AS `c` ON (`c`.`entity_id` = `e`.`id`)")
        );
        // An inner join with no alias needs neither.
        let plain = lowered("CREATE VIEW v AS SELECT a.id FROM a JOIN b ON a.id = b.a_id");
        assert!(!plain.sql.contains("SETTINGS"), "{}", plain.sql);
    }

    #[test]
    fn descending_order_states_postgres_null_placement() {
        let l = lowered("CREATE VIEW v AS SELECT id FROM t ORDER BY day DESC, id LIMIT 10");
        assert!(
            l.sql
                .ends_with("ORDER BY `day` DESC NULLS FIRST, `id` ASC NULLS LAST LIMIT 10"),
            "{}",
            l.sql
        );
    }

    #[test]
    fn union_is_distinct_and_each_block_carries_its_own_settings() {
        let l = lowered(
            "CREATE VIEW v AS SELECT id FROM a UNION SELECT b.id FROM b LEFT JOIN c ON c.id = b.id",
        );
        assert_eq!(
            l.sql,
            "CREATE VIEW `v` AS SELECT `id` FROM `a` UNION DISTINCT (SELECT `b`.`id` AS `id` \
             FROM `b` LEFT OUTER JOIN `c` ON (`c`.`id` = `b`.`id`) SETTINGS join_use_nulls = 1)"
        );
    }

    #[test]
    fn casts_concat_case_and_length_are_rewritten() {
        let l = lowered(
            "CREATE VIEW v AS SELECT id::text, CAST(amount AS numeric(12,2)), \
             first || ' ' || last AS full_name, \
             CASE WHEN length(code) > 3 THEN 'long' ELSE 'short' END FROM t \
             WHERE kind IN ('a', 'b') AND day BETWEEN 1 AND 9 AND name ILIKE 'x%'",
        );
        for part in [
            "CAST(`id` AS Nullable(String)) AS `id`",
            "CAST(`amount` AS Nullable(Decimal64(2))) AS `amount`",
            "concat(concat(`first`, ' '), `last`) AS `full_name`",
            "CASE WHEN (char_length(`code`) > 3) THEN 'long' ELSE 'short' END AS `case`",
            "(`kind` IN ('a', 'b'))",
            "(`day` BETWEEN 1 AND 9)",
            "(`name` ILIKE 'x%')",
        ] {
            assert!(l.sql.contains(part), "missing {part} in {}", l.sql);
        }
    }

    #[test]
    fn every_mapped_cast_type_reads_back() {
        for (source, target) in [
            ("smallint", "Int16"),
            ("integer", "Int32"),
            ("bigint", "Int64"),
            ("boolean", "Bool"),
            ("real", "Float32"),
            ("double precision", "Float64"),
            ("uuid", "UUID"),
            ("text", "String"),
            ("varchar(20)", "String"),
            ("date", "Date32"),
            ("timestamp", "DateTime64(6)"),
            ("numeric(30,4)", "Decimal128(4)"),
        ] {
            let l = lowered(&format!(
                "CREATE VIEW v AS SELECT CAST(x AS {source}) FROM t"
            ));
            assert!(
                l.sql.contains(&format!("CAST(`x` AS Nullable({target}))")),
                "{source}: {}",
                l.sql
            );
        }
        // No precision: the target width would be a guess.
        assert!(codes("CREATE VIEW v AS SELECT x::numeric FROM t").contains(&TYPE.to_string()));
    }

    #[test]
    fn a_derived_table_passes_its_needs_up() {
        let l = lowered(
            "CREATE VIEW v AS SELECT s.id FROM (SELECT a.id FROM a LEFT JOIN b ON a.id = b.id) s",
        );
        assert!(
            l.sql.ends_with(") AS `s` SETTINGS join_use_nulls = 1"),
            "{}",
            l.sql
        );
    }

    #[test]
    fn what_would_change_meaning_is_blocked_by_name() {
        for (sql, code) in [
            ("CREATE MATERIALIZED VIEW v AS SELECT id FROM t", MATVIEW),
            ("CREATE VIEW v AS SELECT a / b AS r FROM t", DIVISION),
            ("CREATE VIEW v AS SELECT a + b FROM t", OUTPUT_NAME),
            (
                "CREATE VIEW v AS SELECT t.*, u.x FROM t JOIN u ON u.id = t.id",
                OUTPUT_NAME,
            ),
            ("CREATE VIEW v AS SELECT * FROM t, u", OUTPUT_NAME),
            (
                "CREATE VIEW v AS SELECT id FROM t WHERE x NOT IN (1, NULL)",
                NULL_SEMANTICS,
            ),
            (
                "CREATE VIEW v AS SELECT id FROM t WHERE flag IS TRUE",
                NULL_SEMANTICS,
            ),
            ("CREATE VIEW v AS SELECT avg(x) FROM t", FUNCTION),
            ("CREATE VIEW v AS SELECT now() FROM t", FUNCTION),
            (
                "CREATE VIEW v AS SELECT sum(x) OVER (PARTITION BY y) FROM t",
                FUNCTION,
            ),
            (
                "CREATE VIEW v AS SELECT DISTINCT ON (a) a, b FROM t",
                QUERY_SHAPE,
            ),
            (
                "CREATE VIEW v AS WITH c AS (SELECT 1) SELECT * FROM c",
                QUERY_SHAPE,
            ),
            (
                "CREATE VIEW v AS SELECT id FROM a EXCEPT SELECT id FROM b",
                QUERY_SHAPE,
            ),
            (
                "CREATE VIEW v AS SELECT id FROM a NATURAL JOIN b",
                QUERY_SHAPE,
            ),
            (
                "CREATE VIEW v AS SELECT id FROM generate_series(1, 3) g(id)",
                QUERY_SHAPE,
            ),
            (
                "CREATE VIEW v AS SELECT id FROM t WHERE EXISTS (SELECT 1 FROM u)",
                EXPRESSION,
            ),
            ("CREATE VIEW v AS SELECT x::interval FROM t", TYPE),
            ("CREATE VIEW v AS SELECT x::timestamptz FROM t", TYPE),
            ("CREATE VIEW v (a, b) AS SELECT x, y FROM t", VIEW_SHAPE),
            ("CREATE TABLE t (id int)", NOT_A_VIEW),
            ("CREATE VIEW v AS SELEC id FROM t", PARSE),
        ] {
            assert!(
                codes(sql).iter().any(|c| c == code),
                "{sql}: {:?}",
                codes(sql)
            );
        }
    }

    #[test]
    fn every_blocker_is_reported_not_only_the_first() {
        let c = codes("CREATE MATERIALIZED VIEW v AS SELECT a / b AS r, now() FROM t");
        assert_eq!(c, [DIVISION, FUNCTION, MATVIEW]);
    }

    #[test]
    fn lowering_is_deterministic_and_reads_back_identically() {
        let sql = "CREATE VIEW v AS SELECT e.id, sum(l.amount) AS total FROM entity e \
                   LEFT JOIN line l ON l.entity_id = e.id WHERE e.kind IN (1, 2) \
                   GROUP BY e.id ORDER BY total DESC";
        let first = lowered(sql);
        assert_eq!(first, lowered(sql));
        // The emitted statement is a fixed point of the walker reading ClickHouse.
        let (again, _) = walk(&first.sql, Side::ClickHouse, &LowerOptions::default()).unwrap();
        assert_eq!(again, first.sql);
    }

    #[test]
    fn text_this_module_did_not_emit_is_not_accepted_as_a_read_back() {
        // The source spellings are refused on the target side: `sum` would return 0 over no rows.
        let err = walk(
            "CREATE VIEW v AS SELECT sum(x) FROM t",
            Side::ClickHouse,
            &LowerOptions::default(),
        )
        .unwrap_err();
        assert!(err.iter().any(|b| b.code == FUNCTION));
        let err = walk(
            "CREATE VIEW v AS SELECT CAST(x AS Int32) FROM t",
            Side::ClickHouse,
            &LowerOptions::default(),
        )
        .unwrap_err();
        assert!(err.iter().any(|b| b.code == TYPE));
    }
}

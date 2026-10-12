//! V5 — differential execution of a lowered view (RFC 0164).
//!
//! The static checks say the lowering parses and invents nothing. This runs it. A small fixture is
//! built from the view's own comparisons, loaded into a PostgreSQL sandbox and a ClickHouse
//! sandbox, the source view is created on one and the lowered view on the other, and the two
//! result sets are compared in RFC 0155's canonical form.
//!
//! The fixture is what makes it more than a smoke test. For every `column <op> literal` the view
//! writes, the column's values include the literal and its neighbours, so each comparison is both
//! true and false somewhere; every column also takes `NULL`. **Branch coverage** is then measured,
//! not assumed: for each such comparison, does the fixture hold a row where it is true and a row
//! where it is not. A result is always reported with that number.
//!
//! Nothing here touches a migration source. The engines are sandboxes behind [`Sandbox`], and both
//! implementations leave nothing behind: PostgreSQL work happens in a transaction that is rolled
//! back, ClickHouse work in a database that is dropped.
//!
//! What V5 does not decide: row order (results are compared as multisets), and anything about
//! tables whose columns have types the fixture cannot generate — those are `NotRun`, with the
//! reason, never a pass.

use crate::invention::{AtomKind, sql_atoms};
use ekos_migrate_validate::{ColumnRule, Dialect, canon_expr};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{
    BinaryOperator, Expr, Statement, UnaryOperator, Value, visit_expressions, visit_relations,
    visit_relations_mut,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

/// An engine a fixture can be loaded into and thrown away.
pub trait Sandbox {
    fn dialect(&self) -> Dialect;
    /// Which path ran the statements, for the report.
    fn label(&self) -> &str;
    /// Run `setup`, then each of `queries`, and leave nothing behind — also when a statement
    /// fails. One result set per query, rows as column strings, SQL `NULL` as `""`.
    fn run(&self, setup: &[String], queries: &[String]) -> Result<Vec<Vec<Vec<String>>>, String>;
}

/// A table the view reads, as EKOS recovered it from the repository's DDL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSpec {
    pub name: String,
    /// `(column, declared type)`.
    pub columns: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Int,
    Decimal,
    Text,
    Bool,
    Date,
    Timestamp,
}

/// The scale every fixture decimal carries, on both engines.
const DECIMAL_SCALE: u32 = 6;

impl Family {
    fn of(declared: &str) -> Option<Self> {
        let lower = declared.trim().to_ascii_lowercase();
        let base = lower.split('(').next().unwrap_or_default().trim();
        Some(match base {
            "int" | "integer" | "int4" | "bigint" | "int8" | "smallint" | "int2" | "serial"
            | "bigserial" | "smallserial" => Self::Int,
            "numeric" | "decimal" => Self::Decimal,
            "text" | "varchar" | "character varying" => Self::Text,
            "bool" | "boolean" => Self::Bool,
            "date" => Self::Date,
            "timestamp" | "timestamp without time zone" => Self::Timestamp,
            _ => return None,
        })
    }

    /// The fixture column type on each engine. The fixture's types are this module's choice, not
    /// the source's: an `int` and a `bigint` column behave the same over these values.
    fn column_type(self, d: Dialect) -> String {
        match (self, d) {
            (Self::Int, Dialect::Postgres) => "bigint".into(),
            (Self::Int, Dialect::ClickHouse) => "Nullable(Int64)".into(),
            (Self::Decimal, Dialect::Postgres) => format!("numeric(30,{DECIMAL_SCALE})"),
            (Self::Decimal, Dialect::ClickHouse) => {
                format!("Nullable(Decimal(30,{DECIMAL_SCALE}))")
            }
            (Self::Text, Dialect::Postgres) => "text".into(),
            (Self::Text, Dialect::ClickHouse) => "Nullable(String)".into(),
            (Self::Bool, Dialect::Postgres) => "boolean".into(),
            (Self::Bool, Dialect::ClickHouse) => "Nullable(Bool)".into(),
            (Self::Date, Dialect::Postgres) => "date".into(),
            (Self::Date, Dialect::ClickHouse) => "Nullable(Date32)".into(),
            (Self::Timestamp, Dialect::Postgres) => "timestamp".into(),
            (Self::Timestamp, Dialect::ClickHouse) => "Nullable(DateTime64(6))".into(),
        }
    }

    /// Values every column of this family takes, before the view's own literals are added.
    fn base(self) -> Vec<Lit> {
        let text = |s: &str| Lit::Text(s.to_string());
        match self {
            Self::Int => [-1, 0, 1, 2, 3].into_iter().map(Lit::Int).collect(),
            Self::Decimal => ["-1.5", "0", "1.5", "2"]
                .into_iter()
                .map(|s| Lit::Num(s.into()))
                .collect(),
            // One value is not ASCII: a function that counts bytes where the source counted
            // characters only shows on such a value.
            Self::Text => vec![text(""), text("a"), text("b"), text("é")],
            Self::Bool => vec![Lit::Bool(true), Lit::Bool(false)],
            Self::Date => vec![text("2000-01-01"), text("2024-02-29")],
            Self::Timestamp => vec![text("2000-01-01 00:00:00"), text("2024-02-29 12:34:56")],
        }
    }
}

/// A literal, written the same way on both engines.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Lit {
    Null,
    Int(i64),
    Num(String),
    Text(String),
    Bool(bool),
}

impl Lit {
    fn sql(&self) -> String {
        match self {
            Self::Null => "NULL".into(),
            Self::Int(i) => i.to_string(),
            Self::Num(n) => n.clone(),
            Self::Text(s) => format!("'{}'", s.replace('\'', "''")),
            Self::Bool(b) => b.to_string(),
        }
    }

    fn of(e: &Expr) -> Option<Self> {
        match e {
            Expr::Value(Value::Number(n, _)) => Some(match n.parse::<i64>() {
                Ok(i) => Self::Int(i),
                Err(_) => Self::Num(n.clone()),
            }),
            // A backslash is read differently by the two engines inside a string literal.
            Expr::Value(Value::SingleQuotedString(s)) if !s.contains('\\') => {
                Some(Self::Text(s.clone()))
            }
            Expr::Value(Value::Boolean(b)) => Some(Self::Bool(*b)),
            Expr::Nested(inner) => Self::of(inner),
            Expr::UnaryOp {
                op: UnaryOperator::Minus,
                expr,
            } => match Self::of(expr)? {
                Self::Int(i) => Some(Self::Int(-i)),
                Self::Num(n) => Some(Self::Num(format!("-{n}"))),
                _ => None,
            },
            _ => None,
        }
    }

    /// The literal and the values either side of it, as far as `family` can hold them.
    fn boundary(&self, family: Family) -> Vec<Lit> {
        match (self, family) {
            (Self::Int(i), Family::Int | Family::Decimal) => {
                vec![Self::Int(i - 1), Self::Int(*i), Self::Int(i + 1)]
            }
            (Self::Num(n), Family::Decimal) => vec![Self::Num(n.clone())],
            (Self::Text(s), Family::Text) => {
                vec![Self::Text(s.clone()), Self::Text(format!("{s}x"))]
            }
            (Self::Text(s), Family::Date | Family::Timestamp)
                if s.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | ' ')) =>
            {
                vec![Self::Text(s.clone())]
            }
            (Self::Bool(b), Family::Bool) => vec![Self::Bool(*b)],
            _ => Vec::new(),
        }
    }
}

fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn column_of(e: &Expr) -> Option<String> {
    match e {
        Expr::Identifier(i) => Some(i.value.to_lowercase()),
        Expr::CompoundIdentifier(parts) => parts.last().map(|i| i.value.to_lowercase()),
        Expr::Nested(inner) => column_of(inner),
        _ => None,
    }
}

/// A `column <op> literal` the view writes: what the fixture is built around and what branch
/// coverage is measured over.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Comparison {
    column: String,
    /// The predicate over the bare column, in SQL both engines read.
    predicate: String,
    literals: Vec<Lit>,
}

fn comparisons(stmt: &Statement) -> (BTreeSet<Comparison>, usize) {
    let mut out = BTreeSet::new();
    // Comparisons this module cannot aim a fixture at: column against column, or an expression.
    let mut unmeasured = 0usize;
    let _ = visit_expressions(stmt, |e| {
        match e {
            Expr::BinaryOp { left, op, right } => {
                let symbol = match op {
                    BinaryOperator::Eq => "=",
                    BinaryOperator::NotEq => "<>",
                    BinaryOperator::Lt => "<",
                    BinaryOperator::LtEq => "<=",
                    BinaryOperator::Gt => ">",
                    BinaryOperator::GtEq => ">=",
                    _ => return ControlFlow::<()>::Continue(()),
                };
                let sides = [(left, right, false), (right, left, true)];
                let found = sides
                    .iter()
                    .find_map(|(c, l, flipped)| Some((column_of(c)?, Lit::of(l)?, *flipped)));
                match found {
                    Some((column, lit, flipped)) => {
                        let predicate = if flipped {
                            format!("{} {symbol} {}", lit.sql(), ident(&column))
                        } else {
                            format!("{} {symbol} {}", ident(&column), lit.sql())
                        };
                        out.insert(Comparison {
                            column,
                            predicate,
                            literals: vec![lit],
                        });
                    }
                    None => unmeasured += 1,
                }
            }
            Expr::InList {
                expr,
                list,
                negated,
            } => {
                let lits: Option<Vec<Lit>> = list.iter().map(Lit::of).collect();
                match (column_of(expr), lits) {
                    (Some(column), Some(literals)) if !literals.is_empty() => {
                        let items: Vec<String> = literals.iter().map(Lit::sql).collect();
                        out.insert(Comparison {
                            predicate: format!(
                                "{} {}IN ({})",
                                ident(&column),
                                if *negated { "NOT " } else { "" },
                                items.join(", ")
                            ),
                            column,
                            literals,
                        });
                    }
                    _ => unmeasured += 1,
                }
            }
            Expr::Between {
                expr,
                negated,
                low,
                high,
            } => match (column_of(expr), Lit::of(low), Lit::of(high)) {
                (Some(column), Some(lo), Some(hi)) => {
                    out.insert(Comparison {
                        predicate: format!(
                            "{} {}BETWEEN {} AND {}",
                            ident(&column),
                            if *negated { "NOT " } else { "" },
                            lo.sql(),
                            hi.sql()
                        ),
                        column,
                        literals: vec![lo, hi],
                    });
                }
                _ => unmeasured += 1,
            },
            _ => {}
        }
        ControlFlow::<()>::Continue(())
    });
    (out, unmeasured)
}

/// Every select-list alias in a query, at any depth: names a later clause may use as a column.
fn output_aliases(query: &sqlparser::ast::Query, out: &mut BTreeSet<String>) {
    use sqlparser::ast::{SelectItem, SetExpr, TableFactor};
    fn body(e: &SetExpr, out: &mut BTreeSet<String>) {
        match e {
            SetExpr::Select(s) => {
                for item in &s.projection {
                    if let SelectItem::ExprWithAlias { alias, .. } = item {
                        out.insert(alias.value.to_lowercase());
                    }
                }
                for twj in &s.from {
                    let factors =
                        std::iter::once(&twj.relation).chain(twj.joins.iter().map(|j| &j.relation));
                    for f in factors {
                        if let TableFactor::Derived { subquery, .. } = f {
                            output_aliases(subquery, out);
                        }
                    }
                }
            }
            SetExpr::Query(q) => output_aliases(q, out),
            SetExpr::SetOperation { left, right, .. } => {
                body(left, out);
                body(right, out);
            }
            _ => {}
        }
    }
    body(&query.body, out);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureTable {
    name: String,
    columns: Vec<(String, Family)>,
    rows: Vec<Vec<Lit>>,
}

/// Everything needed to run one view on both engines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    view: String,
    /// The source definition with schema qualifiers removed, so it reads the fixture.
    source_view: String,
    lowered_view: String,
    tables: Vec<FixtureTable>,
    comparisons: Vec<Comparison>,
    unmeasured: usize,
}

const MIN_ROWS: usize = 8;
/// The most rows one fixture table holds. Enough to cross three compared columns of eight values.
const MAX_ROWS: usize = 512;

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn not_run(why: impl Into<String>) -> V5Outcome {
    V5Outcome::NotRun { why: why.into() }
}

/// The fixture for one table the view reads: a column per fillable type, and rows that cross the
/// compared columns' values.
fn fixture_table(
    table: &str,
    spec: &TableSpec,
    found: &BTreeSet<Comparison>,
    named: &BTreeSet<String>,
    wildcard: bool,
) -> Result<FixtureTable, V5Outcome> {
    let mut columns = Vec::new();
    for (column, declared) in &spec.columns {
        let column = column.to_lowercase();
        match Family::of(declared) {
            Some(family) => columns.push((column, family)),
            None if wildcard || named.contains(&column) => {
                return Err(not_run(format!(
                    "{table}.{column} is {declared}: the fixture has no values for that type"
                )));
            }
            None => {}
        }
    }
    if columns.is_empty() {
        return Err(not_run(format!(
            "{table} has no column the fixture can fill"
        )));
    }
    let pools: Vec<Vec<Lit>> = columns
        .iter()
        .map(|(column, family)| {
            let mut pool: BTreeSet<Lit> = family.base().into_iter().collect();
            for c in found.iter().filter(|c| c.column == *column) {
                for lit in &c.literals {
                    pool.extend(lit.boundary(*family));
                }
            }
            // NULL first: every column takes it, whatever else the pool holds.
            std::iter::once(Lit::Null).chain(pool).collect()
        })
        .collect();
    // Columns the view compares against a literal are **crossed**: every combination of their
    // values appears. Taking each comparison both ways is not enough when two of them are
    // joined by AND — a boundary row that the other filter always removes shows nothing. The
    // remaining columns walk their pools at their own stride, coprime with the pool's length
    // so every value appears.
    let compared: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, (c, _))| found.iter().any(|f| f.column == *c))
        .map(|(j, _)| j)
        .collect();
    let product: usize = compared
        .iter()
        .map(|j| pools[*j].len())
        .try_fold(1usize, |acc, n| {
            acc.checked_mul(n).filter(|p| *p <= MAX_ROWS)
        })
        .unwrap_or(0);
    let crossed: &[usize] = if product > 0 { &compared } else { &[] };
    let longest = pools.iter().map(Vec::len).max().unwrap_or(1);
    let n = product
        .max((longest * 3).clamp(MIN_ROWS, MAX_ROWS))
        .min(MAX_ROWS);
    let strides: Vec<usize> = pools
        .iter()
        .enumerate()
        .map(|(j, pool)| {
            (2 * j + 1..)
                .find(|s| gcd(*s, pool.len()) == 1)
                .unwrap_or(1)
        })
        .collect();
    let rows = (0..n)
        .map(|i| {
            let mut radix = i;
            pools
                .iter()
                .zip(&strides)
                .enumerate()
                .map(|(j, (pool, stride))| {
                    if crossed.contains(&j) {
                        let digit = radix % pool.len();
                        radix /= pool.len();
                        pool[digit].clone()
                    } else {
                        pool[(i * stride + j) % pool.len()].clone()
                    }
                })
                .collect()
        })
        .collect();
    Ok(FixtureTable {
        name: table.to_string(),
        columns,
        rows,
    })
}

/// Build the fixture and both view statements, or say why V5 cannot run on this view.
pub fn plan(
    source_definition: &str,
    lowered_sql: &str,
    tables: &[TableSpec],
) -> Result<Plan, V5Outcome> {
    let mut stmts = Parser::parse_sql(&PostgreSqlDialect {}, source_definition)
        .map_err(|e| not_run(format!("the source does not parse: {e}")))?;
    let [stmt] = stmts.as_mut_slice() else {
        return Err(not_run("the source is not one statement"));
    };
    let Statement::CreateView { name, query, .. } = &*stmt else {
        return Err(not_run("the source is not a view"));
    };
    let view = name
        .0
        .last()
        .map(|i| i.value.to_lowercase())
        .unwrap_or_default();
    if query.limit.is_some() || query.fetch.is_some() {
        return Err(not_run(
            "the view has a LIMIT: which rows it keeps depends on an order the engines need not share",
        ));
    }

    let mut read: BTreeSet<String> = BTreeSet::new();
    let _ = visit_relations(&*stmt, |r| {
        if let Some(last) = r.0.last() {
            read.insert(last.value.to_lowercase());
        }
        ControlFlow::<()>::Continue(())
    });
    read.remove(&view);
    let (found, unmeasured) = comparisons(stmt);
    let named: BTreeSet<String> = sql_atoms(source_definition)
        .map_err(|e| not_run(format!("the source does not parse: {e}")))?
        .into_iter()
        .filter(|a| a.kind == AtomKind::Column)
        .map(|a| a.text)
        .collect();
    let wildcard = lowered_sql.replace("count(*)", "").contains('*');

    let mut fixture = Vec::new();
    for table in &read {
        let Some(spec) = tables
            .iter()
            .find(|t| t.name.rsplit('.').next().unwrap_or(&t.name).to_lowercase() == *table)
        else {
            return Err(not_run(format!(
                "it reads {table}, which is not a table EKOS recovered from the repository (another \
                 view, or a relation it has no columns for)"
            )));
        };
        fixture.push(fixture_table(table, spec, &found, &named, wildcard)?);
    }
    if fixture.is_empty() {
        return Err(not_run("the view reads no table"));
    }
    // A column the view names and no fixture table has would fail on the source engine as
    // "column does not exist" — which says nothing about the lowering. It usually means the
    // repository DDL EKOS compiled predates the column (added later by an `ALTER TABLE`).
    let mut known: BTreeSet<String> = fixture
        .iter()
        .flat_map(|t| t.columns.iter().map(|(c, _)| c.clone()))
        .collect();
    if let Statement::CreateView { query, .. } = &*stmt {
        output_aliases(query, &mut known);
    }
    if let Some(missing) = named.iter().find(|c| !known.contains(*c)) {
        return Err(not_run(format!(
            "it names the column {missing}, which none of the recovered tables it reads has — \
             the repository DDL EKOS compiled may predate it"
        )));
    }

    // The source runs against the fixture, so a schema qualifier has to go: `public.t` would read
    // the sandbox's own `public`, not the fixture.
    let mut qualified_column = false;
    let _ = visit_expressions(&*stmt, |e| {
        if matches!(e, Expr::CompoundIdentifier(parts) if parts.len() > 2) {
            qualified_column = true;
        }
        ControlFlow::<()>::Continue(())
    });
    if qualified_column {
        return Err(not_run("a column is written schema.table.column"));
    }
    let _ = visit_relations_mut(stmt, |r| {
        if r.0.len() > 1 {
            r.0.drain(..r.0.len() - 1);
        }
        ControlFlow::<()>::Continue(())
    });
    if let Statement::CreateView {
        name, or_replace, ..
    } = stmt
    {
        name.0.drain(..name.0.len() - 1);
        *or_replace = false;
    }
    Ok(Plan {
        view,
        source_view: stmt.to_string(),
        lowered_view: lowered_sql.to_string(),
        tables: fixture,
        comparisons: found.into_iter().collect(),
        unmeasured,
    })
}

impl Plan {
    pub fn fixture_rows(&self) -> usize {
        self.tables.iter().map(|t| t.rows.len()).sum()
    }

    /// The statements that load the fixture and create the view on one engine.
    pub fn setup(&self, d: Dialect) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.tables {
            let columns: Vec<String> = t
                .columns
                .iter()
                .map(|(c, f)| format!("{} {}", ident(c), f.column_type(d)))
                .collect();
            let engine = match d {
                Dialect::Postgres => "",
                Dialect::ClickHouse => " ENGINE = Memory",
            };
            out.push(format!(
                "CREATE TABLE {} ({}){engine}",
                ident(&t.name),
                columns.join(", ")
            ));
            let rows: Vec<String> = t
                .rows
                .iter()
                .map(|r| {
                    format!(
                        "({})",
                        r.iter().map(Lit::sql).collect::<Vec<_>>().join(", ")
                    )
                })
                .collect();
            out.push(format!(
                "INSERT INTO {} VALUES {}",
                ident(&t.name),
                rows.join(", ")
            ));
        }
        out.push(match d {
            Dialect::Postgres => self.source_view.clone(),
            Dialect::ClickHouse => self.lowered_view.clone(),
        });
        out
    }

    fn columns_query(&self) -> String {
        format!(
            "SELECT column_name, data_type, coalesce(numeric_scale::text, '') \
             FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = '{}' \
             ORDER BY ordinal_position",
            self.view.replace('\'', "''")
        )
    }

    fn result_query(&self, d: Dialect, columns: &[(String, ColumnRule)]) -> String {
        let exprs: Vec<String> = columns
            .iter()
            .map(|(c, rule)| canon_expr(d, c, *rule))
            .collect();
        format!("SELECT {} FROM {}", exprs.join(", "), ident(&self.view))
    }

    /// One query per comparison and table holding its column: rows where it is true, rows where
    /// it is not.
    fn coverage_queries(&self) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        for (i, c) in self.comparisons.iter().enumerate() {
            for t in self
                .tables
                .iter()
                .filter(|t| t.columns.iter().any(|(n, _)| *n == c.column))
            {
                out.push((
                    i,
                    format!(
                        "SELECT count(*) FILTER (WHERE ({p}) IS TRUE), \
                         count(*) FILTER (WHERE ({p}) IS NOT TRUE) FROM {}",
                        ident(&t.name),
                        p = c.predicate
                    ),
                ));
            }
        }
        out
    }
}

/// The rule a view's output column is rendered by, from what PostgreSQL says its type is.
fn rule_for(data_type: &str, scale: &str) -> Option<ColumnRule> {
    Some(match data_type {
        "bigint" | "integer" | "smallint" => ColumnRule::Int,
        "boolean" => ColumnRule::Bool,
        "text" | "character varying" => ColumnRule::Text,
        "numeric" => ColumnRule::Decimal(scale.parse().unwrap_or(DECIMAL_SCALE)),
        "date" => ColumnRule::Date,
        "timestamp without time zone" => ColumnRule::TimestampNaive,
        _ => return None,
    })
}

/// How much of the view's branching the fixture exercised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// `column <op> literal` comparisons the fixture was aimed at.
    pub measured: usize,
    /// Of those, the ones true on some row and not true on another.
    pub covered: usize,
    /// Comparisons between two columns or over an expression: exercised or not, not measured.
    pub unmeasured: usize,
    /// The measured comparisons the fixture did not cover.
    pub uncovered: Vec<String>,
}

impl Coverage {
    pub fn describe(&self) -> String {
        let pct = if self.measured == 0 {
            "no column-against-literal comparisons to measure".to_string()
        } else {
            format!(
                "{}/{} comparisons taken both ways ({:.0}%)",
                self.covered,
                self.measured,
                100.0 * self.covered as f64 / self.measured as f64
            )
        };
        if self.unmeasured == 0 {
            pct
        } else {
            format!("{pct}; {} more not measured", self.unmeasured)
        }
    }
}

/// A row the two engines returned a different number of times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divergent {
    pub row: Vec<String>,
    pub source: usize,
    pub target: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct V5Report {
    pub passed: bool,
    pub source_engine: String,
    pub target_engine: String,
    pub fixture_rows: usize,
    pub source_rows: usize,
    pub target_rows: usize,
    /// Rows whose counts differ, how many there are in all, and the first few.
    pub divergent_rows: usize,
    pub divergent: Vec<Divergent>,
    pub coverage: Coverage,
}

impl V5Report {
    /// The one-line verdict. Coverage is always part of it: "V5 passed" alone is not a result.
    pub fn verdict(&self) -> String {
        format!(
            "V5 {}: {} source row(s), {} target row(s), {} differing, over {} fixture row(s); {}",
            if self.passed { "passed" } else { "FAILED" },
            self.source_rows,
            self.target_rows,
            self.divergent_rows,
            self.fixture_rows,
            self.coverage.describe()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "v5")]
pub enum V5Outcome {
    Ran(Box<V5Report>),
    /// V5 could not be attempted. Never a pass.
    NotRun {
        why: String,
    },
    /// An engine refused a statement. Never a pass.
    Error {
        engine: String,
        message: String,
    },
}

const SHOWN: usize = 5;

/// Run one lowered view against its source.
pub fn run(plan: &Plan, source: &dyn Sandbox, target: &dyn Sandbox) -> V5Outcome {
    let error = |s: &dyn Sandbox, message: String| V5Outcome::Error {
        engine: s.label().to_string(),
        message,
    };
    let source_setup = plan.setup(source.dialect());
    // What the view returns is asked of the source engine, not assumed.
    let (encoding, described) = match source.run(
        &source_setup,
        &[
            "SELECT current_setting('server_encoding')".to_string(),
            plan.columns_query(),
        ],
    ) {
        Ok(mut sets) => {
            let described = sets.pop().unwrap_or_default();
            let encoding = sets
                .pop()
                .and_then(|s| s.into_iter().next())
                .and_then(|r| r.into_iter().next())
                .unwrap_or_default();
            (encoding, described)
        }
        Err(e) => return error(source, e),
    };
    // In an `SQL_ASCII` database PostgreSQL counts and compares bytes, so `length('é')` is 2 and
    // a correct lowering looks wrong. Found by the live suite on a `LANG=C` sandbox.
    if !encoding.eq_ignore_ascii_case("UTF8") {
        return not_run(format!(
            "the PostgreSQL sandbox database is encoded {encoding}, not UTF8: text functions \
             behave differently there than on a real source"
        ));
    }
    let mut columns = Vec::new();
    for row in &described {
        let (name, data_type) = (&row[0], &row[1]);
        match rule_for(
            data_type,
            row.get(2).map(String::as_str).unwrap_or_default(),
        ) {
            Some(rule) => columns.push((name.clone(), rule)),
            None => {
                return not_run(format!(
                    "output column {name} is {data_type}: there is no canonical rendering for it yet"
                ));
            }
        }
    }
    if columns.is_empty() {
        return error(source, "the view has no columns in the sandbox".into());
    }

    let coverage_queries = plan.coverage_queries();
    let mut queries = vec![plan.result_query(source.dialect(), &columns)];
    queries.extend(coverage_queries.iter().map(|(_, q)| q.clone()));
    let mut sets = match source.run(&source_setup, &queries) {
        Ok(sets) => sets,
        Err(e) => return error(source, e),
    };
    let coverage_sets = sets.split_off(1);
    let source_rows = sets.pop().unwrap_or_default();
    let target_rows = match target.run(
        &plan.setup(target.dialect()),
        &[plan.result_query(target.dialect(), &columns)],
    ) {
        Ok(mut sets) => sets.pop().unwrap_or_default(),
        Err(e) => return error(target, e),
    };

    let mut covered = vec![false; plan.comparisons.len()];
    for ((i, _), set) in coverage_queries.iter().zip(&coverage_sets) {
        let count = |k: usize| {
            set.first()
                .and_then(|r| r.get(k))
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0)
        };
        if count(0) > 0 && count(1) > 0 {
            covered[*i] = true;
        }
    }
    let coverage = Coverage {
        measured: plan.comparisons.len(),
        covered: covered.iter().filter(|c| **c).count(),
        unmeasured: plan.unmeasured,
        uncovered: plan
            .comparisons
            .iter()
            .zip(&covered)
            .filter(|(_, c)| !**c)
            .map(|(c, _)| c.predicate.clone())
            .collect(),
    };

    let mut counts: BTreeMap<&Vec<String>, (usize, usize)> = BTreeMap::new();
    for r in &source_rows {
        counts.entry(r).or_default().0 += 1;
    }
    for r in &target_rows {
        counts.entry(r).or_default().1 += 1;
    }
    let divergent: Vec<Divergent> = counts
        .into_iter()
        .filter(|(_, (s, t))| s != t)
        .map(|(row, (source, target))| Divergent {
            row: row.clone(),
            source,
            target,
        })
        .collect();
    V5Outcome::Ran(Box::new(V5Report {
        passed: divergent.is_empty(),
        source_engine: source.label().to_string(),
        target_engine: target.label().to_string(),
        fixture_rows: plan.fixture_rows(),
        source_rows: source_rows.len(),
        target_rows: target_rows.len(),
        divergent_rows: divergent.len(),
        divergent: divergent.into_iter().take(SHOWN).collect(),
        coverage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::{LowerOptions, Lowering, lower_view};
    use std::cell::RefCell;

    const SOURCE: &str = "CREATE VIEW overdue AS SELECT i.id, i.status FROM public.invoice i \
        LEFT JOIN line l ON l.invoice_id = i.id WHERE i.due < 30 AND i.status IN ('open', 'sent')";

    fn tables() -> Vec<TableSpec> {
        let t = |name: &str, cols: &[(&str, &str)]| TableSpec {
            name: name.into(),
            columns: cols
                .iter()
                .map(|(c, t)| (c.to_string(), t.to_string()))
                .collect(),
        };
        vec![
            t(
                "invoice",
                &[
                    ("id", "int"),
                    ("due", "integer"),
                    ("status", "text"),
                    ("blob", "bytea"),
                ],
            ),
            t(
                "public.line",
                &[("invoice_id", "int"), ("amount", "numeric")],
            ),
        ]
    }

    fn lowered(source: &str) -> String {
        match lower_view(source, &LowerOptions::default()) {
            Lowering::Lowered(l) => l.sql,
            Lowering::Blocked { blockers } => panic!("{blockers:?}"),
        }
    }

    fn planned() -> Plan {
        plan(SOURCE, &lowered(SOURCE), &tables()).unwrap()
    }

    #[test]
    fn the_fixture_is_aimed_at_the_views_own_comparisons() {
        let p = planned();
        let invoice = p.tables.iter().find(|t| t.name == "invoice").unwrap();
        // The unsupported column the view never names is left out.
        assert_eq!(
            invoice
                .columns
                .iter()
                .map(|(c, _)| c.as_str())
                .collect::<Vec<_>>(),
            ["id", "due", "status"]
        );
        let values =
            |col: usize| -> BTreeSet<Lit> { invoice.rows.iter().map(|r| r[col].clone()).collect() };
        // Either side of `due < 30`, and NULL.
        for want in [Lit::Int(29), Lit::Int(30), Lit::Int(31), Lit::Null] {
            assert!(values(1).contains(&want), "due lacks {want:?}");
        }
        // Both listed statuses and one that is neither.
        for want in ["open", "sent", "openx"] {
            assert!(
                values(2).contains(&Lit::Text(want.into())),
                "status lacks {want}"
            );
        }
        // The two compared columns are crossed: the boundary row passes the other filter too.
        assert!(
            invoice
                .rows
                .iter()
                .any(|r| r[1] == Lit::Int(30) && r[2] == Lit::Text("open".into()))
        );
        assert!(
            invoice
                .rows
                .iter()
                .any(|r| r[1] == Lit::Int(29) && r[2] == Lit::Text("sentx".into()))
        );
        assert_eq!(p.comparisons.len(), 2);
        // The join condition compares two columns: counted, not measured.
        assert_eq!(p.unmeasured, 1);
        assert_eq!(p, planned(), "the plan must be deterministic");
    }

    #[test]
    fn each_engine_gets_its_own_types_and_its_own_view() {
        let p = planned();
        let pg = p.setup(Dialect::Postgres);
        let ch = p.setup(Dialect::ClickHouse);
        assert!(pg[0].starts_with(
            "CREATE TABLE \"invoice\" (\"id\" bigint, \"due\" bigint, \"status\" text)"
        ));
        assert!(ch[0].contains("\"due\" Nullable(Int64)") && ch[0].ends_with("ENGINE = Memory"));
        // The same rows, written the same way.
        assert_eq!(pg[1], ch[1]);
        // The source view lost its schema qualifier; the target gets the lowering untouched.
        let source_view = pg.last().unwrap();
        assert!(
            source_view.contains("FROM invoice AS i") && !source_view.contains("public."),
            "{source_view}"
        );
        assert_eq!(ch.last().unwrap(), &lowered(SOURCE));
    }

    #[test]
    fn what_cannot_be_run_is_not_run_and_says_why() {
        let why = |source: &str, tables: &[TableSpec]| match plan(source, &lowered(source), tables)
        {
            Err(V5Outcome::NotRun { why }) => why,
            other => panic!("{other:?}"),
        };
        assert!(
            why("CREATE VIEW v AS SELECT id FROM other_view", &tables())
                .contains("not a table EKOS recovered")
        );
        assert!(why("CREATE VIEW v AS SELECT blob FROM invoice", &tables()).contains("bytea"));
        assert!(
            why(
                "CREATE VIEW v AS SELECT id FROM invoice WHERE reversing = 1",
                &tables()
            )
            .contains("names the column reversing")
        );
        // An alias used further out is not a missing column.
        assert!(
            plan(
                "CREATE VIEW v AS SELECT s.n FROM (SELECT count(id) AS n FROM invoice) s WHERE s.n > 0",
                &lowered("CREATE VIEW v AS SELECT s.n FROM (SELECT count(id) AS n FROM invoice) s WHERE s.n > 0"),
                &tables()
            )
            .is_ok()
        );
        assert!(why("CREATE VIEW v AS SELECT * FROM invoice", &tables()).contains("bytea"));
        assert!(
            why(
                "CREATE VIEW v AS SELECT id FROM invoice ORDER BY id LIMIT 3",
                &tables()
            )
            .contains("LIMIT")
        );
    }

    /// Answers by statement shape: the column list, the view's rows, and coverage counts.
    struct Fake {
        dialect: Dialect,
        rows: Vec<Vec<String>>,
        coverage: (u64, u64),
        fail: Option<String>,
        encoding: String,
        seen: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new(dialect: Dialect, rows: &[&[&str]]) -> Self {
            Self {
                dialect,
                rows: rows
                    .iter()
                    .map(|r| r.iter().map(std::string::ToString::to_string).collect())
                    .collect(),
                coverage: (3, 4),
                fail: None,
                encoding: "UTF8".into(),
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    impl Sandbox for Fake {
        fn dialect(&self) -> Dialect {
            self.dialect
        }
        fn label(&self) -> &str {
            match self.dialect {
                Dialect::Postgres => "fake:postgres",
                Dialect::ClickHouse => "fake:clickhouse",
            }
        }
        fn run(
            &self,
            setup: &[String],
            queries: &[String],
        ) -> Result<Vec<Vec<Vec<String>>>, String> {
            if let Some(e) = &self.fail {
                return Err(e.clone());
            }
            self.seen.borrow_mut().extend(setup.iter().cloned());
            Ok(queries
                .iter()
                .map(|q| {
                    if q.contains("server_encoding") {
                        vec![vec![self.encoding.clone()]]
                    } else if q.contains("information_schema.columns") {
                        vec![
                            vec!["id".into(), "integer".into(), "".into()],
                            vec!["status".into(), "text".into(), "".into()],
                        ]
                    } else if q.contains("FILTER") {
                        vec![vec![
                            self.coverage.0.to_string(),
                            self.coverage.1.to_string(),
                        ]]
                    } else {
                        self.rows.clone()
                    }
                })
                .collect())
        }
    }

    #[test]
    fn equal_multisets_pass_and_the_verdict_carries_coverage() {
        let rows: &[&[&str]] = &[&["1", "open"], &["1", "open"], &["2", "sent"]];
        let swapped: &[&[&str]] = &[&["2", "sent"], &["1", "open"], &["1", "open"]];
        let (pg, ch) = (
            Fake::new(Dialect::Postgres, rows),
            Fake::new(Dialect::ClickHouse, swapped),
        );
        let V5Outcome::Ran(report) = run(&planned(), &pg, &ch) else {
            panic!("not run");
        };
        assert!(report.passed, "{report:#?}");
        assert_eq!(report.coverage.covered, 2);
        assert_eq!(
            report.verdict(),
            format!(
                "V5 passed: 3 source row(s), 3 target row(s), 0 differing, over {} fixture row(s); \
                 2/2 comparisons taken both ways (100%); 1 more not measured",
                report.fixture_rows
            )
        );
        // Each engine was given its own setup.
        assert!(pg.seen.borrow().iter().any(|s| s.contains("bigint")));
        assert!(
            ch.seen
                .borrow()
                .iter()
                .any(|s| s.contains("Nullable(Int64)"))
        );
    }

    #[test]
    fn a_missing_extra_or_changed_row_fails() {
        let source: &[&[&str]] = &[&["1", "open"], &["1", "open"], &["2", "sent"]];
        for target in [
            &[&["1", "open"][..], &["2", "sent"][..]][..], // a duplicate lost
            &[
                &["1", "open"][..],
                &["1", "open"][..],
                &["2", "sent"][..],
                &["3", "open"][..],
            ][..], // one extra
            &[&["1", "open"][..], &["1", "open"][..], &["2", "\\N"][..]][..], // a value became NULL
        ] {
            let outcome = run(
                &planned(),
                &Fake::new(Dialect::Postgres, source),
                &Fake::new(Dialect::ClickHouse, target),
            );
            let V5Outcome::Ran(report) = outcome else {
                panic!("not run");
            };
            assert!(!report.passed);
            assert!(report.divergent_rows >= 1);
            assert!(report.verdict().starts_with("V5 FAILED"));
        }
    }

    #[test]
    fn an_uncovered_comparison_is_named_and_an_engine_error_is_not_a_pass() {
        let rows: &[&[&str]] = &[&["1", "open"]];
        let mut pg = Fake::new(Dialect::Postgres, rows);
        pg.coverage = (5, 0);
        let V5Outcome::Ran(report) = run(&planned(), &pg, &Fake::new(Dialect::ClickHouse, rows))
        else {
            panic!("not run");
        };
        assert_eq!(report.coverage.covered, 0);
        assert_eq!(report.coverage.uncovered.len(), 2);
        assert!(
            report
                .verdict()
                .contains("0/2 comparisons taken both ways (0%)")
        );

        let mut ascii = Fake::new(Dialect::Postgres, rows);
        ascii.encoding = "SQL_ASCII".into();
        assert!(matches!(
            run(&planned(), &ascii, &Fake::new(Dialect::ClickHouse, rows)),
            V5Outcome::NotRun { why } if why.contains("SQL_ASCII")
        ));

        let mut ch = Fake::new(Dialect::ClickHouse, rows);
        ch.fail = Some("Unknown function sumOrNul".into());
        assert_eq!(
            run(&planned(), &Fake::new(Dialect::Postgres, rows), &ch),
            V5Outcome::Error {
                engine: "fake:clickhouse".into(),
                message: "Unknown function sumOrNul".into()
            }
        );
    }
}

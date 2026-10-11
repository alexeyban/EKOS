//! The anti-invention check: a candidate statement may state nothing its source does not.
//!
//! Both sides are reduced to **atoms** — the tables read, the columns named, the literals written,
//! the functions called and the comparisons made — and every atom of the candidate must be one of
//! the source's. An atom that is not is *unmapped*, and a candidate with one is rejected, not
//! flagged for review (RFC 0164).
//!
//! The check is deliberately one-directional. It does not ask whether the candidate kept
//! everything — a dropped branch is found by running both (V5). It asks whether the candidate
//! added a predicate, a constant or a table nobody wrote, which is what a model does when it fills
//! a gap.
//!
//! What it normalizes away is only what [`crate::lower`] itself introduces: quoting and case of
//! names, table qualifiers, `sumOrNull` for `sum`, `concat` for `||`, the written direction of a
//! comparison. An inverted comparison is a different predicate.

use ekos_plpgsql::{LoopKind, ProcStmt, ProcedureIr};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{BinaryOperator, Expr, Statement, Value, visit_expressions, visit_relations};
use sqlparser::dialect::{ClickHouseDialect, Dialect, PostgreSqlDialect};
use sqlparser::parser::Parser;
use std::collections::BTreeSet;
use std::ops::ControlFlow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomKind {
    Table,
    Column,
    Literal,
    Function,
    Predicate,
}

/// One thing a statement states, in a form that compares across the two dialects.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Atom {
    pub kind: AtomKind,
    pub text: String,
}

impl Atom {
    fn new(kind: AtomKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// Target spellings [`crate::lower`] writes, back to the source function they stand for.
fn source_function(name: &str) -> String {
    match name {
        "sumOrNull" => "sum".into(),
        "minOrNull" => "min".into(),
        "maxOrNull" => "max".into(),
        "char_length" => "length".into(),
        other => other.to_lowercase(),
    }
}

fn number(n: &str) -> String {
    // `1.50` and `1.5` are one constant; `01` and `1` are too.
    match n.parse::<f64>() {
        Ok(f) if f.is_finite() => f.to_string(),
        _ => n.to_string(),
    }
}

/// An operand of a comparison, reduced to what it names. Anything composite is `…`: the
/// comparison is then pinned by its operator and its other side, and the composite's own atoms
/// are collected separately.
fn operand(e: &Expr) -> String {
    match e {
        Expr::Identifier(i) => i.value.to_lowercase(),
        Expr::CompoundIdentifier(parts) => parts
            .last()
            .map(|i| i.value.to_lowercase())
            .unwrap_or_default(),
        Expr::Value(Value::Number(n, _)) => number(n),
        Expr::Value(Value::SingleQuotedString(s)) => format!("'{s}'"),
        Expr::Value(Value::Boolean(b)) => b.to_string(),
        Expr::Value(Value::Null) => "null".into(),
        Expr::Nested(inner) => operand(inner),
        Expr::UnaryOp { op, expr } => format!("{op}{}", operand(expr)),
        _ => "…".into(),
    }
}

/// `a < b` and `b > a` are the same comparison; `a < b` and `a > b` are not.
fn comparison(left: &Expr, op: &BinaryOperator, right: &Expr) -> Option<String> {
    let (l, r) = (operand(left), operand(right));
    let ordered = |symbol: &str, l: String, r: String| format!("{l} {symbol} {r}");
    Some(match op {
        BinaryOperator::Eq | BinaryOperator::NotEq => {
            let symbol = if *op == BinaryOperator::Eq { "=" } else { "<>" };
            let (a, b) = if l <= r { (l, r) } else { (r, l) };
            ordered(symbol, a, b)
        }
        BinaryOperator::Lt => ordered("<", l, r),
        BinaryOperator::LtEq => ordered("<=", l, r),
        BinaryOperator::Gt => ordered("<", r, l),
        BinaryOperator::GtEq => ordered("<=", r, l),
        _ => return None,
    })
}

fn expr_atoms(e: &Expr, out: &mut BTreeSet<Atom>) {
    match e {
        Expr::Identifier(i) => {
            out.insert(Atom::new(AtomKind::Column, i.value.to_lowercase()));
        }
        Expr::CompoundIdentifier(parts) => {
            if let Some(last) = parts.last() {
                out.insert(Atom::new(AtomKind::Column, last.value.to_lowercase()));
            }
        }
        Expr::Value(Value::Number(n, _)) => {
            out.insert(Atom::new(AtomKind::Literal, number(n)));
        }
        Expr::Value(
            Value::SingleQuotedString(s)
            | Value::DollarQuotedString(sqlparser::ast::DollarQuotedString { value: s, .. })
            | Value::EscapedStringLiteral(s)
            | Value::NationalStringLiteral(s),
        ) => {
            out.insert(Atom::new(AtomKind::Literal, format!("'{s}'")));
        }
        Expr::Function(f) => {
            out.insert(Atom::new(
                AtomKind::Function,
                source_function(&f.name.to_string()),
            ));
        }
        Expr::BinaryOp { left, op, right } => {
            if *op == BinaryOperator::StringConcat {
                out.insert(Atom::new(AtomKind::Function, "concat"));
            }
            if let Some(text) = comparison(left, op, right) {
                out.insert(Atom::new(AtomKind::Predicate, text));
            }
        }
        Expr::IsNull(inner) => {
            out.insert(Atom::new(
                AtomKind::Predicate,
                format!("{} is null", operand(inner)),
            ));
        }
        Expr::IsNotNull(inner) => {
            out.insert(Atom::new(
                AtomKind::Predicate,
                format!("{} is not null", operand(inner)),
            ));
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let mut items: Vec<String> = list.iter().map(operand).collect();
            items.sort();
            out.insert(Atom::new(
                AtomKind::Predicate,
                format!(
                    "{} {}in ({})",
                    operand(expr),
                    if *negated { "not " } else { "" },
                    items.join(", ")
                ),
            ));
        }
        Expr::Between {
            expr,
            negated,
            low,
            high,
        } => {
            out.insert(Atom::new(
                AtomKind::Predicate,
                format!(
                    "{} {}between {} and {}",
                    operand(expr),
                    if *negated { "not " } else { "" },
                    operand(low),
                    operand(high)
                ),
            ));
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
            let word = if matches!(e, Expr::ILike { .. }) {
                "ilike"
            } else {
                "like"
            };
            out.insert(Atom::new(
                AtomKind::Predicate,
                format!(
                    "{} {}{word} {}",
                    operand(expr),
                    if *negated { "not " } else { "" },
                    operand(pattern)
                ),
            ));
        }
        _ => {}
    }
}

fn statement_atoms(stmts: &[Statement], out: &mut BTreeSet<Atom>) {
    let _ = visit_relations(&stmts.to_vec(), |name| {
        if let Some(last) = name.0.last() {
            out.insert(Atom::new(AtomKind::Table, last.value.to_lowercase()));
        }
        ControlFlow::<()>::Continue(())
    });
    let _ = visit_expressions(&stmts.to_vec(), |e| {
        expr_atoms(e, out);
        ControlFlow::<()>::Continue(())
    });
}

fn atoms_with(dialect: &dyn Dialect, sql: &str) -> Result<BTreeSet<Atom>, String> {
    let stmts = Parser::parse_sql(dialect, sql).map_err(|e| e.to_string())?;
    let mut out = BTreeSet::new();
    statement_atoms(&stmts, &mut out);
    Ok(out)
}

/// The atoms of PostgreSQL source text — a view definition, or any statement.
pub fn sql_atoms(sql: &str) -> Result<BTreeSet<Atom>, String> {
    atoms_with(&PostgreSqlDialect {}, sql)
}

/// What a routine's recovered body states, and how much of it could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutineAtoms {
    pub atoms: BTreeSet<Atom>,
    /// Statements and expressions that did not parse. They contribute nothing, so a candidate that
    /// restates one of them is rejected — the safe direction.
    pub unparsed: usize,
}

/// The atoms of every statement and condition in a routine's procedural IR.
pub fn routine_atoms(ir: &ProcedureIr) -> RoutineAtoms {
    let mut found = RoutineAtoms::default();
    let dialect = PostgreSqlDialect {};
    let statement = |sql: &str, found: &mut RoutineAtoms| match Parser::parse_sql(&dialect, sql) {
        Ok(stmts) => statement_atoms(&stmts, &mut found.atoms),
        Err(_) => found.unparsed += 1,
    };
    fn expression(text: &str, found: &mut RoutineAtoms) {
        let parsed = Parser::new(&PostgreSqlDialect {})
            .try_with_sql(text)
            .and_then(|mut p| p.parse_expr());
        match parsed {
            Ok(e) => {
                let _ = visit_expressions(&e, |x| {
                    expr_atoms(x, &mut found.atoms);
                    ControlFlow::<()>::Continue(())
                });
            }
            Err(_) => found.unparsed += 1,
        }
    }
    for d in &ir.declarations {
        if let Some(default) = &d.default {
            expression(default, &mut found);
        }
    }
    for s in &ir.body {
        s.walk(&mut |node| match node {
            ProcStmt::Sql { sql, .. } | ProcStmt::Perform { sql, .. } => {
                // `PERFORM f(x)` is `SELECT f(x)` with the result discarded.
                if matches!(node, ProcStmt::Perform { .. })
                    && !sql.trim_start().to_ascii_lowercase().starts_with("select")
                {
                    statement(&format!("SELECT {sql}"), &mut found);
                } else {
                    statement(sql, &mut found);
                }
            }
            ProcStmt::Assign { expr, .. } => expression(expr, &mut found),
            ProcStmt::If { branches, .. } => {
                for (condition, _) in branches {
                    expression(condition, &mut found);
                }
            }
            ProcStmt::Case {
                operand, branches, ..
            } => {
                if let Some(o) = operand {
                    expression(o, &mut found);
                }
                for (when, _) in branches {
                    expression(when, &mut found);
                }
            }
            ProcStmt::Loop { kind, .. } => match kind {
                LoopKind::While { condition } => expression(condition, &mut found),
                LoopKind::ForRange { from, to, .. } => {
                    expression(from, &mut found);
                    expression(to, &mut found);
                }
                LoopKind::ForQuery { sql, .. } => statement(sql, &mut found),
                LoopKind::ForEach { array, .. } => expression(array, &mut found),
                LoopKind::Plain => {}
            },
            ProcStmt::Exit { when, .. } => {
                if let Some(w) = when {
                    expression(w, &mut found);
                }
            }
            ProcStmt::Return { value, query, .. } => {
                if let Some(v) = value {
                    expression(v, &mut found);
                }
                if let Some(q) = query {
                    statement(q, &mut found);
                }
            }
            ProcStmt::Cursor { query, .. } => {
                if let Some(q) = query {
                    statement(q, &mut found);
                }
            }
            ProcStmt::Raise { .. }
            | ProcStmt::Block { .. }
            | ProcStmt::DynamicExecute { .. }
            | ProcStmt::Unrecovered { .. } => {}
        });
    }
    found
}

/// The verdict on one candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventionReport {
    /// Atoms the candidate states and the source does not, sorted.
    pub unmapped: Vec<Atom>,
    /// Set when the candidate could not be parsed: nothing could be checked, so it is rejected.
    pub parse_error: Option<String>,
    /// Atoms the candidate states in all.
    pub checked: usize,
}

impl InventionReport {
    /// A candidate is accepted only when it parsed and every atom mapped.
    pub fn accepted(&self) -> bool {
        self.parse_error.is_none() && self.unmapped.is_empty()
    }
}

/// Check a ClickHouse candidate against its source's atoms.
pub fn check_candidate(source: &BTreeSet<Atom>, candidate_sql: &str) -> InventionReport {
    match atoms_with(&ClickHouseDialect {}, candidate_sql) {
        Err(e) => InventionReport {
            unmapped: Vec::new(),
            parse_error: Some(e),
            checked: 0,
        },
        Ok(atoms) => InventionReport {
            checked: atoms.len(),
            unmapped: atoms.into_iter().filter(|a| !source.contains(a)).collect(),
            parse_error: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::{LowerOptions, Lowering, lower_view};

    const SOURCE: &str = "CREATE VIEW overdue AS SELECT i.id, sum(l.amount) AS total \
        FROM invoice i LEFT JOIN line l ON l.invoice_id = i.id \
        WHERE i.due < 30 AND i.status IN ('open', 'sent') AND l.qty >= 0 GROUP BY i.id";

    fn unmapped(candidate: &str) -> Vec<String> {
        let report = check_candidate(&sql_atoms(SOURCE).unwrap(), candidate);
        assert!(report.parse_error.is_none(), "{report:?}");
        report.unmapped.into_iter().map(|a| a.text).collect()
    }

    #[test]
    fn the_lowering_of_a_view_invents_nothing() {
        let Lowering::Lowered(l) = lower_view(SOURCE, &LowerOptions::default()) else {
            panic!("blocked");
        };
        let report = check_candidate(&sql_atoms(SOURCE).unwrap(), &l.sql);
        assert!(report.accepted(), "{report:#?}");
        assert!(report.checked > 10);
    }

    #[test]
    fn an_invented_predicate_constant_table_or_function_is_named() {
        let faithful = "SELECT `i`.`id` FROM `invoice` AS `i` WHERE (`i`.`due` < 30)";
        assert!(unmapped(faithful).is_empty());
        // A filter nobody wrote.
        assert_eq!(
            unmapped(
                "SELECT `i`.`id` FROM `invoice` AS `i` WHERE (`i`.`due` < 30) AND (`i`.`deleted` = 0)"
            ),
            ["deleted", "0 = deleted"]
        );
        // A changed constant: both the constant and the comparison it makes are new.
        assert_eq!(
            unmapped("SELECT `id` FROM `invoice` WHERE (`due` < 31)"),
            ["31", "due < 31"]
        );
        // A table and a function from nowhere.
        assert_eq!(
            unmapped("SELECT `id`, now() FROM `invoice_archive`"),
            ["invoice_archive", "now"]
        );
    }

    #[test]
    fn an_inverted_or_widened_comparison_is_a_different_predicate() {
        assert_eq!(
            unmapped("SELECT `id` FROM `invoice` WHERE (`due` > 30)"),
            ["30 < due"]
        );
        assert_eq!(
            unmapped("SELECT `id` FROM `invoice` WHERE (`due` <= 30)"),
            ["due <= 30"]
        );
        assert_eq!(
            unmapped("SELECT `id` FROM `line` WHERE (`qty` > 0)"),
            ["0 < qty"]
        );
        // Written the other way round, it is the same comparison.
        assert!(unmapped("SELECT `id` FROM `invoice` WHERE (30 > `due`)").is_empty());
        assert!(unmapped("SELECT `id` FROM `line` WHERE (0 <= `qty`)").is_empty());
        // A list with one more value is a different list.
        assert_eq!(
            unmapped("SELECT `id` FROM `invoice` WHERE `status` IN ('open', 'sent', 'paid')"),
            ["'paid'", "status in ('open', 'paid', 'sent')"]
        );
    }

    #[test]
    fn a_candidate_that_does_not_parse_is_rejected() {
        let report = check_candidate(&sql_atoms(SOURCE).unwrap(), "SELEC id FROM invoice");
        assert!(report.parse_error.is_some());
        assert!(!report.accepted());
    }

    #[test]
    fn a_routine_states_what_its_statements_and_conditions_state() {
        let ir = ekos_plpgsql::parse_function(
            "CREATE FUNCTION close_period(in_end date) RETURNS int AS $$\n\
             DECLARE n int := 0;\n\
             BEGIN\n\
               IF in_end < '2000-01-01' THEN RAISE EXCEPTION 'too old'; END IF;\n\
               UPDATE period SET closed = true WHERE end_date <= in_end AND closed = false;\n\
               SELECT count(*) INTO n FROM period WHERE closed = true;\n\
               RETURN n;\n\
             END; $$ LANGUAGE plpgsql;",
        )
        .unwrap();
        let found = routine_atoms(&ir);
        assert_eq!(found.unparsed, 0, "{found:#?}");
        let has = |kind, text: &str| found.atoms.contains(&Atom::new(kind, text));
        assert!(has(AtomKind::Table, "period"));
        assert!(has(AtomKind::Predicate, "end_date <= in_end"));
        assert!(
            has(AtomKind::Predicate, "'2000-01-01' < in_end")
                || has(AtomKind::Predicate, "in_end < '2000-01-01'")
        );
        assert!(has(AtomKind::Function, "count"));

        // A faithful restatement of the count passes; one with a new filter does not.
        let ok = check_candidate(
            &found.atoms,
            "SELECT count(*) FROM `period` WHERE (`closed` = true)",
        );
        assert!(ok.accepted(), "{ok:#?}");
        let bad = check_candidate(
            &found.atoms,
            "SELECT count(*) FROM `period` WHERE (`closed` = true) AND (`fiscal_year` = 2024)",
        );
        assert!(!bad.accepted());
        assert!(bad.unmapped.iter().any(|a| a.text == "fiscal_year"));
    }
}

//! RFC 0246 — tier V2m: the metrics a semantic model (Apache Ossie) defines over a unit's table,
//! computed on the source and on the target and compared.
//!
//! A metric is a business number someone already relies on (`SUM(ss_ext_sales_price)`), so it
//! agreeing on both engines is a stronger statement than row counts. Only metrics over the unit's
//! own table whose functions are portable between PostgreSQL and ClickHouse are run; any other is
//! `not_run` with the reason, never a pass. Integers must be equal; other numbers equal within a
//! relative 1e-9 (engines round averages differently in the last digit). The planted control
//! offsets one target value and must be caught, or the tier does not report green.

use crate::dialect::Dialect;
use crate::divergence::{Class, Divergence};
use crate::reader::{EngineReader, ReadError};
use crate::tiers::{ControlResult, Tier, TierOutcome};
use sqlparser::ast::{Expr, Ident, visit_expressions, visit_expressions_mut};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

/// Functions both engines evaluate alike over numbers.
const PORTABLE: &[&str] = &[
    "SUM", "COUNT", "AVG", "MIN", "MAX", "NULLIF", "COALESCE", "ABS",
];

/// One metric, ready for both engines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricCheck {
    pub name: String,
    pub source_sql: String,
    pub target_sql: String,
}

/// A metric that could not be run, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRun {
    pub name: String,
    pub reason: String,
}

fn quoted(d: Dialect, col: &str) -> Ident {
    Ident::with_quote(if d == Dialect::ClickHouse { '`' } else { '"' }, col)
}

/// `SELECT <metric> FROM <table>` for one engine. `table` names the unit's table as the metric's
/// SQL qualifies it (the last part is compared).
fn query_for(d: Dialect, metric_sql: &str, table: &str, from: &str) -> Result<String, String> {
    let mut e = Parser::new(&GenericDialect {})
        .try_with_sql(metric_sql)
        .and_then(|mut p| p.parse_expr())
        .map_err(|e| format!("does not parse: {e}"))?;
    let bare = table.rsplit('.').next().unwrap_or(table).to_lowercase();
    let mut problem: Option<String> = None;
    let _ = visit_expressions(&e, |x| {
        if let Expr::Function(f) = x {
            let name = f.name.to_string().to_uppercase();
            if f.over.is_some() {
                problem = Some(format!("{name}(…) OVER is a window, not one value"));
            } else if !PORTABLE.contains(&name.as_str()) {
                problem = Some(format!("{name} is not known to agree between the engines"));
            }
        }
        if problem.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    if let Some(p) = problem {
        return Err(p);
    }
    let _ = visit_expressions_mut(&mut e, |x| {
        match x {
            Expr::CompoundIdentifier(ids) if ids.len() >= 2 => {
                let t = ids[ids.len() - 2].value.to_lowercase();
                if t != bare {
                    problem = Some(format!("reads the table {t}, not the unit's"));
                    return ControlFlow::Break(());
                }
                *x = Expr::Identifier(quoted(d, &ids[ids.len() - 1].value));
            }
            Expr::Identifier(id) => *x = Expr::Identifier(quoted(d, &id.value)),
            _ => {}
        }
        ControlFlow::Continue(())
    });
    if let Some(p) = problem {
        return Err(p);
    }
    Ok(format!("SELECT {e} FROM {from}"))
}

/// The checks for the metrics over a unit's table: `(name, sql over tables, tables read)`.
/// `source_table` / `target_table` are the FROM clauses each engine reads.
pub fn plan_metrics(
    unit_table: &str,
    source_table: &str,
    target_table: &str,
    metrics: &[(String, String, Vec<String>)],
) -> (Vec<MetricCheck>, Vec<NotRun>) {
    let bare = unit_table
        .rsplit('.')
        .next()
        .unwrap_or(unit_table)
        .to_lowercase();
    let mut checks = Vec::new();
    let mut not_run = Vec::new();
    for (name, sql, tables) in metrics {
        let own = tables.len() == 1
            && tables[0]
                .rsplit('.')
                .next()
                .unwrap_or(&tables[0])
                .to_lowercase()
                == bare;
        if !own {
            continue;
        }
        let both = query_for(Dialect::Postgres, sql, unit_table, source_table).and_then(|s| {
            query_for(Dialect::ClickHouse, sql, unit_table, target_table).map(|t| (s, t))
        });
        match both {
            Ok((source_sql, target_sql)) => checks.push(MetricCheck {
                name: name.clone(),
                source_sql,
                target_sql,
            }),
            Err(reason) => not_run.push(NotRun {
                name: name.clone(),
                reason,
            }),
        }
    }
    (checks, not_run)
}

/// Whether two engines' renderings of one number agree.
pub fn values_agree(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    let null = |s: &str| s.is_empty() || s == "\\N" || s.eq_ignore_ascii_case("null");
    if null(a) || null(b) {
        return null(a) && null(b);
    }
    if let (Ok(x), Ok(y)) = (a.parse::<i128>(), b.parse::<i128>()) {
        return x == y;
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => {
            let scale = x.abs().max(y.abs()).max(1.0);
            (x - y).abs() <= 1e-9 * scale
        }
        _ => a == b,
    }
}

/// `v` plus one, as text — the planted control's offset.
fn offset(v: &str) -> String {
    match v.trim().parse::<i128>() {
        Ok(i) => (i + 1).to_string(),
        Err(_) => v
            .trim()
            .parse::<f64>()
            .map(|f| (f + 1.0).to_string())
            .unwrap_or_else(|_| format!("{v}#")),
    }
}

/// V2m over one unit. A unit with no runnable metric returns no outcome (`None`): nothing was
/// compared, so there is nothing to report as passed.
pub fn run_v2m(
    unit: &str,
    checks: &[MetricCheck],
    source: &dyn EngineReader,
    target: &dyn EngineReader,
) -> Result<Option<TierOutcome>, ReadError> {
    let Some(first) = checks.first() else {
        return Ok(None);
    };
    let mut divergences = Vec::new();
    let mut control_value: Option<(String, String)> = None;
    for c in checks {
        let s = source
            .one_row(&c.source_sql)?
            .into_iter()
            .next()
            .unwrap_or_default();
        let t = target
            .one_row(&c.target_sql)?
            .into_iter()
            .next()
            .unwrap_or_default();
        if c.name == first.name {
            control_value = Some((s.clone(), t.clone()));
        }
        if !values_agree(&s, &t) {
            divergences.push(Divergence {
                locus: format!("{unit}:metric:{}", c.name),
                columns: vec![],
                // The metric's name only: its value is business data.
                detail: format!("the metric {} differs between source and target", c.name),
                class: Class::Unexplained,
            });
        }
    }
    let detected = control_value.is_some_and(|(s, t)| !values_agree(&s, &offset(&t)));
    Ok(Some(TierOutcome {
        tier: Tier::V2Metrics,
        source_path: source.label().to_string(),
        target_path: target.label().to_string(),
        divergences,
        controls: vec![ControlResult {
            control: format!("v2m:offset:{}", first.name),
            tier: Tier::V2Metrics,
            detected,
        }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Vec<(String, String, Vec<String>)> {
        let m = |n: &str, sql: &str, t: &[&str]| {
            (
                n.to_string(),
                sql.to_string(),
                t.iter().map(|s| s.to_string()).collect(),
            )
        };
        vec![
            m(
                "total_quantity",
                "SUM(store_sales.ss_quantity)",
                &["store_sales"],
            ),
            m(
                "avg_price",
                "AVG(store_sales.ss_sales_price)",
                &["store_sales"],
            ),
            m(
                "ratio",
                "SUM(store_sales.a) / NULLIF(COUNT(store_sales.b), 0)",
                &["store_sales"],
            ),
            m(
                "running",
                "SUM(SUM(store_sales.a)) OVER (ORDER BY store_sales.d)",
                &["store_sales"],
            ),
            m("odd_fn", "MEDIAN(store_sales.a)", &["store_sales"]),
            m(
                "cross",
                "SUM(store_sales.a) / COUNT(customer.c)",
                &["customer", "store_sales"],
            ),
            m("other", "SUM(item.p)", &["item"]),
        ]
    }

    #[test]
    fn portable_single_table_metrics_are_planned_per_engine() {
        let (checks, not_run) = plan_metrics(
            "public.store_sales",
            "\"public\".\"store_sales\"",
            "`dw`.`store_sales`",
            &metrics(),
        );
        let names: Vec<&str> = checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["total_quantity", "avg_price", "ratio"]);
        assert_eq!(
            checks[0].source_sql,
            "SELECT SUM(\"ss_quantity\") FROM \"public\".\"store_sales\""
        );
        assert_eq!(
            checks[0].target_sql,
            "SELECT SUM(`ss_quantity`) FROM `dw`.`store_sales`"
        );
        let skipped: Vec<(&str, &str)> = not_run
            .iter()
            .map(|n| (n.name.as_str(), n.reason.as_str()))
            .collect();
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(skipped[0].1.contains("window"));
        assert!(skipped[1].1.contains("MEDIAN"));
    }

    #[test]
    fn values_agree_exactly_for_integers_and_closely_for_the_rest() {
        assert!(values_agree("2378", "2378"));
        assert!(!values_agree("2378", "2379"));
        assert!(values_agree("3.1666666666666667", "3.1666666666666665"));
        assert!(!values_agree("3.16", "3.17"));
        assert!(values_agree("", "\\N"));
        assert!(!values_agree("", "0"));
        assert_eq!(offset("41"), "42");
    }

    struct Fixed(Dialect, &'static str, Vec<(&'static str, &'static str)>);
    impl EngineReader for Fixed {
        fn dialect(&self) -> Dialect {
            self.0
        }
        fn label(&self) -> &str {
            self.1
        }
        fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ReadError> {
            let v = self
                .2
                .iter()
                .find(|(k, _)| sql.contains(k))
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            Ok(vec![vec![v]])
        }
    }

    #[test]
    fn a_differing_metric_blocks_and_the_control_fires() {
        let (checks, _) = plan_metrics(
            "store_sales",
            "store_sales",
            "`dw`.`store_sales`",
            &metrics(),
        );
        let src = Fixed(
            Dialect::Postgres,
            "pg",
            vec![
                ("ss_quantity", "2378"),
                ("ss_sales_price", "98.5"),
                ("NULLIF", "7"),
            ],
        );
        let same = Fixed(
            Dialect::ClickHouse,
            "ch",
            vec![
                ("ss_quantity", "2378"),
                ("ss_sales_price", "98.50000000000001"),
                ("NULLIF", "7"),
            ],
        );
        let o = run_v2m("store_sales", &checks, &src, &same)
            .unwrap()
            .unwrap();
        assert!(o.passed(), "{o:?}");
        let off = Fixed(
            Dialect::ClickHouse,
            "ch",
            vec![
                ("ss_quantity", "2377"),
                ("ss_sales_price", "98.5"),
                ("NULLIF", "7"),
            ],
        );
        let o = run_v2m("store_sales", &checks, &src, &off)
            .unwrap()
            .unwrap();
        assert!(!o.passed());
        assert_eq!(o.divergences[0].locus, "store_sales:metric:total_quantity");
        assert!(
            !o.divergences[0].detail.contains("2377"),
            "no value in the report"
        );
        assert!(run_v2m("store_sales", &[], &src, &same).unwrap().is_none());
    }
}

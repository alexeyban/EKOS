//! What may be sent to a live database (RFC 0243): one query, and nothing in it that writes or
//! locks. Every live probe checks this before it sends anything; the session's own read-only
//! mode is the second lock, not the only one.

use crate::RowLimit;
use sqlparser::ast::{Query, SetExpr, Statement, Visit, Visitor};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

/// `Ok` when `sql` is exactly one query with no `SELECT … INTO`, no locking clause and no
/// data-modifying part (a `WITH x AS (DELETE …)` included).
pub fn single_select(sql: &str, dialect: &dyn Dialect) -> Result<(), String> {
    let stmts = Parser::parse_sql(dialect, sql).map_err(|e| format!("not sent: {e}"))?;
    let [Statement::Query(q)] = stmts.as_slice() else {
        return Err(format!(
            "not sent: only one SELECT statement may run against a live database (found {})",
            stmts.len()
        ));
    };
    struct Check(Option<&'static str>);
    impl Visitor for Check {
        type Break = ();
        fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
            self.0 = if !q.locks.is_empty() {
                Some("a locking clause (FOR UPDATE / FOR SHARE)")
            } else {
                match q.body.as_ref() {
                    SetExpr::Select(s) if s.into.is_some() => Some("SELECT … INTO"),
                    SetExpr::Select(_)
                    | SetExpr::Query(_)
                    | SetExpr::SetOperation { .. }
                    | SetExpr::Values(_)
                    | SetExpr::Table(_) => None,
                    _ => Some("a statement that changes data"),
                }
            };
            if self.0.is_some() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
    }
    let mut check = Check(None);
    let _ = q.visit(&mut check);
    match check.0 {
        Some(why) => Err(format!("not sent: the query contains {why}")),
        None => Ok(()),
    }
}

/// `sql` wrapped so the engine returns at most `n` rows, in the dialect's row-limit form.
pub fn wrap_rows(sql: &str, row_limit: RowLimit, n: usize) -> String {
    match row_limit {
        RowLimit::FetchFirst => {
            format!("SELECT * FROM ({sql}) ekos_probe FETCH FIRST {n} ROWS ONLY")
        }
        RowLimit::TopOrFetch => format!("SELECT TOP ({n}) * FROM ({sql}) ekos_probe"),
        _ => format!("SELECT * FROM ({sql}) ekos_probe LIMIT {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::dialect::{GenericDialect, PostgreSqlDialect};

    #[test]
    fn accepts_queries_with_ctes_subqueries_and_set_operations() {
        let d = PostgreSqlDialect {};
        for sql in [
            "SELECT 1",
            "WITH a AS (SELECT 1 AS x) SELECT x FROM a",
            "SELECT x FROM t WHERE y IN (SELECT y FROM u) UNION SELECT 2",
        ] {
            assert!(single_select(sql, &d).is_ok(), "{sql}");
        }
    }

    #[test]
    fn refuses_writes_locks_and_escapes() {
        let d = PostgreSqlDialect {};
        for sql in [
            "SELECT 1; DELETE FROM t",
            "SELECT 1) x; SET default_transaction_read_only = off; DELETE FROM t; SELECT (1",
            "DELETE FROM t",
            "SELECT * INTO copy FROM t",
            "SELECT * FROM t FOR UPDATE",
            "WITH gone AS (DELETE FROM t RETURNING *) SELECT * FROM gone",
            "UPDATE t SET a = 1",
        ] {
            assert!(single_select(sql, &d).is_err(), "{sql}");
        }
        assert!(single_select("DROP TABLE t", &GenericDialect {}).is_err());
    }

    #[test]
    fn wraps_in_the_dialects_row_limit_form() {
        assert!(
            wrap_rows("SELECT 1", RowLimit::FetchFirst, 3).ends_with("FETCH FIRST 3 ROWS ONLY")
        );
        assert!(wrap_rows("SELECT 1", RowLimit::TopOrFetch, 3).starts_with("SELECT TOP (3)"));
        assert!(wrap_rows("SELECT 1", RowLimit::Limit, 3).ends_with("LIMIT 3"));
    }
}

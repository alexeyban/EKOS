//! What may be sent to a DuckDB file (RFC 0259 D4) beyond the shared single-`SELECT` guard.
//!
//! DuckDB, unlike PostgreSQL, reads files and the network from a plain `SELECT`: a table function
//! (`read_csv('/etc/passwd')`), a quoted path in `FROM` (`FROM '/etc/passwd'`), or a scalar
//! (`getenv`). The session refuses all of these itself (`enable_external_access = false`, locked);
//! this check refuses them before anything is sent, so a probe never depends on one lock alone.

use ekos_sql_dialect_sdk::guard::single_select;
use sqlparser::ast::{Expr, ObjectName, TableFactor, Visit, Visitor};
use sqlparser::dialect::DuckDbDialect;
use sqlparser::parser::Parser;
use std::ops::ControlFlow;

/// Function-name prefixes that read files, other databases or the network.
const REFUSED_PREFIXES: &[&str] = &[
    "read_",
    "parquet_",
    "iceberg_",
    "delta_",
    "sqlite_",
    "postgres_",
    "mysql_",
];

/// Whole function names that read files, run SQL from a string, or reveal the environment.
const REFUSED_NAMES: &[&str] = &[
    "glob",
    "sniff_csv",
    "parquet_scan",
    "iceberg_scan",
    "delta_scan",
    "getenv",
    "query",
    "query_table",
    "st_read",
];

/// `Ok` when `sql` is one query that reads only tables of the attached file.
pub fn probe_guard(sql: &str) -> Result<(), String> {
    let dialect = DuckDbDialect {};
    single_select(sql, &dialect)?;
    let stmts = Parser::parse_sql(&dialect, sql).map_err(|e| format!("not sent: {e}"))?;
    let mut check = Refuse(None);
    for s in &stmts {
        if let ControlFlow::Break(()) = s.visit(&mut check) {
            break;
        }
    }
    match check.0 {
        Some(why) => Err(format!("not sent: {why}")),
        None => Ok(()),
    }
}

fn refused_function(name: &ObjectName) -> Option<String> {
    let last = name.0.last()?;
    let lower = last.value.to_ascii_lowercase();
    let refused = REFUSED_NAMES.contains(&lower.as_str())
        || REFUSED_PREFIXES.iter().any(|p| lower.starts_with(p));
    refused.then_some(lower)
}

struct Refuse(Option<String>);

impl Visitor for Refuse {
    type Break = ();

    fn pre_visit_table_factor(&mut self, t: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table { name, args, .. } = t {
            // `FROM 'file.csv'` — a string where a table name belongs.
            if name.0.iter().any(|i| i.quote_style == Some('\'')) {
                self.0 = Some(format!("{name} is a file path, not a table"));
            } else if let (Some(_), Some(f)) = (args, refused_function(name)) {
                self.0 = Some(format!(
                    "the table function {f}() reads outside the database"
                ));
            }
        }
        if self.0.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        if let Expr::Function(f) = e {
            if let Some(name) = refused_function(&f.name) {
                self.0 = Some(format!("the function {name}() reads outside the database"));
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mart_queries_pass() {
        for sql in [
            "SELECT docstatus, count(*), sum(grandtotal) FROM gold.fct_invoice GROUP BY ALL",
            "SELECT * EXCLUDE (x) FROM t QUALIFY row_number() OVER (ORDER BY id) = 1",
            "WITH a AS (SELECT 1 AS x) SELECT x FROM a",
            "SELECT read_count, json_extract(j, '$.a') FROM t",
            "SELECT * FROM range(10) r(i)",
        ] {
            assert!(probe_guard(sql).is_ok(), "{sql}: {:?}", probe_guard(sql));
        }
    }

    /// RFC 0259 §5: files, the network, settings and writes are refused before anything is sent.
    #[test]
    fn files_settings_and_writes_are_refused() {
        for sql in [
            "SELECT * FROM read_csv('/etc/passwd')",
            "SELECT * FROM read_csv_auto('/etc/passwd')",
            "SELECT * FROM read_parquet('s3://bucket/x.parquet')",
            "SELECT * FROM '/etc/passwd'",
            "SELECT * FROM t WHERE x IN (SELECT * FROM read_text('/etc/hostname'))",
            "SELECT * FROM glob('/home/*')",
            "SELECT * FROM iceberg_scan('s3://b/t')",
            "SELECT * FROM query('SELECT 1')",
            "SELECT getenv('HOME')",
            "ATTACH '/tmp/other.duckdb' AS o",
            "SET enable_external_access = true",
            "COPY t TO '/tmp/out.csv'",
            "INSTALL httpfs",
            "LOAD httpfs",
            "CREATE TABLE x AS SELECT 1",
            "PRAGMA version",
            "SELECT 1; SELECT 2",
        ] {
            let e = probe_guard(sql).unwrap_err();
            assert!(e.starts_with("not sent"), "{sql}: {e}");
        }
    }
}

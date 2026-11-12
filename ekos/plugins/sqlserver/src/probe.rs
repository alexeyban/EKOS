//! RFC 0243 for SQL Server: checking SQL against a live database for `ekos sql check
//! --database` — the text-to-SQL probes run through this plugin's read-only session.
//!
//! The locks, in order: the text must be one `SELECT` (`guard::single_select`, T-SQL grammar)
//! before anything is sent; it runs inside a transaction that is always rolled back, with
//! `ApplicationIntent=ReadOnly`, a lock timeout and a command timeout (`crate::session`); and a
//! login that may write to the listed schemas is refused unless the person running the check
//! accepts the risk (`--allow-writable-role`), as for PostgreSQL.

use crate::SqlServerConnection;
use crate::session::Session;
use ekos_sql_dialect_sdk::guard::single_select;
use ekos_sql_dialect_sdk::probe::{Preview, Probe};
use sqlparser::dialect::MsSqlDialect;
use std::cell::RefCell;

/// A probe over one read-only session. Not `Send`: use it on the thread that connected.
pub struct SqlServerProbe {
    session: RefCell<Session>,
}

impl SqlServerProbe {
    /// Connects and checks the login's rights on `schemas`. `Err` says why the database will not
    /// be probed.
    pub fn connect(
        connection: &SqlServerConnection,
        schemas: &[String],
        run_id: &str,
        allow_writable_role: bool,
    ) -> Result<SqlServerProbe, String> {
        let mut session = Session::connect(connection, run_id)?;
        let check = session.rows(&crate::catalog::write_check(schemas))?;
        let verdict = check
            .first()
            .map(|r| crate::catalog::write_check_result(r))
            .unwrap_or_default();
        if verdict["result"] != "refused_by_server" && !allow_writable_role {
            return Err(format!(
                "the login {} can write to {}, so generated SQL is not run there; connect as a \
                 read-only login (or pass --allow-writable-role to accept the risk)",
                connection.user,
                connection.label()
            ));
        }
        Ok(SqlServerProbe {
            session: RefCell::new(session),
        })
    }
}

/// The text sent for `sql`: refused unless it is one `SELECT`, then cut to `max_rows + 1` rows.
///
/// Not wrapped in `SELECT TOP (n) * FROM (…)` (`guard::wrap_rows`): T-SQL refuses an `ORDER BY`
/// inside a derived table, and generated queries usually end in one. `SET ROWCOUNT` stops the
/// server after `n` rows of a `SELECT` instead; the session resets it with every rollback.
pub fn probe_text(sql: &str, max_rows: usize) -> Result<String, String> {
    single_select(sql, &MsSqlDialect {})?;
    let sql = sql.trim().trim_end_matches(';');
    Ok(format!("SET ROWCOUNT {}; {sql}", max_rows + 1))
}

impl Probe for SqlServerProbe {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        let text = probe_text(sql, max_rows)?;
        let (columns, mut rows) = self.session.borrow_mut().query(&text)?;
        let truncated = rows.len() > max_rows;
        rows.truncate(max_rows);
        Ok(Preview {
            columns,
            rows,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_select_is_ever_sent() {
        for write in [
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "SELECT 1; DROP TABLE t",
            "EXEC sp_who",
        ] {
            let e = probe_text(write, 5).unwrap_err();
            assert!(e.starts_with("not sent"), "{write}: {e}");
        }
        assert_eq!(
            probe_text("SELECT id FROM sales.orders ORDER BY id;", 5).unwrap(),
            "SET ROWCOUNT 6; SELECT id FROM sales.orders ORDER BY id"
        );
    }

    #[test]
    fn the_usage_queries_pass_the_guard() {
        use ekos_observation_sdk::usage::{TableName, UsageSource, available_sql, usage_sql};
        let t = vec![TableName {
            owner: "sales".into(),
            table: "order_lines".into(),
        }];
        for src in [UsageSource::QueryStore, UsageSource::ExecQueryStats] {
            let sql = usage_sql(src, &t).unwrap();
            if let Err(e) = probe_text(&sql, 10) {
                panic!("{src:?}: {e}\n{sql}");
            }
        }
        assert!(probe_text(available_sql(UsageSource::QueryStore).unwrap(), 1).is_ok());
    }
}

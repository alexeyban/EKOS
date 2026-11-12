//! RFC 0243: checking SQL against a live PostgreSQL database — the text-to-SQL probes run
//! through this plugin's read-only session.
//!
//! Three locks, each enough on its own for the common case: the text must be one SELECT
//! (`guard::single_select`), the session is `default_transaction_read_only` with a statement
//! timeout, and the role itself must be refused a write (a session setting can be undone by a
//! statement; a role without the privilege cannot). A writable role is probed only when the
//! person running the check says so.

use crate::PgConnection;
use ekos_pg_live::PgSource;
use ekos_sql_dialect_sdk::RowLimit;
use ekos_sql_dialect_sdk::guard::{single_select, wrap_rows};
use ekos_sql_dialect_sdk::probe::{Preview, Probe};
use sqlparser::dialect::PostgreSqlDialect;

/// A probe over one read-only session. Not `Send`: use it on the thread that connected.
pub struct PgProbe {
    source: PgSource,
}

impl PgProbe {
    /// Connects and checks the role. `Err` says why the database will not be probed.
    pub fn connect(
        connection: &PgConnection,
        run_id: &str,
        allow_writable_role: bool,
    ) -> Result<PgProbe, String> {
        let source = connection.connect(run_id).map_err(|e| e.to_string())?;
        let check = source.check_write_refused().map_err(|e| e.to_string())?;
        if !check.is_safe() && !allow_writable_role {
            return Err(format!(
                "the role {} can write to {}, so generated SQL is not run there; connect as a \
                 read-only role (or pass --allow-writable-role to accept the risk)",
                connection.user,
                connection.label()
            ));
        }
        Ok(PgProbe { source })
    }
}

impl Probe for PgProbe {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        single_select(sql, &PostgreSqlDialect {})?;
        let wrapped = wrap_rows(sql, RowLimit::LimitOrFetch, max_rows + 1);
        let r = self
            .source
            .preview(&wrapped, max_rows)
            .map_err(|e| e.to_string())?;
        Ok(Preview {
            columns: r.columns,
            rows: r.rows,
            truncated: r.truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_queries_pass_the_guard() {
        use ekos_observation_sdk::usage::{TableName, UsageSource, available_sql, usage_sql};
        let t = vec![TableName {
            owner: "public".into(),
            table: "acc_trans".into(),
        }];
        for src in [UsageSource::PgStatStatements, UsageSource::TableStats] {
            let sql = usage_sql(src, &t).unwrap();
            if let Err(e) = single_select(&sql, &PostgreSqlDialect {}) {
                panic!("{src:?}: {e}\n{sql}");
            }
        }
        assert!(
            single_select(
                available_sql(UsageSource::PgStatStatements).unwrap(),
                &PostgreSqlDialect {}
            )
            .is_ok()
        );
    }
}

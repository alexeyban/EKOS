//! RFC 0243 for Informix: checking SQL against a live database for `ekos sql check --database`
//! (and the aggregate queries of `ekos profile run` / `ekos relationships discover`) — the probes
//! run through this plugin's read-only session.
//!
//! The locks, in order: the text must be one `SELECT` (`guard::single_select`) before anything is
//! sent; it runs in its own read-only transaction that is always rolled back (`crate::session`);
//! at most `max_rows + 1` rows are fetched and the cursor is closed; and a user that may write to
//! the listed owners' tables is refused unless the person running the check accepts the risk
//! (`--allow-writable-role`), as for PostgreSQL and SQL Server.
//!
//! There is no Informix grammar in `sqlparser`, so the guard parses with the generic dialect: a
//! query only Informix reads (`SELECT FIRST 10 …`, `MATCHES`, outer joins written `OUTER t`) is
//! refused as "not sent" rather than sent unchecked. One Informix form is read: a table of the
//! server's `sysmaster` database written `sysmaster:table` (RFC 0252 usage evidence reads
//! `sysmaster:syssqltrace` and `sysmaster:sysptprof`) — the guard parses it as `sysmaster.table`,
//! which has the same structure, and the original text is sent.

use crate::InformixConnection;
use crate::session::Session;
use ekos_sql_dialect_sdk::guard::single_select;
use ekos_sql_dialect_sdk::probe::{Preview, Probe};
use sqlparser::dialect::GenericDialect;
use std::cell::RefCell;

/// A probe over one read-only session. Not `Send`: use it on the thread that connected.
pub struct InformixProbe {
    session: RefCell<Session>,
}

impl InformixProbe {
    /// Connects and checks the user's rights on `owners`. `Err` says why the database will not be
    /// probed.
    pub fn connect(
        connection: &InformixConnection,
        owners: &[String],
        run_id: &str,
        allow_writable_role: bool,
    ) -> Result<InformixProbe, String> {
        let mut session = Session::connect(connection, run_id)?;
        let check = session.rows(&crate::catalog::write_check(owners))?;
        let verdict = check
            .first()
            .map(|r| crate::catalog::write_check_result(r, &session.guards()))
            .unwrap_or_default();
        if verdict["result"] != "refused_by_server" && !allow_writable_role {
            return Err(format!(
                "the user {} can write to {}, so generated SQL is not run there; connect as a \
                 read-only user (or pass --allow-writable-role to accept the risk)",
                connection.user,
                connection.label()
            ));
        }
        Ok(InformixProbe {
            session: RefCell::new(session),
        })
    }
}

/// The text sent for `sql`: refused unless it is one `SELECT`. Not wrapped in a row limit — the
/// session stops fetching after `max_rows + 1` rows instead, because Informix refuses an
/// `ORDER BY` inside a derived table and generated queries usually end in one.
pub fn probe_text(sql: &str) -> Result<String, String> {
    single_select(&guard_text(sql), &GenericDialect {})?;
    Ok(sql.trim().trim_end_matches(';').to_string())
}

/// What the guard parses: `sysmaster:t` → `sysmaster.t` (a qualifier the generic grammar reads).
/// Nothing else changes, so a statement that is not one `SELECT` stays one that is not.
fn guard_text(sql: &str) -> String {
    const DB: &str = "sysmaster:";
    let lower = sql.to_ascii_lowercase();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while let Some(at) = lower[i..].find(DB) {
        let start = i + at;
        let end = start + DB.len();
        let next_is_name = sql[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '"');
        out.push_str(&sql[i..end - 1]);
        out.push(if next_is_name { '.' } else { ':' });
        i = end;
    }
    out.push_str(&sql[i..]);
    out
}

impl Probe for InformixProbe {
    fn preview(&self, sql: &str, max_rows: usize) -> Result<Preview, String> {
        let text = probe_text(sql)?;
        let (columns, rows, truncated) = self
            .session
            .borrow_mut()
            .query_limited(&text, Some(max_rows))?;
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
            "SELECT 1 FROM t; DROP TABLE t",
            "EXECUTE PROCEDURE p()",
        ] {
            let e = probe_text(write).unwrap_err();
            assert!(e.starts_with("not sent"), "{write}: {e}");
        }
        assert_eq!(
            probe_text("SELECT id FROM \"sales\".orders ORDER BY id;").unwrap(),
            "SELECT id FROM \"sales\".orders ORDER BY id"
        );
        // Usage evidence reads the server's sysmaster tables; a write there is still refused.
        let t = vec![ekos_observation_sdk::usage::TableName {
            owner: "sales".into(),
            table: "orders".into(),
        }];
        for src in [
            ekos_observation_sdk::usage::UsageSource::SqlTrace,
            ekos_observation_sdk::usage::UsageSource::PartitionProfile,
        ] {
            let sql = ekos_observation_sdk::usage::usage_sql(src, &t).unwrap();
            assert_eq!(probe_text(&sql).unwrap(), sql);
        }
        for write in [
            "DELETE FROM sysmaster:syssqltrace",
            "SELECT 1 FROM sysmaster:sysptprof; DROP TABLE t",
        ] {
            assert!(
                probe_text(write).unwrap_err().starts_with("not sent"),
                "{write}"
            );
        }
        assert_eq!(
            guard_text("a sysmaster:x, SYSMASTER:\"y\" sysmaster:"),
            "a sysmaster.x, SYSMASTER.\"y\" sysmaster:"
        );
        // The profile's own aggregates pass the guard.
        assert!(
            probe_text(
                "SELECT COUNT(*) FROM (SELECT DISTINCT \"a\", \"b\" FROM \"s\".\"t\" \
                 WHERE \"a\" IS NOT NULL AND \"b\" IS NOT NULL) d"
            )
            .is_ok()
        );
        assert!(
            probe_text(
                "SELECT CASE WHEN CAST(\"c\" AS LVARCHAR(4000)) IN ('A') THEN \
                 CAST(\"c\" AS LVARCHAR(4000)) END, COUNT(*), TO_CHAR(MIN(\"t\"), '%Y-%m') \
                 FROM \"s\".\"x\" WHERE \"c\" IS NOT NULL GROUP BY 1"
            )
            .is_ok()
        );
    }
}

//! The read-only discipline of a SQL Server session (RFC 0252 §3.5).
//!
//! SQL Server has no session-level read-only flag like PostgreSQL's
//! `default_transaction_read_only` or ClickHouse's `readonly=1`. What this session does instead,
//! each of which holds on its own for the common case:
//!
//! 1. **`ApplicationIntent=ReadOnly`** at login. In an Availability Group with read-only routing,
//!    the listener sends the session to a readable secondary, where every write fails. On a
//!    standalone server it changes nothing, which is why it is not the only lock.
//! 2. **Every statement runs inside an explicit transaction that is always rolled back**
//!    (`BEGIN TRANSACTION` … `IF @@TRANCOUNT > 0 ROLLBACK`), with `XACT_ABORT ON` so an error ends
//!    the transaction instead of leaving it open. Even a statement that did write would leave
//!    nothing behind.
//! 3. **Only planned text is sent.** The catalog queries are this crate's constants, the profile
//!    queries come from `ekos_observation_sdk::profile`'s closed aggregate vocabulary, and the
//!    `ekos sql check` probe refuses anything that is not one `SELECT`
//!    (`ekos_sql_dialect_sdk::guard::single_select`) before it is sent.
//! 4. **The login's own rights are checked** (`HAS_PERMS_BY_NAME`, see `catalog::WRITE_CHECK`)
//!    and reported, exactly as the other kinds report their write probe; a probe refuses a login
//!    that can write unless the person running it says otherwise.
//!
//! The session also never waits long on someone else's lock (`SET LOCK_TIMEOUT`), gives way in a
//! deadlock (`SET DEADLOCK_PRIORITY LOW`), and bounds every server round trip (tiberius's
//! command timeout). It sets `DATEFIRST` to
//! [`EKOS_DATEFIRST`](ekos_observation_sdk::usage::EKOS_DATEFIRST): Query Store and the plan cache
//! record it per statement, so usage evidence (RFC 0252 §3.5) leaves EKOS's own queries out. No
//! EKOS query depends on the first day of the week. After a timeout tiberius leaves the connection mid-response, so a timed-out
//! session is reported lost and not reused.

use crate::SqlServerConnection;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, FromSql};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

/// Rows of cells as text, `None` for SQL NULL.
pub type Rows = Vec<Vec<Option<String>>>;

/// Milliseconds a statement waits for a lock before failing.
pub const LOCK_TIMEOUT_MS: u32 = 10_000;
/// Seconds one server round trip may take.
pub const COMMAND_TIMEOUT_SECS: u64 = 120;

/// The settings sent once after login (a batch of their own, so every later statement is compiled
/// under them — `DATEFIRST` included).
pub const SESSION_SETTINGS: &str = "SET LOCK_TIMEOUT 10000; SET DEADLOCK_PRIORITY LOW; \
     SET XACT_ABORT ON; SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET NOCOUNT ON; \
     SET DATEFIRST 5;";

/// One connection on its own single-threaded runtime: blocking calls, like `ekos-pg-live`'s.
/// Create and use it off any async runtime (a blocking thread or a plain one).
pub struct Session {
    rt: tokio::runtime::Runtime,
    client: Client<Compat<TcpStream>>,
    lost: bool,
}

/// The tiberius configuration for `conn`. Pure apart from reading the password variable.
pub(crate) fn config(conn: &SqlServerConnection, run_id: &str) -> Result<Config, String> {
    let password = std::env::var(&conn.password_env).map_err(|_| {
        format!(
            "the environment variable {} names this database's password but is not set",
            conn.password_env
        )
    })?;
    let mut c = Config::new();
    c.host(&conn.host);
    c.port(conn.port);
    c.database(&conn.database);
    c.authentication(AuthMethod::sql_server(&conn.user, password));
    c.application_name(format!("ekos/{run_id}"));
    // Lock 1: route to a readable secondary where there is one.
    c.readonly(true);
    // Credentials and catalog text never travel in clear.
    c.encryption(EncryptionLevel::Required);
    if conn.trust_server_certificate {
        c.trust_cert();
    }
    c.command_timeout(Some(std::time::Duration::from_secs(COMMAND_TIMEOUT_SECS)));
    Ok(c)
}

impl Session {
    /// Connect and apply the session settings.
    pub fn connect(conn: &SqlServerConnection, run_id: &str) -> Result<Session, String> {
        let config = config(conn, run_id)?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let client = rt.block_on(async {
            let tcp = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                TcpStream::connect(config.get_addr()),
            )
            .await
            .map_err(|_| format!("cannot connect to {}: timed out", conn.label()))?
            .map_err(|e| format!("cannot connect to {}: {e}", conn.label()))?;
            tcp.set_nodelay(true).map_err(|e| e.to_string())?;
            let mut client = Client::connect(config, tcp.compat_write())
                .await
                .map_err(|e| format!("cannot sign in to {}: {e}", conn.label()))?;
            client
                .simple_query(SESSION_SETTINGS)
                .await
                .map_err(|e| e.to_string())?
                .into_results()
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(client)
        })?;
        Ok(Session {
            rt,
            client,
            lost: false,
        })
    }

    /// Whether an earlier failure left the connection unusable.
    pub fn is_lost(&self) -> bool {
        self.lost
    }

    /// One statement inside a transaction that is always rolled back: column names and rows.
    pub fn query(&mut self, sql: &str) -> Result<(Vec<String>, Rows), String> {
        if self.lost {
            return Err("the session was lost after an earlier failure".into());
        }
        let client = &mut self.client;
        let result = self.rt.block_on(async {
            client
                .simple_query("BEGIN TRANSACTION")
                .await
                .map_err(|e| e.to_string())?
                .into_results()
                .await
                .map_err(|e| e.to_string())?;
            let answer = run(client, sql).await;
            // Lock 2: whatever happened, nothing is kept. Under XACT_ABORT an error has already
            // rolled back, hence the guard.
            let rollback = async {
                client
                    .simple_query("SET ROWCOUNT 0; IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION")
                    .await?
                    .into_results()
                    .await
            }
            .await
            .map_err(|e| format!("rollback failed: {e}"));
            match (answer, rollback) {
                (Ok(a), Ok(_)) => Ok(a),
                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
            }
        });
        if let Err(e) = &result
            && session_lost(e)
        {
            self.lost = true;
        }
        result
    }

    /// The rows only.
    pub fn rows(&mut self, sql: &str) -> Result<Rows, String> {
        self.query(sql).map(|(_, rows)| rows)
    }
}

async fn run(
    client: &mut Client<Compat<TcpStream>>,
    sql: &str,
) -> Result<(Vec<String>, Rows), String> {
    let mut stream = client.simple_query(sql).await.map_err(|e| e.to_string())?;
    let columns = stream
        .columns()
        .await
        .map_err(|e| e.to_string())?
        .map(|cs| cs.iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default();
    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|row| row.cells().map(|(_, d)| cell_text(d)).collect())
        .collect();
    Ok((columns, rows))
}

/// An I/O failure or a timeout leaves the TDS stream out of step; a refused statement does not.
pub(crate) fn session_lost(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("timed out")
        || m.contains("timeout")
        || m.contains("connection reset")
        || m.contains("broken pipe")
        || m.contains("unexpected eof")
        || m.contains("rollback failed")
}

/// One cell as text, `None` for NULL. Dates in ISO 8601, binary as `0x…` hex.
pub fn cell_text(d: &ColumnData<'static>) -> Option<String> {
    use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
    match d {
        ColumnData::U8(v) => v.map(|x| x.to_string()),
        ColumnData::I16(v) => v.map(|x| x.to_string()),
        ColumnData::I32(v) => v.map(|x| x.to_string()),
        ColumnData::I64(v) => v.map(|x| x.to_string()),
        ColumnData::F32(v) => v.map(|x| x.to_string()),
        ColumnData::F64(v) => v.map(|x| x.to_string()),
        ColumnData::Bit(v) => v.map(|b| if b { "1" } else { "0" }.to_string()),
        ColumnData::String(v) => v.as_ref().map(|s| s.to_string()),
        ColumnData::Guid(v) => v.map(|g| g.to_string()),
        ColumnData::Binary(v) => v.as_ref().map(|b| format!("0x{}", hex::encode(b))),
        ColumnData::Numeric(v) => v.map(|n| n.to_string()),
        ColumnData::Xml(v) => v.as_ref().map(|x| x.to_string()),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            NaiveDateTime::from_sql(d)
                .ok()
                .flatten()
                .map(|t| t.to_string())
        }
        ColumnData::Date(_) => NaiveDate::from_sql(d).ok().flatten().map(|t| t.to_string()),
        ColumnData::Time(_) => NaiveTime::from_sql(d).ok().flatten().map(|t| t.to_string()),
        ColumnData::DateTimeOffset(_) => DateTime::<Utc>::from_sql(d)
            .ok()
            .flatten()
            .map(|t| t.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn cells_read_as_text_with_null_kept() {
        assert_eq!(cell_text(&ColumnData::I64(Some(42))), Some("42".into()));
        assert_eq!(cell_text(&ColumnData::I32(None)), None);
        assert_eq!(cell_text(&ColumnData::Bit(Some(true))), Some("1".into()));
        assert_eq!(
            cell_text(&ColumnData::String(Some(Cow::Borrowed("dbo")))),
            Some("dbo".into())
        );
        assert_eq!(
            cell_text(&ColumnData::Binary(Some(Cow::Borrowed(&[0xab, 0x01])))),
            Some("0xab01".into())
        );
    }

    #[test]
    fn the_session_tags_itself_for_usage_evidence() {
        let tag = format!(
            "SET DATEFIRST {};",
            ekos_observation_sdk::usage::EKOS_DATEFIRST
        );
        assert!(SESSION_SETTINGS.ends_with(&tag), "{SESSION_SETTINGS}");
    }

    #[test]
    fn a_timeout_loses_the_session_a_refusal_does_not() {
        assert!(session_lost("Operation timed out"));
        assert!(session_lost("rollback failed: broken pipe"));
        assert!(!session_lost(
            "The SELECT permission was denied on the object 'orders'"
        ));
    }

    #[test]
    fn an_unset_password_variable_is_named_never_guessed() {
        let missing = SqlServerConnection::parse("h/d", "ro", "EKOS_TEST_MSSQL_PW_UNSET").unwrap();
        let e = config(&missing, "t").unwrap_err();
        assert!(
            e.contains("EKOS_TEST_MSSQL_PW_UNSET") && e.contains("not set"),
            "{e}"
        );
    }
}

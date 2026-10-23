//! RFC 0211 §3.2 — connecting to an Oracle source, and the session settings that make it safe.
//!
//! As in `pg-live`, every setting is a guard applied before the first statement of real work. The
//! driver is compiled only with the crate feature `oracle`; without it [`OraSource::connect`]
//! returns [`OraError::DriverNotBuilt`], so callers need no `cfg` of their own.

use crate::OraError;
use crate::catalog::{CatalogSource, Query, Rows, ViewFamily};
use crate::profile::ScanSource;

/// How a source session is configured. Policy, not constants: the right timeout for a large
/// warehouse is not the right one for an application database.
#[derive(Debug, Clone)]
pub struct SessionPolicy {
    /// ODPI-C call timeout: no single round trip may take longer.
    pub call_timeout_ms: u32,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            call_timeout_ms: 30_000,
        }
    }
}

/// The NLS settings every session pins, so numbers and dates read as strings render identically
/// whatever the server's or client's defaults are.
pub const SESSION_SETTINGS: &[(&str, &str)] = &[
    ("NLS_DATE_FORMAT", "YYYY-MM-DD\"T\"HH24:MI:SS"),
    ("NLS_TIMESTAMP_FORMAT", "YYYY-MM-DD\"T\"HH24:MI:SS.FF"),
    (
        "NLS_TIMESTAMP_TZ_FORMAT",
        "YYYY-MM-DD\"T\"HH24:MI:SS.FFTZH:TZM",
    ),
    ("NLS_NUMERIC_CHARACTERS", ".,"),
    ("NLS_SORT", "BINARY"),
    ("NLS_COMP", "BINARY"),
    ("TIME_ZONE", "UTC"),
];

/// Where to connect. The password is named by an environment variable; the ledger and every
/// configuration file store the variable's name, never its value (RFC 0154's rule).
#[derive(Debug, Clone)]
pub struct OraConnection {
    /// Easy Connect string: `host:port/service`.
    pub connect_string: String,
    pub user: String,
    pub password_env: String,
}

impl OraConnection {
    pub fn label(&self) -> String {
        format!(
            "oracle:{}@{}",
            self.user.to_lowercase(),
            self.connect_string
        )
    }

    #[cfg_attr(not(feature = "oracle"), allow(dead_code))]
    fn password(&self) -> Result<String, OraError> {
        std::env::var(&self.password_env)
            .map_err(|_| OraError::MissingSecret(self.password_env.clone()))
    }
}

/// The result of asking the server whether this user can write.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum WriteCheck {
    /// `ORA-01031`: the guarantee EKOS wants.
    RefusedByServer { message: String },
    /// The zero-row insert was accepted (and rolled back): usable, but a finding.
    UserCanWrite,
}

impl WriteCheck {
    pub fn is_safe(&self) -> bool {
        matches!(self, Self::RefusedByServer { .. })
    }
}

/// An Oracle identifier safe to splice into SQL inside double quotes.
pub fn guard_identifier(name: &str) -> Result<&str, OraError> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$' | b'#'));
    if ok {
        Ok(name)
    } else {
        Err(OraError::UnsafeIdentifier(name.to_string()))
    }
}

/// A live, read-only Oracle session.
pub struct OraSource {
    label: String,
    #[cfg(feature = "oracle")]
    conn: oracle::Connection,
}

impl OraSource {
    pub fn label(&self) -> &str {
        &self.label
    }
}

#[cfg(not(feature = "oracle"))]
impl OraSource {
    pub fn connect(
        _conn: &OraConnection,
        _run_id: &str,
        _policy: &SessionPolicy,
    ) -> Result<Self, OraError> {
        Err(OraError::DriverNotBuilt)
    }

    pub fn family(&self) -> ViewFamily {
        ViewFamily::All
    }

    pub fn check_write_refused(&self, _owner: &str, _table: &str) -> Result<WriteCheck, OraError> {
        Err(OraError::DriverNotBuilt)
    }

    pub fn session_privileges(&self) -> Result<Vec<String>, OraError> {
        Err(OraError::DriverNotBuilt)
    }

    pub fn rows_of(&self, _sql: &str, _binds: &[&str]) -> Result<Rows, OraError> {
        Err(OraError::DriverNotBuilt)
    }

    pub fn begin_read_only(&self) -> Result<(), OraError> {
        Err(OraError::DriverNotBuilt)
    }

    pub fn end_transaction(&self) -> Result<(), OraError> {
        Err(OraError::DriverNotBuilt)
    }
}

#[cfg(feature = "oracle")]
impl OraSource {
    /// Connect and apply the session guards. `run_id` becomes the session's `ACTION`, so a DBA
    /// watching `V$SESSION` can attribute every statement to one EKOS run.
    pub fn connect(
        conn: &OraConnection,
        run_id: &str,
        policy: &SessionPolicy,
    ) -> Result<Self, OraError> {
        let password = conn.password()?;
        let c = oracle::Connection::connect(&conn.user, &password, &conn.connect_string)
            .map_err(|e| OraError::Connect(e.to_string()))?;
        c.set_call_timeout(Some(std::time::Duration::from_millis(u64::from(
            policy.call_timeout_ms,
        ))))
        .map_err(|e| session_error("call timeout", &e))?;
        c.set_module("ekos")
            .map_err(|e| session_error("module", &e))?;
        c.set_action(run_id)
            .map_err(|e| session_error("action", &e))?;
        for (setting, value) in SESSION_SETTINGS {
            // ALTER SESSION takes no binds; the values are the constants above.
            c.execute(&format!("ALTER SESSION SET {setting} = '{value}'"), &[])
                .map_err(|e| session_error(setting, &e))?;
        }
        Ok(Self {
            label: conn.label(),
            conn: c,
        })
    }

    /// `DBA_*` when the session can read it (`SELECT_CATALOG_ROLE`), `ALL_*` otherwise.
    pub fn family(&self) -> ViewFamily {
        match self
            .conn
            .query("SELECT 1 FROM dba_objects WHERE ROWNUM = 1", &[])
        {
            Ok(_) => ViewFamily::Dba,
            Err(_) => ViewFamily::All,
        }
    }

    /// Ask the server whether this user can write `owner.table`, by a zero-row insert that is
    /// always rolled back. The privilege is checked at parse, so a read-only user gets `ORA-01031`
    /// and a writer inserts nothing. A DDL probe is not used: Oracle DDL commits implicitly.
    pub fn check_write_refused(&self, owner: &str, table: &str) -> Result<WriteCheck, OraError> {
        let target = format!(
            "\"{}\".\"{}\"",
            guard_identifier(owner)?,
            guard_identifier(table)?
        );
        let sql = format!("INSERT INTO {target} SELECT * FROM {target} WHERE 1 = 0");
        let result = self.conn.execute(&sql, &[]);
        let rollback = self.conn.rollback();
        match result {
            Ok(_) => {
                rollback.map_err(|e| query_error("ROLLBACK", &e))?;
                Ok(WriteCheck::UserCanWrite)
            }
            Err(e) if e.db_error().is_some_and(|d| d.code() == 1031) => {
                Ok(WriteCheck::RefusedByServer {
                    message: e.to_string(),
                })
            }
            Err(e) => Err(query_error(&sql, &e)),
        }
    }

    /// `SET TRANSACTION READ ONLY`: the rest of the transaction reads one snapshot and the server
    /// refuses any write in it (`ORA-01456`), whatever the user's privileges.
    pub fn begin_read_only(&self) -> Result<(), OraError> {
        // It must be the transaction's first statement; end anything a caller left open.
        self.end_transaction()?;
        self.conn
            .execute("SET TRANSACTION READ ONLY", &[])
            .map(|_| ())
            .map_err(|e| query_error("SET TRANSACTION READ ONLY", &e))
    }

    /// End the current transaction. Always a rollback: nothing this session does is kept.
    pub fn end_transaction(&self) -> Result<(), OraError> {
        self.conn
            .rollback()
            .map_err(|e| query_error("ROLLBACK", &e))
    }

    /// `SESSION_PRIVS`: an `… ANY TABLE` privilege is a finding even when one table refuses.
    pub fn session_privileges(&self) -> Result<Vec<String>, OraError> {
        Ok(self
            .rows_of(
                "SELECT privilege FROM session_privs ORDER BY privilege",
                &[],
            )?
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect())
    }

    /// Run a query with string binds and read every column as a nullable string.
    pub fn rows_of(&self, sql: &str, binds: &[&str]) -> Result<Rows, OraError> {
        let params: Vec<&dyn oracle::sql_type::ToSql> = binds
            .iter()
            .map(|b| b as &dyn oracle::sql_type::ToSql)
            .collect();
        let rs = self
            .conn
            .query(sql, &params)
            .map_err(|e| query_error(sql, &e))?;
        let mut out = Vec::new();
        for row in rs {
            let row = row.map_err(|e| query_error(sql, &e))?;
            let n = row.sql_values().len();
            let mut cells = Vec::with_capacity(n);
            for i in 0..n {
                cells.push(
                    row.get::<usize, Option<String>>(i)
                        .map_err(|e| query_error(sql, &e))?,
                );
            }
            out.push(cells);
        }
        Ok(out)
    }
}

#[cfg(feature = "oracle")]
fn session_error(setting: &str, e: &oracle::Error) -> OraError {
    OraError::Session {
        setting: setting.to_string(),
        message: e.to_string(),
    }
}

#[cfg(feature = "oracle")]
fn query_error(sql: &str, e: &oracle::Error) -> OraError {
    OraError::Query {
        sql: sql.to_string(),
        message: e.to_string(),
    }
}

impl CatalogSource for OraSource {
    fn rows(&self, query: &Query, owner: &str) -> Result<Rows, OraError> {
        self.rows_of(&query.sql, &[owner])
    }

    fn begin_snapshot(&self) -> Result<(), OraError> {
        self.begin_read_only()
    }

    fn end_snapshot(&self) -> Result<(), OraError> {
        self.end_transaction()
    }
}

impl ScanSource for OraSource {
    fn scan(&self, sql: &str) -> Result<Rows, OraError> {
        self.rows_of(sql, &[])
    }

    fn begin_read_only(&self) -> Result<(), OraError> {
        OraSource::begin_read_only(self)
    }

    fn end_transaction(&self) -> Result<(), OraError> {
        OraSource::end_transaction(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_that_could_break_out_of_quotes_are_refused() {
        assert!(guard_identifier("C_INVOICE").is_ok());
        assert!(guard_identifier("T$X#1").is_ok());
        assert!(guard_identifier("X\" FROM dual --").is_err());
        assert!(guard_identifier("").is_err());
    }

    #[test]
    fn a_missing_password_variable_names_the_variable_not_a_value() {
        let c = OraConnection {
            connect_string: "localhost:1521/XEPDB1".into(),
            user: "ekos_ro".into(),
            password_env: "EKOS_TEST_SURELY_UNSET_PASSWORD".into(),
        };
        let err = c.password().unwrap_err();
        assert!(err.to_string().contains("EKOS_TEST_SURELY_UNSET_PASSWORD"));
        assert_eq!(c.label(), "oracle:ekos_ro@localhost:1521/XEPDB1");
    }

    #[cfg(not(feature = "oracle"))]
    #[test]
    fn without_the_driver_connecting_says_so() {
        let c = OraConnection {
            connect_string: "x".into(),
            user: "u".into(),
            password_env: "P".into(),
        };
        assert!(matches!(
            OraSource::connect(&c, "run", &SessionPolicy::default()),
            Err(OraError::DriverNotBuilt)
        ));
    }
}

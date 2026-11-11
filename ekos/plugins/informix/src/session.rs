//! The read-only discipline of an Informix session (RFC 0252 §3.5).
//!
//! Informix has no login-level read-only flag. What this session does instead:
//!
//! 1. **Autocommit is off and every statement is rolled back.** In a database with transaction
//!    logging, the driver opens a transaction before the first statement; the session rolls it back
//!    after each one, so even a statement that did write would leave nothing behind. A database
//!    created without logging has no transactions at all: the session records that
//!    ([`Guards::transactions`]) and the write check reports it, because this lock is then absent.
//! 2. **`SET TRANSACTION READ ONLY`** opens every transaction where the server accepts it: the
//!    server itself then refuses `INSERT`, `UPDATE`, `DELETE` and DDL inside it.
//! 3. **Only planned text is sent.** The catalog queries are this crate's constants, the profile
//!    queries come from `ekos_observation_sdk::profile`'s closed aggregate vocabulary, and the
//!    `ekos sql check` probe refuses anything that is not one `SELECT`
//!    (`ekos_sql_dialect_sdk::guard::single_select`) before it is sent.
//! 4. **The user's own rights are checked** (`systabauth`, `sysusers`, roles — see
//!    `catalog::write_check`) and reported, as for every kind; a probe refuses a user that can write
//!    unless the person running it says otherwise.
//!
//! The session also never blocks on a writer: `SET ISOLATION TO COMMITTED READ LAST COMMITTED`
//! reads the last committed version of a locked row (falling back to `DIRTY READ`, which takes no
//! locks, where the server refuses it), `SET LOCK MODE TO WAIT 10` bounds any other wait, and every
//! statement has a query timeout. A timed-out or broken connection is reported lost and not reused.
//!
//! Built without the crate feature `informix`, [`Session::connect`] fails with a message saying
//! so; nothing else changes.

use crate::InformixConnection;

/// Rows of cells as text, `None` for SQL NULL.
pub type Rows = Vec<Vec<Option<String>>>;

/// Seconds one statement may run before the driver cancels it.
pub const QUERY_TIMEOUT_SECS: usize = 120;
/// Seconds a statement waits for a lock before failing.
pub const LOCK_WAIT_SECS: u32 = 10;

/// The isolation levels tried, in order: never wait on a writer, else never lock at all.
pub const ISOLATIONS: &[&str] = &[
    "SET ISOLATION TO COMMITTED READ LAST COMMITTED",
    "SET ISOLATION TO DIRTY READ",
];

/// What the message says when the driver binding was not built in.
pub const NOT_BUILT: &str = "this ekos was built without Informix support: rebuild with \
     `--features informix` (it links unixODBC, and needs the IBM Informix Client SDK or the IBM \
     Data Server Driver at run time)";

/// Which of the read-only locks this session holds, for the write check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Guards {
    /// Autocommit is off and every statement is rolled back (a logged database).
    pub transactions: bool,
    /// Every transaction starts with `SET TRANSACTION READ ONLY`.
    pub read_only_transactions: bool,
    /// The isolation statement the server accepted, if any.
    pub isolation: Option<String>,
    /// `"name"` is an identifier, not a string, in this session ([`DELIMIDENT`] accepted).
    pub delimited_identifiers: bool,
}

/// Makes `"name"` a delimited identifier for the session, whatever the client's settings.
pub const DELIMIDENT: &str = "SET ENVIRONMENT DELIMIDENT 'on'";

/// An I/O failure or a timeout leaves the connection unusable; a refused statement does not.
pub(crate) fn session_lost(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("timed out")
        || m.contains("timeout")
        || m.contains("communication link")
        || m.contains("connection reset")
        || m.contains("broken pipe")
        || m.contains("-25582")
        || m.contains("network connection")
        || m.contains("rollback failed")
}

/// The password, read from the variable `connect` names; never echoed.
pub(crate) fn password(conn: &InformixConnection) -> Result<String, String> {
    std::env::var(&conn.password_env).map_err(|_| {
        format!(
            "the environment variable {} names this database's password but is not set",
            conn.password_env
        )
    })
}

/// A driver message with the password taken out, should a driver ever quote it.
#[cfg_attr(not(feature = "informix"), allow(dead_code))]
pub(crate) fn scrub(message: &str, password: &str) -> String {
    let m = message.trim().to_string();
    if password.len() >= 3 {
        m.replace(password, "***")
    } else {
        m
    }
}

/// What a length indicator that started at 0 says after a fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reported {
    Null,
    NoTotal,
    Length(usize),
}

/// A driver with a 32-bit `SQLLEN` writes `SQL_NULL_DATA` (-1) and `SQL_NO_TOTAL` (-4) into the
/// low four bytes only: read from an indicator that started at 0, they are `u32::MAX` and
/// `u32::MAX - 3`. No real length comes near either (cells are capped far below).
#[cfg_attr(not(feature = "informix"), allow(dead_code))]
pub(crate) fn reported(length: usize) -> Reported {
    match u32::try_from(length) {
        Ok(u32::MAX) => Reported::Null,
        Ok(l) if l == u32::MAX - 3 => Reported::NoTotal,
        _ => Reported::Length(length),
    }
}

#[cfg(feature = "informix")]
mod driver {
    use super::{
        DELIMIDENT, Guards, ISOLATIONS, InformixConnection, LOCK_WAIT_SECS, QUERY_TIMEOUT_SECS,
        Reported, Rows, password, reported, scrub, session_lost,
    };
    use odbc_api::buffers::{ColumnarBuffer, Indicator, TextColumn, TextRowSet};
    use odbc_api::{Connection, ConnectionOptions, Cursor, ResultSetMetadata};

    /// Longest text a cell may hold; a longer one is an error, never silently cut.
    const MAX_CELL: usize = 32_768;

    /// One ODBC connection. Blocking; use it off any async runtime.
    pub struct Session {
        conn: Connection<'static>,
        guards: Guards,
        password: String,
        lost: bool,
    }

    impl Session {
        /// Connect and apply the session guards.
        pub fn connect(conn: &InformixConnection, run_id: &str) -> Result<Session, String> {
            let password = password(conn)?;
            let env = odbc_api::environment().map_err(|e| format!("ODBC environment: {e}"))?;
            let driver = conn.driver_with(&|k| std::env::var(k).ok());
            let c = env
                .connect_with_connection_string(
                    &conn.connection_string(&driver, &password),
                    ConnectionOptions {
                        login_timeout_sec: Some(30),
                        ..Default::default()
                    },
                )
                .map_err(|e| {
                    format!(
                        "cannot sign in to {} (driver {driver}, run {run_id}): {}",
                        conn.label(),
                        scrub(&e.to_string(), &password)
                    )
                })?;
            let mut s = Session {
                conn: c,
                guards: Guards::default(),
                password,
                lost: false,
            };
            s.guards.transactions = s.conn.set_autocommit(false).is_ok();
            s.execute_quietly(&format!("SET LOCK MODE TO WAIT {LOCK_WAIT_SECS}"))?;
            // Double-quoted names must be identifiers, not strings: the profile quotes with `"`.
            // The Client SDK also takes `DELIMIDENT=y` in the connection string; over DRDA only
            // the session can ask (the value is `'on'`; `'y'` is refused here).
            s.guards.delimited_identifiers = s.execute_quietly(DELIMIDENT).is_ok();
            for isolation in ISOLATIONS {
                if s.execute_quietly(isolation).is_ok() {
                    s.guards.isolation = Some(isolation.to_string());
                    break;
                }
            }
            s.end_transaction()?;
            if s.guards.transactions {
                s.guards.read_only_transactions =
                    s.execute_quietly("SET TRANSACTION READ ONLY").is_ok();
                s.end_transaction()?;
            }
            Ok(s)
        }

        /// The locks this session holds.
        pub fn guards(&self) -> Guards {
            self.guards.clone()
        }

        /// Whether an earlier failure left the connection unusable.
        pub fn is_lost(&self) -> bool {
            self.lost
        }

        fn execute_quietly(&self, sql: &str) -> Result<(), String> {
            self.conn
                .execute(sql, (), Some(QUERY_TIMEOUT_SECS))
                .map(|_| ())
                .map_err(|e| scrub(&e.to_string(), &self.password))
        }

        fn end_transaction(&self) -> Result<(), String> {
            if self.guards.transactions {
                self.conn.rollback().map_err(|e| {
                    format!("rollback failed: {}", scrub(&e.to_string(), &self.password))
                })?;
            }
            Ok(())
        }

        /// One statement in its own read-only transaction, always rolled back: column names and
        /// at most `max_rows` rows (all rows when `None`), and whether more were left unread.
        pub fn query_limited(
            &mut self,
            sql: &str,
            max_rows: Option<usize>,
        ) -> Result<(Vec<String>, Rows, bool), String> {
            if self.lost {
                return Err("the session was lost after an earlier failure".into());
            }
            let answer = (|| {
                if self.guards.read_only_transactions {
                    self.execute_quietly("SET TRANSACTION READ ONLY")?;
                }
                self.fetch(sql, max_rows)
            })();
            let rollback = self.end_transaction();
            let result = match (answer, rollback) {
                (Ok(a), Ok(())) => Ok(a),
                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
            };
            if let Err(e) = &result
                && session_lost(e)
            {
                self.lost = true;
            }
            result
        }

        /// Fetches one row at a time into buffers whose indicators start at 0, rebinding before
        /// every fetch. A driver built with a 32-bit `SQLLEN` — the IBM Data Server Driver's
        /// `libdb2.so` under unixODBC is one — writes only the low four bytes of each indicator;
        /// from 0 those still read right ([`reported`]), where `odbc-api`'s own buffers start at
        /// -1 and the result reads as a negative length. One row per fetch also keeps such a
        /// driver from striding through an indicator array with the wrong width.
        fn fetch(
            &self,
            sql: &str,
            max_rows: Option<usize>,
        ) -> Result<(Vec<String>, Rows, bool), String> {
            let err = |e: odbc_api::Error| scrub(&e.to_string(), &self.password);
            let Some(mut cursor) = self
                .conn
                .execute(sql, (), Some(QUERY_TIMEOUT_SECS))
                .map_err(err)?
            else {
                return Ok((Vec::new(), Vec::new(), false));
            };
            let columns: Vec<String> = cursor
                .column_names()
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
            let n = u16::try_from(columns.len()).map_err(|_| "too many columns".to_string())?;
            let fresh = || -> TextRowSet {
                ColumnarBuffer::new(
                    (1..=n)
                        .map(|i| {
                            let mut c = TextColumn::new(1, MAX_CELL);
                            c.set_value(0, Some(&[]));
                            (i, c)
                        })
                        .collect(),
                )
            };
            let mut block = cursor.bind_buffer(fresh()).map_err(err)?;
            let mut rows = Rows::new();
            let mut more = false;
            while let Some(set) = block.fetch().map_err(err)? {
                if set.num_rows() == 0 {
                    break;
                }
                if max_rows.is_some_and(|m| rows.len() >= m) {
                    more = true;
                    break;
                }
                let mut row = Vec::with_capacity(columns.len());
                for (c, name) in columns.iter().enumerate() {
                    let length = match set.indicator_at(c, 0) {
                        Indicator::Null => Reported::Null,
                        Indicator::NoTotal => Reported::NoTotal,
                        Indicator::Length(l) => reported(l),
                    };
                    row.push(match length {
                        Reported::Null => None,
                        Reported::Length(l) if l > MAX_CELL => {
                            return Err(format!(
                                "a value of {l} bytes in column {name} is longer than \
                                 {MAX_CELL}; not read rather than cut"
                            ));
                        }
                        Reported::NoTotal | Reported::Length(_) => set
                            .at(c, 0)
                            .map(|b| String::from_utf8_lossy(b).into_owned()),
                    });
                }
                rows.push(row);
                let (cursor, _) = block.unbind().map_err(err)?;
                block = cursor.bind_buffer(fresh()).map_err(err)?;
            }
            Ok((columns, rows, more))
        }

        /// Column names and every row.
        pub fn query(&mut self, sql: &str) -> Result<(Vec<String>, Rows), String> {
            self.query_limited(sql, None).map(|(c, r, _)| (c, r))
        }

        /// The rows only.
        pub fn rows(&mut self, sql: &str) -> Result<Rows, String> {
            self.query(sql).map(|(_, rows)| rows)
        }
    }
}

#[cfg(not(feature = "informix"))]
mod driver {
    use super::{Guards, InformixConnection, NOT_BUILT, Rows, password};

    /// The session of a build without the driver binding: it never connects.
    pub struct Session {
        guards: Guards,
    }

    impl Session {
        pub fn connect(conn: &InformixConnection, _run_id: &str) -> Result<Session, String> {
            // The password variable is still checked first, so a misconfiguration reads the same.
            password(conn)?;
            Err(NOT_BUILT.to_string())
        }

        pub fn guards(&self) -> Guards {
            self.guards.clone()
        }

        pub fn is_lost(&self) -> bool {
            true
        }

        pub fn query_limited(
            &mut self,
            _sql: &str,
            _max_rows: Option<usize>,
        ) -> Result<(Vec<String>, Rows, bool), String> {
            Err(NOT_BUILT.to_string())
        }

        pub fn query(&mut self, _sql: &str) -> Result<(Vec<String>, Rows), String> {
            Err(NOT_BUILT.to_string())
        }

        pub fn rows(&mut self, _sql: &str) -> Result<Rows, String> {
            Err(NOT_BUILT.to_string())
        }
    }
}

pub use driver::Session;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeout_loses_the_session_a_refusal_does_not() {
        assert!(session_lost("[unixODBC] Query timeout expired"));
        assert!(session_lost("rollback failed: communication link failure"));
        assert!(!session_lost(
            "-272: No SELECT permission for orders (sales)"
        ));
    }

    #[test]
    fn an_unset_password_variable_is_named_never_guessed() {
        let missing = InformixConnection::parse("h/d@ol", "ro", "EKOS_TEST_IFX_PW_UNSET").unwrap();
        let e = Session::connect(&missing, "t").err().unwrap();
        assert!(
            e.contains("EKOS_TEST_IFX_PW_UNSET") && e.contains("not set"),
            "{e}"
        );
    }

    #[test]
    fn a_32_bit_sqllen_driver_still_reads_right() {
        assert_eq!(reported(0xFFFF_FFFF), Reported::Null);
        assert_eq!(reported(0xFFFF_FFFC), Reported::NoTotal);
        assert_eq!(reported(12), Reported::Length(12));
        assert_eq!(reported(0), Reported::Length(0));
    }

    #[test]
    fn a_driver_message_never_carries_the_password() {
        assert_eq!(
            scrub("login failed for PWD=s3cret! ", "s3cret!"),
            "login failed for PWD=***"
        );
    }

    #[cfg(not(feature = "informix"))]
    #[test]
    fn without_the_driver_connecting_says_so() {
        // `PATH` is always set, so the password check passes and the driver check speaks.
        let c = InformixConnection::parse("h/d@ol", "ro", "PATH").unwrap();
        let e = Session::connect(&c, "t").err().unwrap();
        assert!(e.contains("--features informix"), "{e}");
    }
}

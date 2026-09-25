//! RFC 0157 — connecting to a migration source, and the session settings that make it safe to do so.
//!
//! The source is somebody's production database and EKOS is a guest there. Every setting below is a
//! guard rather than a preference, and each one is applied before the first statement of real work.

use crate::PgError;
use ekos_migrate::ConnectionRef;
use std::cell::RefCell;

/// How a source session is configured. Every field is policy, deliberately: the right timeout for a
/// 200 GB warehouse is not the right timeout for a 2 GB application database, and a constant in code
/// is a constant somebody has to patch.
#[derive(Debug, Clone)]
pub struct SessionPolicy {
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
    pub idle_in_transaction_timeout_ms: u64,
    /// Deliberately low. A profiling query has no business asking for a large sort buffer on a
    /// server that is also serving traffic.
    pub work_mem: String,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            statement_timeout_ms: 30_000,
            lock_timeout_ms: 5_000,
            idle_in_transaction_timeout_ms: 60_000,
            work_mem: "16MB".into(),
        }
    }
}

/// The result of asking the server whether this role can actually write.
///
/// A role that *can* write is reported, not silently used: `default_transaction_read_only` is a
/// session setting, and a session setting is one `SET` away from being undone by anything sharing
/// the connection. The real guarantee is a role without write privileges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteCheck {
    /// The server refused the write. The guarantee EKOS wants.
    RefusedByServer { message: String },
    /// The write succeeded and was rolled back. Usable, but it is a finding.
    RoleCanWrite,
}

impl WriteCheck {
    pub fn is_safe(&self) -> bool {
        matches!(self, Self::RefusedByServer { .. })
    }
}

/// A live, read-only PostgreSQL session.
///
/// `RefCell<Client>`: [`ekos_migrate_validate::EngineReader`] takes `&self`, because a reader is a
/// *read* path and forcing `&mut` through every tier would be noise. `postgres::Client` needs
/// `&mut` to run a query, so the mutability is confined here. The type is deliberately not `Sync`
/// — one session, one thread. Chunk parallelism (RFC 0160) gets a pool of separate sessions on
/// separate threads rather than sharing one.
pub struct PgSource {
    client: RefCell<postgres::Client>,
    label: String,
}

impl PgSource {
    /// Connect and apply the session guards.
    ///
    /// `run_id` becomes part of `application_name`, so a DBA watching `pg_stat_activity` during an
    /// unexpected load can attribute every statement to one `ekos migrate` invocation without having
    /// to ask anyone.
    pub fn connect(
        conn: &ConnectionRef,
        host: &str,
        port: u16,
        user: &str,
        run_id: &str,
        policy: &SessionPolicy,
    ) -> Result<Self, PgError> {
        let password = resolve_secret(conn)?;
        let mut cfg = postgres::Config::new();
        cfg.host(host)
            .port(port)
            .user(user)
            .dbname(&conn.database)
            .application_name(&format!("ekos-migrate/{run_id}"));
        if let Some(pw) = &password {
            cfg.password(pw);
        }

        let mut client = cfg
            .connect(postgres::NoTls)
            .map_err(|e| PgError::Connect(describe(&e)))?;

        // Order matters: read-only first, so that even a mistake in the statements below cannot
        // write. `SET` is not parameterizable, so the values are validated rather than bound —
        // see `guard_setting_value`.
        for (setting, value) in [
            ("default_transaction_read_only", "on".to_string()),
            (
                "statement_timeout",
                format!("{}", policy.statement_timeout_ms),
            ),
            ("lock_timeout", format!("{}", policy.lock_timeout_ms)),
            (
                "idle_in_transaction_session_timeout",
                format!("{}", policy.idle_in_transaction_timeout_ms),
            ),
            ("work_mem", policy.work_mem.clone()),
        ] {
            guard_setting_value(&value)?;
            client
                .batch_execute(&format!("SET {setting} = '{value}'"))
                .map_err(|e| PgError::Session {
                    setting: setting.to_string(),
                    message: describe(&e),
                })?;
        }

        Ok(Self {
            client: RefCell::new(client),
            label: format!("postgres:{}/{}", conn.alias, conn.database),
        })
    }

    /// Ask the server whether this **role** can write, by trying and rolling back.
    ///
    /// The probe deliberately turns `transaction_read_only` **off** for the duration. Leaving it on
    /// would make every role look safe, which defeats the purpose: the session setting is one `SET`
    /// away from being undone, and the guarantee EKOS actually wants is a role without write
    /// privileges. A probe that cannot tell those apart is decoration.
    ///
    /// Everything happens inside a transaction that is always rolled back, so a role that *can*
    /// write leaves nothing behind — asserted by a test.
    pub fn check_write_refused(&self) -> Result<WriteCheck, PgError> {
        let mut c = self.client.borrow_mut();
        let mut tx = c.transaction().map_err(|e| PgError::Query {
            sql: "BEGIN".into(),
            message: describe(&e),
        })?;
        // If this fails the session is locked read-only at a level the probe cannot lift, which is
        // itself a safe answer.
        if let Err(e) = tx.batch_execute("SET LOCAL transaction_read_only = off") {
            return Ok(WriteCheck::RefusedByServer {
                message: describe(&e),
            });
        }
        let attempt = tx.batch_execute("CREATE TABLE ekos_migrate_write_probe (x int)");
        let verdict = match attempt {
            Err(e) => WriteCheck::RefusedByServer {
                message: describe(&e),
            },
            Ok(()) => WriteCheck::RoleCanWrite,
        };
        tx.rollback().map_err(|e| PgError::Query {
            sql: "ROLLBACK".into(),
            message: describe(&e),
        })?;
        Ok(verdict)
    }

    /// Replica lag in seconds, or `None` on a primary.
    ///
    /// Read *before* a run and recorded on the run fact, so a later reader knows which source state
    /// was compared — and so a run against a lagging replica can be refused rather than producing
    /// divergences that are really just lag.
    pub fn replica_lag_seconds(&self) -> Result<Option<f64>, PgError> {
        let rows = self.raw_query(
            "SELECT pg_is_in_recovery(), \
             COALESCE(EXTRACT(EPOCH FROM (now() - pg_last_xact_replay_timestamp())), 0)",
        )?;
        let row = rows.first().ok_or_else(|| PgError::Query {
            sql: "replica lag".into(),
            message: "no row".into(),
        })?;
        if row.first().map(String::as_str) == Some("t") {
            Ok(row.get(1).and_then(|v| v.parse().ok()))
        } else {
            Ok(None)
        }
    }

    /// Refuse to proceed if this session is a replica lagging beyond `max_lag_seconds`.
    ///
    /// Called before a profiling or validation run, not during: a run started against a lagging
    /// replica produces divergences that are really just lag, and those are the most expensive kind
    /// of false positive because they look exactly like real ones. A primary always passes.
    ///
    /// Returns the observed lag so the caller can record it on the run fact alongside the LSN —
    /// "which source state did we compare?" needs both.
    pub fn guard_replica_lag(&self, max_lag_seconds: f64) -> Result<Option<f64>, PgError> {
        match self.replica_lag_seconds()? {
            None => Ok(None),
            Some(lag) if lag <= max_lag_seconds => Ok(Some(lag)),
            Some(lag) => Err(PgError::ReplicaLagTooHigh {
                lag_seconds: lag,
                max_seconds: max_lag_seconds,
            }),
        }
    }

    /// The write-ahead LSN this session is reading at. Recorded on a validation run so "which source
    /// state did we compare?" has an exact answer afterwards (RFC 0156).
    pub fn current_lsn(&self) -> Result<String, PgError> {
        let rows = self.raw_query(
            "SELECT CASE WHEN pg_is_in_recovery() \
             THEN pg_last_wal_replay_lsn()::text ELSE pg_current_wal_lsn()::text END",
        )?;
        rows.first()
            .and_then(|r| r.first())
            .cloned()
            .ok_or_else(|| PgError::Query {
                sql: "current lsn".into(),
                message: "no row".into(),
            })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Run a query and render every column as text — the shape every tier and the catalog reader
    /// consume.
    ///
    /// Uses the **simple query protocol** (`simple_query`), not the extended one. The extended
    /// protocol negotiates a binary format per type, so `client.query(...)` hands back an `int8`
    /// that a `String` decoder simply refuses; an implementation that swallows that refusal renders
    /// every numeric column as an empty string and every `count(*)` as nothing. The simple protocol
    /// returns every value in its text form, which is also the form RFC 0155's canonical
    /// expressions are written against, so there is one rendering rather than two.
    ///
    /// **NULL comes back as an empty string here.** That is safe only because every query this
    /// crate issues renders NULL explicitly server-side — the canonical form coalesces it to the
    /// sentinel, and the catalog reads use `COALESCE`. A caller that writes a raw query returning a
    /// bare NULL will not be able to tell it from `''`, which is why callers do not write raw
    /// queries.
    ///
    /// The simple protocol also permits multiple statements in one message. Nothing here builds SQL
    /// from user text, and RFC 0160's statement classifier is the control for anything that does.
    pub fn raw_query(&self, sql: &str) -> Result<Vec<Vec<String>>, PgError> {
        use postgres::SimpleQueryMessage;
        let mut c = self.client.borrow_mut();
        let messages = c.simple_query(sql).map_err(|e| PgError::Query {
            sql: sql.to_string(),
            message: describe(&e),
        })?;
        Ok(messages
            .into_iter()
            .filter_map(|m| match m {
                SimpleQueryMessage::Row(row) => Some(
                    (0..row.len())
                        .map(|i| row.get(i).unwrap_or_default().to_string())
                        .collect(),
                ),
                _ => None,
            })
            .collect())
    }
}

/// `postgres::Error`'s own `Display` is "db error" — the server's message lives on the source.
/// Rendering only the outer error throws away the one part a human needs, and it is exactly the
/// part a test asserts on.
pub(crate) fn describe(e: &postgres::Error) -> String {
    use std::error::Error;
    match e.source() {
        Some(src) => format!("{e}: {src}"),
        None => e.to_string(),
    }
}

/// A `SET` value cannot be parameterized, so it is validated instead.
///
/// The values here are all EKOS's own policy rather than user input, but "it is our own input" is
/// how injection holes are argued into existence — the check costs nothing and survives a future
/// caller who plumbs this to a config file.
fn guard_setting_value(value: &str) -> Result<(), PgError> {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        Ok(())
    } else {
        Err(PgError::UnsafeSetting(value.to_string()))
    }
}

/// Read the password from the environment variable the `ConnectionRef` *names*.
///
/// The variable's name is a ledger fact; its value never is, and never passes through a connection
/// string either (RFC 0154 refuses a DSN carrying credentials outright).
fn resolve_secret(conn: &ConnectionRef) -> Result<Option<String>, PgError> {
    match &conn.secret_env {
        None => Ok(None),
        Some(var) => match std::env::var(var) {
            Ok(v) => Ok(Some(v)),
            Err(_) => Err(PgError::MissingSecret(var.clone())),
        },
    }
}

impl ekos_migrate_validate::EngineReader for PgSource {
    fn dialect(&self) -> ekos_migrate_validate::Dialect {
        ekos_migrate_validate::Dialect::Postgres
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>, ekos_migrate_validate::ReadError> {
        self.raw_query(sql)
            .map_err(|e| ekos_migrate_validate::ReadError::Query {
                engine: self.label.clone(),
                message: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_value_with_sql_in_it_is_refused() {
        assert!(guard_setting_value("16MB").is_ok());
        assert!(guard_setting_value("30000").is_ok());
        assert!(guard_setting_value("on").is_ok());
        for bad in ["16MB'; DROP TABLE x --", "a b", "'", "\\"] {
            assert!(guard_setting_value(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn a_named_but_absent_secret_is_an_error_not_an_empty_password() {
        let conn = ConnectionRef::parse("postgres://h/db")
            .unwrap()
            .with_secret_env(Some("EKOS_TEST_DEFINITELY_UNSET_VAR".into()));
        assert!(matches!(
            resolve_secret(&conn),
            Err(PgError::MissingSecret(_))
        ));
    }

    #[test]
    fn no_secret_named_means_no_password_not_a_failure() {
        let conn = ConnectionRef::parse("postgres://h/db").unwrap();
        assert_eq!(resolve_secret(&conn).unwrap(), None);
    }

    #[test]
    fn only_a_server_refusal_counts_as_safe() {
        assert!(
            WriteCheck::RefusedByServer {
                message: "x".into()
            }
            .is_safe()
        );
        assert!(!WriteCheck::RoleCanWrite.is_safe());
    }
}

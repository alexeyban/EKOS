//! RFC 0164 — a writable PostgreSQL **sandbox** session, for V5 fixtures.
//!
//! Everything else in this crate is a guest on somebody's production database and sets
//! `default_transaction_read_only` first. This is the one path that writes, so it is a separate
//! type that cannot be obtained from a [`crate::PgSource`], and its caller must have checked that
//! the connection is marked as a sandbox.
//!
//! It writes nothing that lasts: every [`PgSandbox::run`] is one transaction in a schema of its
//! own, and the transaction is always rolled back — also when a statement fails.

use crate::PgError;
use crate::session::describe;
use ekos_migrate::ConnectionRef;
use std::cell::RefCell;

pub struct PgSandbox {
    client: RefCell<postgres::Client>,
    label: String,
    schema: String,
}

impl PgSandbox {
    /// Connect. `run_id` names the session in `pg_stat_activity` and the fixture schema.
    pub fn connect(
        conn: &ConnectionRef,
        host: &str,
        port: u16,
        user: &str,
        run_id: &str,
    ) -> Result<Self, PgError> {
        let mut cfg = postgres::Config::new();
        cfg.host(host)
            .port(port)
            .user(user)
            .dbname(&conn.database)
            .application_name(&format!("ekos-migrate-v5/{run_id}"));
        if let Some(pw) = crate::session::resolve_secret(conn)? {
            cfg.password(&pw);
        }
        let client = cfg
            .connect(postgres::NoTls)
            .map_err(|e| PgError::Connect(describe(&e)))?;
        // Only what cannot break out of an identifier.
        let suffix: String = run_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(24)
            .collect();
        Ok(Self {
            client: RefCell::new(client),
            label: format!("postgres-sandbox:{}/{}", conn.alias, conn.database),
            schema: format!("ekos_v5_{}", suffix.to_ascii_lowercase()),
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Run `setup` then each of `queries` in one transaction, inside a fresh schema, and roll it
    /// back. One result set per query; SQL `NULL` is `""`.
    pub fn run(
        &self,
        setup: &[String],
        queries: &[String],
    ) -> Result<Vec<Vec<Vec<String>>>, PgError> {
        use postgres::SimpleQueryMessage;
        let mut c = self.client.borrow_mut();
        let mut exec = |sql: &str| {
            c.simple_query(sql).map_err(|e| PgError::Query {
                sql: sql.to_string(),
                message: describe(&e),
            })
        };
        exec("BEGIN")?;
        let work = (|| {
            exec(&format!("CREATE SCHEMA {}", self.schema))?;
            exec(&format!("SET LOCAL search_path TO {}", self.schema))?;
            exec("SET LOCAL statement_timeout = '30s'")?;
            for s in setup {
                exec(s)?;
            }
            let mut sets = Vec::new();
            for q in queries {
                sets.push(
                    exec(q)?
                        .into_iter()
                        .filter_map(|m| match m {
                            SimpleQueryMessage::Row(row) => Some(
                                (0..row.len())
                                    .map(|i| row.get(i).unwrap_or_default().to_string())
                                    .collect(),
                            ),
                            _ => None,
                        })
                        .collect(),
                );
            }
            Ok(sets)
        })();
        // Always, and its own failure outranks nothing: a dropped connection rolls back too.
        let _ = exec("ROLLBACK");
        work
    }
}

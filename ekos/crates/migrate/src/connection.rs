//! RFC 0154 / RFC 0157 — connection references and environments.
//!
//! **The ledger never sees a credential.** A [`ConnectionRef`] carries the engine, a host *alias*,
//! a database name and the *name of* the secret that holds the password — never the password. The
//! secret is resolved at connection time, from the process environment, and the resolved value is
//! never written anywhere.

use std::fmt;
use std::str::FromStr;

/// Which engine a connection points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Postgres,
    ClickHouse,
    Delta,
}

impl EngineKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::ClickHouse => "clickhouse",
            Self::Delta => "delta",
        }
    }

    /// `true` if this engine may be a migration *source*. Today only PostgreSQL; the check exists
    /// so `ekos migrate init --source clickhouse://…` fails at parse time rather than at first
    /// query.
    pub fn can_be_source(self) -> bool {
        matches!(self, Self::Postgres)
    }

    pub fn can_be_target(self) -> bool {
        matches!(self, Self::ClickHouse | Self::Delta)
    }
}

impl fmt::Display for EngineKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EngineKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "postgres" | "postgresql" | "pg" => Self::Postgres,
            "clickhouse" | "ch" => Self::ClickHouse,
            "delta" | "spark" | "databricks" => Self::Delta,
            other => return Err(format!("unknown engine: {other}")),
        })
    }
}

/// Sandbox, staging or production. RFC 0154 makes this first-class because risk is computed from
/// `(statement class, environment)` — the same `DROP TABLE` is R1 in a sandbox and R4 in
/// production, with no second code path.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Sandbox,
    Staging,
    Production,
}

impl Environment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sandbox => "sandbox",
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }

    /// `true` where a write needs an explicit approval before the executor may resolve credentials
    /// at all (RFC 0160). Sandbox writes are logged, not gated.
    pub fn requires_approval_to_write(self) -> bool {
        !matches!(self, Self::Sandbox)
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Environment {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "sandbox" => Self::Sandbox,
            "staging" => Self::Staging,
            "production" | "prod" => Self::Production,
            other => return Err(format!("unknown environment: {other}")),
        })
    }
}

/// A connection, as it is safe to persist.
///
/// Constructed from a DSN-shaped string of the form `engine://alias/database`. A DSN carrying
/// userinfo (`postgres://user:pw@host/db`) is **rejected**, rather than parsed and stripped: a
/// password that reached this process's argv has already leaked into shell history and process
/// listings, and silently accepting it would teach the habit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionRef {
    pub engine: EngineKind,
    /// A host *alias*, resolved from configuration or the environment — not a hostname with
    /// credentials attached.
    pub alias: String,
    pub database: String,
    /// The name of the environment variable holding the secret, e.g. `EKOS_MIGRATE_PG_PASSWORD`.
    /// The value is never read here and never persisted.
    pub secret_env: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConnectionError {
    #[error("expected <engine>://<alias>/<database>, got: {0}")]
    Malformed(String),
    #[error("unknown engine in {dsn}: {msg}")]
    Engine { dsn: String, msg: String },
    #[error(
        "credentials must never appear in a connection string. Put the password in an environment \
         variable and pass its name with --secret-env instead: {0}"
    )]
    EmbeddedCredentials(String),
    #[error("{engine} cannot be a migration {role}")]
    WrongRole {
        engine: EngineKind,
        role: &'static str,
    },
}

impl ConnectionRef {
    /// Parse `engine://alias/database`.
    pub fn parse(dsn: &str) -> Result<Self, ConnectionError> {
        let (engine, rest) = dsn
            .split_once("://")
            .ok_or_else(|| ConnectionError::Malformed(dsn.to_string()))?;
        let engine: EngineKind = engine.parse().map_err(|msg| ConnectionError::Engine {
            dsn: dsn.to_string(),
            msg,
        })?;
        if rest.contains('@') {
            return Err(ConnectionError::EmbeddedCredentials(dsn.to_string()));
        }
        let (alias, database) = rest
            .split_once('/')
            .ok_or_else(|| ConnectionError::Malformed(dsn.to_string()))?;
        if alias.is_empty() || database.is_empty() {
            return Err(ConnectionError::Malformed(dsn.to_string()));
        }
        Ok(Self {
            engine,
            alias: alias.to_string(),
            database: database.to_string(),
            secret_env: None,
        })
    }

    pub fn with_secret_env(mut self, var: Option<String>) -> Self {
        self.secret_env = var;
        self
    }

    pub fn require_source(self) -> Result<Self, ConnectionError> {
        if self.engine.can_be_source() {
            Ok(self)
        } else {
            Err(ConnectionError::WrongRole {
                engine: self.engine,
                role: "source",
            })
        }
    }

    pub fn require_target(self) -> Result<Self, ConnectionError> {
        if self.engine.can_be_target() {
            Ok(self)
        } else {
            Err(ConnectionError::WrongRole {
                engine: self.engine,
                role: "target",
            })
        }
    }

    /// The stable display form, and the form persisted on the `MigrationConnectionRef` object.
    /// Contains no secret by construction.
    pub fn dsn(&self) -> String {
        format!("{}://{}/{}", self.engine, self.alias, self.database)
    }
}

impl fmt::Display for ConnectionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.dsn())
    }
}

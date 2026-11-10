//! RFC 0252 §3.5 — SQL Server as a live database source (`[[databases]] kind = "sqlserver"`).
//!
//! One `[[databases]]` entry becomes one observer. A scan connects through `tiberius` on a
//! read-only session ([`session`]: `ApplicationIntent=ReadOnly`, every statement inside a
//! transaction that is always rolled back, lock and command timeouts), reads the listed schemas'
//! catalog from the `sys.*` views ([`catalog`]), reconciles it against `sys.objects`' own counts
//! (a short catalog is an error, RFC 0157), profiles it at the configured tier ([`profile`]), asks
//! whether the login could write, and emits RFC 0211's artifact shape
//! (`ekos_observation_sdk::live_catalog`) — the same as the PostgreSQL plugin's — so the shared
//! recover pass compiles `Table` / `View` / `Procedure` / `Trigger` objects with `origin: live`,
//! `database` and `owner`.
//!
//! The driver is async but the session is blocking (its own single-threaded runtime), like
//! `ekos-pg-live`: the observer runs the read on a blocking thread.

use async_trait::async_trait;
use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::live_catalog::{self, OwnerCatalog};
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde_json::{Value, json};
use std::sync::Arc;

pub mod catalog;
pub mod probe;
pub mod profile;
pub mod session;

pub use ekos_observation_sdk::profile::ProfileTier;

/// The observer's name, and the `source_kind` its artifacts carry.
pub const OBSERVER: &str = "sqlserver";
/// The target scheme: `sqlserver://<database>/<schema>/…`.
pub const SCHEME: &str = "sqlserver";
/// The query that asks the server to trust a self-signed certificate.
const TRUST_OPTION: &str = "trust-server-certificate=true";

/// One database's connection: the password stays an environment variable's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlServerConnection {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_env: String,
    /// Accept the server's certificate without checking it (a self-signed certificate, as a
    /// fresh SQL Server install has). Off unless `connect` ends in
    /// `?trust-server-certificate=true`; the connection is encrypted either way.
    pub trust_server_certificate: bool,
}

impl SqlServerConnection {
    /// `host[:port]/database[?trust-server-certificate=true]`; the port defaults to 1433. A
    /// named instance is reached by its port (the SQL Browser service is not used).
    pub fn parse(connect: &str, user: &str, password_env: &str) -> Result<Self, String> {
        let (connect_main, trust) = match connect.split_once('?') {
            Some((main, TRUST_OPTION)) => (main, true),
            Some((_, other)) => {
                return Err(format!(
                    "connect accepts only ?{TRUST_OPTION} after the database, not ?{other}"
                ));
            }
            None => (connect, false),
        };
        let (address, database) = connect_main
            .split_once('/')
            .filter(|(a, d)| !a.is_empty() && !d.is_empty() && !d.contains('/'))
            .ok_or_else(|| format!("connect is host[:port]/database, not {connect:?}"))?;
        let (host, port) = match address.rsplit_once(':') {
            Some((h, p)) => (
                h,
                p.parse::<u16>()
                    .map_err(|_| format!("connect {connect:?}: {p:?} is not a port"))?,
            ),
            None => (address, 1433),
        };
        if host.contains('\\') {
            return Err(format!(
                "connect {connect:?}: reach a named instance by its port (host:port/database)"
            ));
        }
        Ok(Self {
            host: host.to_string(),
            port,
            database: database.to_string(),
            user: user.to_string(),
            password_env: password_env.to_string(),
            trust_server_certificate: trust,
        })
    }

    pub fn label(&self) -> String {
        format!(
            "sqlserver:{}@{}:{}/{}",
            self.user, self.host, self.port, self.database
        )
    }
}

/// What one scan read.
#[derive(Debug, Clone)]
pub struct DatabaseReading {
    pub catalogs: Vec<OwnerCatalog>,
    pub write_check: Option<Value>,
}

/// Where a reading comes from: the live database, or a fixed reading in tests.
pub trait CatalogReader: Send + Sync {
    fn read(&self) -> Result<DatabaseReading, String>;
}

/// Reads a live database.
pub struct LiveReader {
    pub connection: SqlServerConnection,
    pub schemas: Vec<String>,
    pub run_id: String,
    pub redaction: RedactionConfig,
    /// RFC 0211 §3.4: how much the read measures beyond the row estimates.
    pub profile: ProfileTier,
}

impl CatalogReader for LiveReader {
    fn read(&self) -> Result<DatabaseReading, String> {
        read(
            &self.connection,
            &self.schemas,
            &self.run_id,
            &self.redaction,
            self.profile,
        )
    }
}

/// Connect, read and reconcile the schemas' catalog, profile it at `tier`, and check whether the
/// login could write. Blocking: call it off any async runtime.
pub fn read(
    connection: &SqlServerConnection,
    schemas: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
    tier: ProfileTier,
) -> Result<DatabaseReading, String> {
    let mut session = session::Session::connect(connection, run_id)?;
    let mut rows = catalog::CatalogRows::new();
    for (name, sql) in catalog::queries(schemas) {
        let r = session
            .rows(&sql)
            .map_err(|e| format!("reading the catalog ({name}): {e}"))?;
        rows.insert(name.to_string(), r);
    }
    let mut catalogs = catalog::normalize(schemas, &rows, redaction);
    catalog::reconcile(&catalogs)?;
    profile::estimates(&mut catalogs, schemas, &mut |sql: &str| session.rows(sql))
        .map_err(|e| format!("reading row estimates: {e}"))?;
    if tier != ProfileTier::P0 {
        for c in &mut catalogs {
            let started = std::time::Instant::now();
            let stats = profile::scan(c, tier, redaction, &mut |sql: &str| session.rows(sql));
            let mut line = json!({ "owner": c.owner, "tier": tier.as_str() });
            line["counts"] = serde_json::to_value(&stats).unwrap_or_default();
            line["ms"] = (started.elapsed().as_millis() as u64).into();
            ekos_common::run_log::emit("sqlserver.profile", "profiled", &line);
            c.profile = Some(stats);
            if session.is_lost() {
                // A later owner would only be skipped table by table; reconnect once instead.
                session = session::Session::connect(connection, run_id)?;
            }
        }
    }
    let check = session
        .rows(&catalog::write_check(schemas))
        .map_err(|e| format!("checking the login's rights: {e}"))?;
    let write_check = check.first().map(|r| catalog::write_check_result(r));
    Ok(DatabaseReading {
        catalogs,
        write_check,
    })
}

/// RFC 0223 / 0235: what `ekos database test` reports — the same JSON for every kind.
pub fn test_connection(
    connection: &SqlServerConnection,
    schemas: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
) -> Result<Value, String> {
    let reading = read(connection, schemas, run_id, redaction, ProfileTier::P0)?;
    Ok(test_report(&connection.label(), &reading))
}

/// The test report of one reading. Pure.
pub fn test_report(label: &str, reading: &DatabaseReading) -> Value {
    json!({
        "label": label,
        "family": catalog::FAMILY,
        "owners": reading.catalogs.iter().map(|c| json!({
            "owner": c.owner,
            "census": c.census,
            "listed": c.listed(),
            "columns": c.columns,
        })).collect::<Vec<_>>(),
        "write_check": reading.write_check,
        "any_privileges": [],
    })
}

/// One `[[databases]]` entry of kind `sqlserver`.
pub struct SqlServerObserver {
    database: String,
    reader: Arc<dyn CatalogReader>,
}

impl SqlServerObserver {
    pub fn new(database: impl Into<String>, reader: Arc<dyn CatalogReader>) -> Self {
        Self {
            database: database.into(),
            reader,
        }
    }
}

#[async_trait]
impl Observer for SqlServerObserver {
    fn name(&self) -> &str {
        OBSERVER
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let reader = Arc::clone(&self.reader);
        let reading = tokio::task::spawn_blocking(move || reader.read())
            .await
            .map_err(|e| ObserveError::connector(format!("sqlserver reader stopped: {e}")))?
            .map_err(|e| {
                ObserveError::connector(format!("sqlserver database {}: {e}", self.database))
            })?;
        let mut pkg = ObservationPackage::new(OBSERVER, "database");
        for a in live_catalog::artifacts(
            OBSERVER,
            "ekos-plugin-sqlserver",
            SCHEME,
            &self.database,
            &reading.catalogs,
            reading.write_check.as_ref(),
        ) {
            pkg.push(a);
        }
        let objects: usize = reading.catalogs.iter().map(|c| c.objects.len()).sum();
        tracing::info!(database = %self.database, schemas = reading.catalogs.len(), objects, "sqlserver catalog read");
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_is_host_port_database_with_an_explicit_trust_option() {
        let c = SqlServerConnection::parse("db.local:14330/shop", "ro", "PW").unwrap();
        assert_eq!(
            (c.host.as_str(), c.port, c.database.as_str()),
            ("db.local", 14330, "shop")
        );
        assert!(
            !c.trust_server_certificate,
            "the certificate is checked by default"
        );
        assert_eq!(c.label(), "sqlserver:ro@db.local:14330/shop");
        let d =
            SqlServerConnection::parse("db.local/shop?trust-server-certificate=true", "ro", "PW")
                .unwrap();
        assert_eq!((d.port, d.trust_server_certificate), (1433, true));
        for bad in [
            "db.local",
            "db.local:x/shop",
            "/shop",
            "h/a/b",
            "h/shop?encrypt=false",
            "h\\SQLEXPRESS/shop",
        ] {
            assert!(
                SqlServerConnection::parse(bad, "ro", "PW").is_err(),
                "{bad}"
            );
        }
    }

    struct Fixed(DatabaseReading);
    impl CatalogReader for Fixed {
        fn read(&self) -> Result<DatabaseReading, String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn a_scan_emits_the_live_catalog_shape() {
        let catalogs = catalog::normalize(
            &["sales".into()],
            &catalog::tests_fixture(),
            &RedactionConfig::default(),
        );
        let reading = DatabaseReading {
            catalogs,
            write_check: Some(catalog::write_check_result(&[
                Some("0".into()),
                Some("0".into()),
                Some("READ_WRITE".into()),
            ])),
        };
        let pkg = SqlServerObserver::new("erp", Arc::new(Fixed(reading.clone())))
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert_eq!(pkg.artifacts[0].content.target, "sqlserver://erp/sales");
        assert!(
            pkg.artifacts
                .iter()
                .any(|a| a.content.target == "sqlserver://erp/sales/table/orders")
        );
        let report = test_report("sqlserver:ro@h:1433/erp", &reading);
        assert_eq!(report["family"], "sys");
        assert_eq!(report["owners"][0]["census"]["TABLE"]["valid"], 2);
        assert_eq!(report["write_check"]["result"], "refused_by_server");
    }
}

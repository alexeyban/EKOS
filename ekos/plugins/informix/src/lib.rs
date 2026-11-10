//! RFC 0252 §3.5 — Informix as a live database source (`[[databases]] kind = "informix"`).
//!
//! One `[[databases]]` entry becomes one observer. A scan connects over ODBC (`odbc-api` over
//! unixODBC, behind the crate feature `informix`) on a read-only session ([`session`]: autocommit
//! off and every statement rolled back, `SET TRANSACTION READ ONLY` where the server takes it, a
//! committed-read isolation that never waits on a writer, a lock wait and a query timeout), reads
//! the listed owners' catalog from the system catalog tables (`systables`, `syscolumns`,
//! `sysconstraints`, `sysindexes`, `sysreferences`, `syschecks`, `sysdefaults`, `sysviews`,
//! `sysprocedures` / `sysprocbody`, `systriggers` / `systrigbody` — [`catalog`]), reconciles it
//! against the catalog's own counts (a short catalog is an error, RFC 0157), profiles it at the
//! configured tier ([`profile`]), asks whether the user could write, and emits RFC 0211's artifact
//! shape (`ekos_observation_sdk::live_catalog`) — the same as the other kinds — so the shared
//! recover pass compiles `Table` / `View` / `Procedure` / `Trigger` objects with `origin: live`.
//!
//! Two ODBC drivers reach Informix, and both are supported (`connect`):
//!
//! - the **IBM Informix Client SDK** (CSDK) driver over SQLI, the server's native protocol:
//!   `host[:port]/database@server` (port 9088 by default; `server` is `INFORMIXSERVER`);
//! - the **IBM Data Server Driver for ODBC and CLI** over DRDA, which Informix also listens for:
//!   `drda://host[:port]/database` (port 9089 by default). It is a free download with no account,
//!   which is why the live suite runs through it; the SQL sent is the same Informix SQL.
//!
//! `?driver=<name or path>` picks the ODBC driver explicitly; `?db-locale=<locale>` sets the CSDK's
//! `DB_LOCALE` for a database whose locale is not the client default.

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
pub const OBSERVER: &str = "informix";
/// The target scheme: `informix://<database>/<owner>/…`.
pub const SCHEME: &str = "informix";
/// The ODBC driver the CSDK registers under in `odbcinst.ini`.
pub const CSDK_DRIVER_NAME: &str = "IBM INFORMIX ODBC DRIVER";
/// The ODBC driver the IBM Data Server Driver registers under in `odbcinst.ini`.
pub const DRDA_DRIVER_NAME: &str = "IBM DB2 ODBC DRIVER";

/// How the session reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The Informix Client SDK's driver over SQLI (`onsoctcp`).
    Sqli,
    /// The IBM Data Server Driver over DRDA (`drsoctcp`).
    Drda,
}

/// One database's connection: the password stays an environment variable's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InformixConnection {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub database: String,
    /// `INFORMIXSERVER`, the server's name in `sqlhosts` (SQLI only).
    pub server: Option<String>,
    /// The ODBC driver: a name in `odbcinst.ini` or a path to its shared library.
    pub driver: Option<String>,
    /// The CSDK's `DB_LOCALE`, when the database's locale is not the client default.
    pub db_locale: Option<String>,
    pub user: String,
    pub password_env: String,
}

/// A value inside an ODBC connection string: braced when it holds a `;`, `{` or `}` (with `}`
/// doubled), as the ODBC grammar says.
pub fn odbc_value(v: &str) -> String {
    if v.contains([';', '{', '}']) || v.starts_with(' ') || v.ends_with(' ') {
        format!("{{{}}}", v.replace('}', "}}"))
    } else {
        v.to_string()
    }
}

fn plain(what: &str, v: &str) -> Result<(), String> {
    let ok = !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'$'));
    if ok {
        Ok(())
    } else {
        Err(format!("{what} {v:?} is not a plain name"))
    }
}

impl InformixConnection {
    /// `host[:port]/database@server` (CSDK, SQLI, port 9088) or `drda://host[:port]/database`
    /// (Data Server Driver, port 9089), each optionally followed by `?driver=…&db-locale=…`.
    pub fn parse(connect: &str, user: &str, password_env: &str) -> Result<Self, String> {
        let (protocol, rest) = match connect.strip_prefix("drda://") {
            Some(r) => (Protocol::Drda, r),
            None => (Protocol::Sqli, connect),
        };
        let (main, query) = match rest.split_once('?') {
            Some((m, q)) => (m, Some(q)),
            None => (rest, None),
        };
        let (mut driver, mut db_locale) = (None, None);
        for pair in query.into_iter().flat_map(|q| q.split('&')) {
            match pair.split_once('=') {
                Some(("driver", v)) if !v.is_empty() && !v.contains(['{', '}', ';']) => {
                    driver = Some(v.to_string())
                }
                Some(("db-locale", v)) => {
                    plain("db-locale", v)?;
                    db_locale = Some(v.to_string());
                }
                _ => {
                    return Err(format!(
                        "connect accepts only ?driver=<name or path> and ?db-locale=<locale>, \
                         not ?{pair}"
                    ));
                }
            }
        }
        let (address, target) = main
            .split_once('/')
            .filter(|(a, d)| !a.is_empty() && !d.is_empty() && !d.contains('/'))
            .ok_or_else(|| {
                format!(
                    "connect is host[:port]/database@server (or drda://host[:port]/database), \
                     not {connect:?}"
                )
            })?;
        let (database, server) = match (protocol, target.split_once('@')) {
            (Protocol::Sqli, Some((d, s))) if !d.is_empty() && !s.is_empty() => {
                (d, Some(s.to_string()))
            }
            (Protocol::Sqli, _) => {
                return Err(format!(
                    "connect {connect:?}: name the Informix server after the database \
                     (host:port/database@server, the INFORMIXSERVER name)"
                ));
            }
            (Protocol::Drda, Some(_)) => {
                return Err(format!(
                    "connect {connect:?}: a DRDA connection names no server (drda://host:port/database)"
                ));
            }
            (Protocol::Drda, None) => (target, None),
        };
        plain("database", database)?;
        if let Some(s) = &server {
            plain("server", s)?;
        }
        let default_port = match protocol {
            Protocol::Sqli => 9088,
            Protocol::Drda => 9089,
        };
        let (host, port) = match address.rsplit_once(':') {
            Some((h, p)) => (
                h,
                p.parse::<u16>()
                    .map_err(|_| format!("connect {connect:?}: {p:?} is not a port"))?,
            ),
            None => (address, default_port),
        };
        plain("host", host)?;
        Ok(Self {
            protocol,
            host: host.to_string(),
            port,
            database: database.to_string(),
            server,
            driver,
            db_locale,
            user: user.to_string(),
            password_env: password_env.to_string(),
        })
    }

    pub fn label(&self) -> String {
        match (&self.protocol, &self.server) {
            (Protocol::Sqli, Some(s)) => format!(
                "informix:{}@{}:{}/{}@{s}",
                self.user, self.host, self.port, self.database
            ),
            _ => format!(
                "informix+drda:{}@{}:{}/{}",
                self.user, self.host, self.port, self.database
            ),
        }
    }

    /// The ODBC driver to load: the one `connect` names, else for the CSDK
    /// `$INFORMIXDIR/lib/cli/iclit09b.so` when `INFORMIXDIR` is set, else the name each driver
    /// registers in `odbcinst.ini`. `env` reads an environment variable.
    pub fn driver_with(&self, env: &dyn Fn(&str) -> Option<String>) -> String {
        if let Some(d) = &self.driver {
            return d.clone();
        }
        match self.protocol {
            Protocol::Sqli => env("INFORMIXDIR")
                .filter(|d| !d.is_empty())
                .map(|d| format!("{}/lib/cli/iclit09b.so", d.trim_end_matches('/')))
                .unwrap_or_else(|| CSDK_DRIVER_NAME.to_string()),
            Protocol::Drda => DRDA_DRIVER_NAME.to_string(),
        }
    }

    /// The ODBC connection string. Pure; never logged (it holds the password).
    pub fn connection_string(&self, driver: &str, password: &str) -> String {
        let mut parts = vec![format!("DRIVER={}", odbc_value(driver))];
        match self.protocol {
            Protocol::Sqli => {
                parts.push(format!("HOST={}", self.host));
                parts.push(format!("SERVICE={}", self.port));
                parts.push(format!(
                    "SERVER={}",
                    self.server.as_deref().unwrap_or_default()
                ));
                parts.push("PROTOCOL=onsoctcp".into());
                parts.push(format!("DATABASE={}", self.database));
                // Double-quoted names are identifiers, not strings: the profile quotes with `"`.
                parts.push("DELIMIDENT=y".into());
                parts.push("CLIENT_LOCALE=en_US.utf8".into());
                if let Some(l) = &self.db_locale {
                    parts.push(format!("DB_LOCALE={l}"));
                }
            }
            Protocol::Drda => {
                parts.push(format!("DATABASE={}", self.database));
                parts.push(format!("HOSTNAME={}", self.host));
                parts.push(format!("PORT={}", self.port));
                parts.push("PROTOCOL=TCPIP".into());
            }
        }
        parts.push(format!("UID={}", odbc_value(&self.user)));
        parts.push(format!("PWD={}", odbc_value(password)));
        parts.join(";") + ";"
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
    pub connection: InformixConnection,
    pub owners: Vec<String>,
    pub run_id: String,
    pub redaction: RedactionConfig,
    /// RFC 0211 §3.4: how much the read measures beyond the row estimates.
    pub profile: ProfileTier,
}

impl CatalogReader for LiveReader {
    fn read(&self) -> Result<DatabaseReading, String> {
        read(
            &self.connection,
            &self.owners,
            &self.run_id,
            &self.redaction,
            self.profile,
        )
    }
}

/// Connect, read and reconcile the owners' catalog, profile it at `tier`, and check whether the
/// user could write. Blocking: call it off any async runtime.
pub fn read(
    connection: &InformixConnection,
    owners: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
    tier: ProfileTier,
) -> Result<DatabaseReading, String> {
    let mut session = session::Session::connect(connection, run_id)?;
    let mut rows = catalog::CatalogRows::new();
    for (name, sql) in catalog::queries(owners) {
        let r = session
            .rows(&sql)
            .map_err(|e| format!("reading the catalog ({name}): {e}"))?;
        rows.insert(name.to_string(), r);
    }
    let mut catalogs = catalog::normalize(owners, &rows, redaction);
    catalog::reconcile(&catalogs)?;
    profile::estimates(&mut catalogs, owners, &mut |sql: &str| session.rows(sql))
        .map_err(|e| format!("reading row estimates: {e}"))?;
    if tier != ProfileTier::P0 {
        for c in &mut catalogs {
            let started = std::time::Instant::now();
            let stats = profile::scan(c, tier, redaction, &mut |sql: &str| session.rows(sql));
            let mut line = json!({ "owner": c.owner, "tier": tier.as_str() });
            line["counts"] = serde_json::to_value(&stats).unwrap_or_default();
            line["ms"] = (started.elapsed().as_millis() as u64).into();
            ekos_common::run_log::emit("informix.profile", "profiled", &line);
            c.profile = Some(stats);
            if session.is_lost() {
                // A later owner would only be skipped table by table; reconnect once instead.
                session = session::Session::connect(connection, run_id)?;
            }
        }
    }
    let check = session
        .rows(&catalog::write_check(owners))
        .map_err(|e| format!("checking the user's rights: {e}"))?;
    let write_check = check
        .first()
        .map(|r| catalog::write_check_result(r, &session.guards()));
    Ok(DatabaseReading {
        catalogs,
        write_check,
    })
}

/// RFC 0223 / 0235: what `ekos database test` reports — the same JSON for every kind.
pub fn test_connection(
    connection: &InformixConnection,
    owners: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
) -> Result<Value, String> {
    let reading = read(connection, owners, run_id, redaction, ProfileTier::P0)?;
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

/// One `[[databases]]` entry of kind `informix`.
pub struct InformixObserver {
    database: String,
    reader: Arc<dyn CatalogReader>,
}

impl InformixObserver {
    pub fn new(database: impl Into<String>, reader: Arc<dyn CatalogReader>) -> Self {
        Self {
            database: database.into(),
            reader,
        }
    }
}

#[async_trait]
impl Observer for InformixObserver {
    fn name(&self) -> &str {
        OBSERVER
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let reader = Arc::clone(&self.reader);
        let reading = tokio::task::spawn_blocking(move || reader.read())
            .await
            .map_err(|e| ObserveError::connector(format!("informix reader stopped: {e}")))?
            .map_err(|e| {
                ObserveError::connector(format!("informix database {}: {e}", self.database))
            })?;
        let mut pkg = ObservationPackage::new(OBSERVER, "database");
        for a in live_catalog::artifacts(
            OBSERVER,
            "ekos-plugin-informix",
            SCHEME,
            &self.database,
            &reading.catalogs,
            reading.write_check.as_ref(),
        ) {
            pkg.push(a);
        }
        let objects: usize = reading.catalogs.iter().map(|c| c.objects.len()).sum();
        tracing::info!(database = %self.database, owners = reading.catalogs.len(), objects, "informix catalog read");
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_names_the_server_for_sqli_and_no_server_for_drda() {
        let c = InformixConnection::parse("db.local:9090/stores@ol_prod", "ro", "PW").unwrap();
        assert_eq!(
            (c.protocol, c.host.as_str(), c.port, c.database.as_str()),
            (Protocol::Sqli, "db.local", 9090, "stores")
        );
        assert_eq!(c.server.as_deref(), Some("ol_prod"));
        assert_eq!(c.label(), "informix:ro@db.local:9090/stores@ol_prod");
        let d = InformixConnection::parse("drda://db.local/stores", "ro", "PW").unwrap();
        assert_eq!(
            (d.protocol, d.port, d.server.clone()),
            (Protocol::Drda, 9089, None)
        );
        assert_eq!(d.label(), "informix+drda:ro@db.local:9089/stores");
        let e = InformixConnection::parse(
            "h/stores@ol?driver=/opt/csdk/lib/cli/iclit09b.so&db-locale=en_US.utf8",
            "ro",
            "PW",
        )
        .unwrap();
        assert_eq!(
            (e.port, e.driver.as_deref(), e.db_locale.as_deref()),
            (
                9088,
                Some("/opt/csdk/lib/cli/iclit09b.so"),
                Some("en_US.utf8")
            )
        );
        for bad in [
            "db.local",
            "h/stores",
            "h:x/stores@ol",
            "/stores@ol",
            "h/a/b@ol",
            "h/stores@",
            "drda://h/stores@ol",
            "h/stores@ol?encrypt=1",
            "h/stores@ol?db-locale=x;y",
            "h/sto;res@ol",
            "h;x/stores@ol",
        ] {
            assert!(InformixConnection::parse(bad, "ro", "PW").is_err(), "{bad}");
        }
    }

    #[test]
    fn the_connection_string_quotes_what_odbc_would_split() {
        let c = InformixConnection::parse("h:9088/stores@ol", "ro", "PW").unwrap();
        let s = c.connection_string(CSDK_DRIVER_NAME, "p;w}d");
        assert!(s.starts_with("DRIVER=IBM INFORMIX ODBC DRIVER;HOST=h;SERVICE=9088;SERVER=ol;"));
        assert!(s.contains("PROTOCOL=onsoctcp;DATABASE=stores;DELIMIDENT=y;"));
        assert!(s.ends_with("UID=ro;PWD={p;w}}d};"), "{s}");
        let d = InformixConnection::parse("drda://h/stores", "ro", "PW").unwrap();
        assert_eq!(
            d.connection_string("/x/libdb2.so", "pw"),
            "DRIVER=/x/libdb2.so;DATABASE=stores;HOSTNAME=h;PORT=9089;PROTOCOL=TCPIP;UID=ro;PWD=pw;"
        );
    }

    #[test]
    fn the_driver_is_the_named_one_else_the_csdk_under_informixdir() {
        let c = InformixConnection::parse("h/stores@ol", "ro", "PW").unwrap();
        assert_eq!(
            c.driver_with(&|k| (k == "INFORMIXDIR").then(|| "/opt/ifx/".to_string())),
            "/opt/ifx/lib/cli/iclit09b.so"
        );
        assert_eq!(c.driver_with(&|_| None), CSDK_DRIVER_NAME);
        let d = InformixConnection::parse("drda://h/stores", "ro", "PW").unwrap();
        assert_eq!(
            d.driver_with(&|_| Some("/opt/ifx".into())),
            DRDA_DRIVER_NAME
        );
        let n =
            InformixConnection::parse("drda://h/stores?driver=/c/lib/libdb2.so", "r", "P").unwrap();
        assert_eq!(n.driver_with(&|_| None), "/c/lib/libdb2.so");
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
            write_check: Some(catalog::write_check_result(
                &[Some("0".into()), Some("0".into()), Some("0".into())],
                &session::Guards::default(),
            )),
        };
        let pkg = InformixObserver::new("stores", Arc::new(Fixed(reading.clone())))
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert_eq!(pkg.artifacts[0].content.target, "informix://stores/sales");
        assert!(
            pkg.artifacts
                .iter()
                .any(|a| a.content.target == "informix://stores/sales/table/orders")
        );
        let report = test_report("informix:ro@h:9088/stores@ol", &reading);
        assert_eq!(report["family"], "systables");
        assert_eq!(report["owners"][0]["census"]["TABLE"]["valid"], 2);
        assert_eq!(report["write_check"]["result"], "refused_by_server");
    }
}

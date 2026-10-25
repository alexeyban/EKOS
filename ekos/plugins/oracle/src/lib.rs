//! RFC 0211 §3.5 — the live Oracle catalog observer.
//!
//! One `[[databases]]` entry of kind `oracle` becomes one observer. A scan connects through
//! `ekos-ora-live` as a read-only user, reads each listed owner's catalog from one read-only
//! snapshot, reconciles it against the dictionary, and emits:
//!
//! - one `catalog` artifact per owner: the census, the view family read, the write probe's result
//!   and any `… ANY …` privilege the session holds — the facts a reviewer needs to trust the rest;
//! - one `object` artifact per catalog object (table with its columns, view, routine, constraint,
//!   index, …), its text already redacted by `ora-live`.
//!
//! Profiles (`ora-live::profile`) add counts, null counts, number and date bounds and text
//! lengths at the tier the `[[databases]]` entry allows — never a text value. The database work
//! runs on a blocking thread: the driver is
//! synchronous, and `ekos build` drives observers from an async runtime.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use ekos_ora_live::{
    CatalogSnapshot, ObjectKind, OraError, OraSource, ProfileStats, SessionPolicy, TableFlags,
    WriteCheck,
};
use std::sync::Arc;

pub use ekos_ora_live::{DictionaryKind, OraConnection, ProfileTier};

/// The observer's name, and the `source_kind` its artifacts carry.
pub const OBSERVER: &str = "oracle";

/// What one scan of a database read.
#[derive(Debug, Clone)]
pub struct DatabaseReading {
    pub snapshots: Vec<CatalogSnapshot>,
    /// Probed against the first table of the first owner that has one.
    pub write_check: Option<WriteCheck>,
    /// `SESSION_PRIVS` entries containing `ANY`: a finding even when the probe was refused.
    pub any_privileges: Vec<String>,
    /// One per snapshot, in the same order.
    pub profiles: Vec<ProfileStats>,
    /// Archive-log and supplemental-logging state, per owner in snapshot order.
    pub log_states: Vec<serde_json::Value>,
    /// RFC 0212: the dictionary's table flags per owner, when `[[databases]] dictionary` asks.
    pub dictionaries: Vec<Option<Vec<TableFlags>>>,
}

/// Where a reading comes from: the live database, or a fixed reading in tests.
pub trait CatalogReader: Send + Sync {
    fn read(&self) -> Result<DatabaseReading, OraError>;
}

/// Reads a live database through `ekos-ora-live`.
pub struct LiveReader {
    pub connection: OraConnection,
    pub owners: Vec<String>,
    pub run_id: String,
    pub policy: SessionPolicy,
    pub redaction: RedactionConfig,
    pub profile: ProfileTier,
    pub dictionary: Option<DictionaryKind>,
}

/// RFC 0220: one run-log line for a step over one owner, `counts` flattened into it.
fn log_step(
    stage: &str,
    msg: &str,
    owner: &str,
    started: std::time::Instant,
    counts: &impl serde::Serialize,
) {
    let mut line = serde_json::json!({ "owner": owner });
    if let Ok(serde_json::Value::Object(map)) = serde_json::to_value(counts) {
        line.as_object_mut().expect("an object").extend(map);
    }
    line["ms"] = (started.elapsed().as_millis() as u64).into();
    ekos_common::run_log::emit(stage, msg, &line);
}

impl CatalogReader for LiveReader {
    fn read(&self) -> Result<DatabaseReading, OraError> {
        let source = OraSource::connect(&self.connection, &self.run_id, &self.policy)?;
        let family = source.family();
        let mut snapshots = Vec::new();
        let mut profiles = Vec::new();
        let mut log_states = Vec::new();
        let mut dictionaries = Vec::new();
        for owner in &self.owners {
            let mut snapshot = ekos_ora_live::introspect(&source, owner, family, &self.redaction)?;
            let started = std::time::Instant::now();
            let profile = ekos_ora_live::profile::scan(
                &source,
                &mut snapshot,
                self.profile,
                &self.redaction,
            )?;
            log_step("oracle.profile", "profiled", owner, started, &profile);
            profiles.push(profile);
            log_states.push(ekos_ora_live::profile::log_state(&source, owner));
            dictionaries.push(match self.dictionary {
                Some(kind) => {
                    let started = std::time::Instant::now();
                    let flags = ekos_ora_live::dictionary::read(&source, kind, owner)?;
                    let counts = serde_json::json!({ "tables": flags.len() });
                    log_step(
                        "oracle.dictionary",
                        "read AD_Table flags",
                        owner,
                        started,
                        &counts,
                    );
                    Some(flags)
                }
                None => None,
            });
            snapshots.push(snapshot);
        }
        let probe = snapshots.iter().find_map(|s| {
            s.of_kind(ObjectKind::Table)
                .next()
                .map(|t| (s.owner.clone(), t.name.clone()))
        });
        let write_check = match probe {
            Some((owner, table)) => Some(source.check_write_refused(&owner, &table)?),
            None => None,
        };
        let any_privileges = source
            .session_privileges()?
            .into_iter()
            .filter(|p| p.contains(" ANY "))
            .collect();
        Ok(DatabaseReading {
            snapshots,
            write_check,
            any_privileges,
            profiles,
            log_states,
            dictionaries,
        })
    }
}

/// RFC 0223: what `ekos database test` found — the census and what the connector listed, per owner.
/// Counts and names of object types only: never a password, never a row value.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionTest {
    /// `oracle:<user>@<connect>`.
    pub label: String,
    /// `dba` with `SELECT_CATALOG_ROLE`, else `all` (a read-only user then sees fewer routines).
    pub family: String,
    pub owners: Vec<OwnerCensus>,
    /// The zero-row insert probe: refused by the server is what EKOS wants.
    pub write_check: Option<WriteCheck>,
    /// `… ANY …` privileges the session holds (a finding: they reach beyond the owners listed).
    pub any_privileges: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OwnerCensus {
    pub owner: String,
    /// `*_OBJECTS` by `OBJECT_TYPE`: valid and invalid.
    pub census: std::collections::BTreeMap<String, ekos_ora_live::Census>,
    /// What the connector lists, by kind (equal to the census after reconciliation).
    pub listed: std::collections::BTreeMap<ObjectKind, usize>,
    pub columns: usize,
}

/// RFC 0223: connect read-only, read each owner's catalog (no profiling), reconcile it with
/// `*_OBJECTS`, and check that a write is refused. Blocking: call it off an async runtime.
pub fn test_connection(
    connection: &OraConnection,
    owners: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
) -> Result<ConnectionTest, OraError> {
    let source = OraSource::connect(connection, run_id, &SessionPolicy::default())?;
    let family = source.family();
    let mut out = Vec::new();
    let mut probe = None;
    for owner in owners {
        let snapshot = ekos_ora_live::introspect(&source, owner, family, redaction)?;
        if probe.is_none() {
            probe = snapshot
                .of_kind(ObjectKind::Table)
                .next()
                .map(|t| (owner.clone(), t.name.clone()));
        }
        out.push(OwnerCensus {
            owner: owner.clone(),
            listed: snapshot.counts(),
            census: snapshot.census,
            columns: snapshot.columns,
        });
    }
    let write_check = match probe {
        Some((owner, table)) => Some(source.check_write_refused(&owner, &table)?),
        None => None,
    };
    let any_privileges = source
        .session_privileges()?
        .into_iter()
        .filter(|p| p.contains(" ANY "))
        .collect();
    Ok(ConnectionTest {
        label: connection.label(),
        family: format!("{family:?}").to_ascii_lowercase(),
        owners: out,
        write_check,
        any_privileges,
    })
}

/// One `[[databases]]` entry of kind `oracle`.
pub struct OracleObserver {
    /// The `[[databases]] name`: it keys every artifact and object of this database.
    database: String,
    reader: Arc<dyn CatalogReader>,
}

impl OracleObserver {
    pub fn new(database: impl Into<String>, reader: Arc<dyn CatalogReader>) -> Self {
        Self {
            database: database.into(),
            reader,
        }
    }
}

/// `oracle://<database>/<OWNER>` and `…/<kind>/<NAME>`: stable, and never a file path, so the
/// excluded-file globs of RFC 0043 cannot match it by accident.
pub fn catalog_target(database: &str, owner: &str) -> String {
    format!("oracle://{database}/{owner}")
}

pub fn object_target(database: &str, owner: &str, kind: ObjectKind, name: &str) -> String {
    format!("oracle://{database}/{owner}/{}/{name}", kind.as_str())
}

/// `<kind>/<NAME>` of every object in `s`, sorted: the tail of each object's target.
pub fn object_entries(s: &CatalogSnapshot) -> Vec<String> {
    let mut v: Vec<String> = s
        .objects
        .iter()
        .map(|o| format!("{}/{}", o.kind.as_str(), o.name))
        .collect();
    v.sort();
    v
}

/// The artifacts of one reading. Pure, so the mapping is tested without a database.
pub fn artifacts(database: &str, reading: &DatabaseReading) -> Vec<ObservationArtifact> {
    let mut out = Vec::new();
    for (n, s) in reading.snapshots.iter().enumerate() {
        let data = serde_json::json!({
            "kind": "catalog",
            "database": database,
            "owner": s.owner,
            "family": s.family,
            "census": s.census,
            "listed": s.counts().into_iter().map(|(k, n)| (k.as_str(), n)).collect::<std::collections::BTreeMap<_, _>>(),
            "columns": s.columns,
            "folded_not_null": s.folded_not_null,
            "other_constraints": s.other_constraints,
            "write_check": reading.write_check,
            "any_privileges": reading.any_privileges,
            "profile": reading.profiles.get(n),
            "log_state": reading.log_states.get(n),
            // What this read found, so recover can drop objects an earlier read left behind.
            "objects": object_entries(s),
        });
        out.push(
            ObservationArtifact::new(OBSERVER, catalog_target(database, &s.owner), data)
                .with_producer("ekos-plugin-oracle"),
        );
        if let Some(Some(flags)) = reading.dictionaries.get(n) {
            let data = serde_json::json!({
                "kind": "dictionary",
                "database": database,
                "owner": s.owner,
                "tables": flags,
            });
            out.push(
                ObservationArtifact::new(
                    OBSERVER,
                    format!("{}/dictionary", catalog_target(database, &s.owner)),
                    data,
                )
                .with_producer("ekos-plugin-oracle"),
            );
        }
        for o in &s.objects {
            let data = serde_json::json!({
                "kind": "object",
                "database": database,
                "object": o,
            });
            out.push(
                ObservationArtifact::new(
                    OBSERVER,
                    object_target(database, &o.owner, o.kind, &o.name),
                    data,
                )
                .with_producer("ekos-plugin-oracle"),
            );
        }
    }
    out
}

#[async_trait]
impl Observer for OracleObserver {
    fn name(&self) -> &str {
        OBSERVER
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let reader = Arc::clone(&self.reader);
        let reading = tokio::task::spawn_blocking(move || reader.read())
            .await
            .map_err(|e| ObserveError::connector(format!("oracle reader stopped: {e}")))?
            .map_err(|e| {
                ObserveError::connector(format!("oracle database {}: {e}", self.database))
            })?;
        let mut pkg = ObservationPackage::new(OBSERVER, "database");
        for a in artifacts(&self.database, &reading) {
            pkg.push(a);
        }
        let objects: usize = reading.snapshots.iter().map(|s| s.objects.len()).sum();
        tracing::info!(
            database = %self.database,
            owners = reading.snapshots.len(),
            objects,
            "oracle catalog read"
        );
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_ora_live::{CatalogObject, Census, ViewFamily};

    struct Fixed(DatabaseReading);

    impl CatalogReader for Fixed {
        fn read(&self) -> Result<DatabaseReading, OraError> {
            Ok(self.0.clone())
        }
    }

    fn reading() -> DatabaseReading {
        let table = CatalogObject {
            kind: ObjectKind::Table,
            owner: "ADEMPIERE".into(),
            name: "C_INVOICE".into(),
            status: Some("VALID".into()),
            detail: serde_json::json!({ "columns": [{ "name": "C_INVOICE_ID", "data_type": "NUMBER(10,0)", "nullable": false }] }),
        };
        let f = CatalogObject {
            kind: ObjectKind::Function,
            owner: "ADEMPIERE".into(),
            name: "INVOICEOPEN".into(),
            status: Some("VALID".into()),
            detail: serde_json::json!({ "text": "FUNCTION invoiceOpen RETURN NUMBER IS BEGIN RETURN 0; END;" }),
        };
        DatabaseReading {
            snapshots: vec![CatalogSnapshot {
                owner: "ADEMPIERE".into(),
                family: ViewFamily::Dba,
                objects: vec![table, f],
                census: [
                    (
                        "TABLE".to_string(),
                        Census {
                            valid: 1,
                            invalid: 0,
                        },
                    ),
                    (
                        "FUNCTION".to_string(),
                        Census {
                            valid: 1,
                            invalid: 0,
                        },
                    ),
                ]
                .into_iter()
                .collect(),
                columns: 1,
                folded_not_null: 0,
                other_constraints: Default::default(),
            }],
            write_check: Some(WriteCheck::RefusedByServer {
                message: "ORA-01031: insufficient privileges".into(),
            }),
            any_privileges: vec![],
            profiles: vec![],
            log_states: vec![],
            dictionaries: vec![],
        }
    }

    #[tokio::test]
    async fn a_scan_emits_one_catalog_artifact_and_one_per_object() {
        let obs = OracleObserver::new("erp", Arc::new(Fixed(reading())));
        let pkg = obs.scan(&ScanContext::new(".")).await.unwrap();
        let targets: Vec<&str> = pkg
            .artifacts
            .iter()
            .map(|a| a.content.target.as_str())
            .collect();
        assert_eq!(
            targets,
            [
                "oracle://erp/ADEMPIERE",
                "oracle://erp/ADEMPIERE/table/C_INVOICE",
                "oracle://erp/ADEMPIERE/function/INVOICEOPEN",
            ]
        );
        let catalog = &pkg.artifacts[0].content.data;
        assert_eq!(catalog["family"], "dba");
        assert_eq!(catalog["write_check"]["result"], "refused_by_server");
        assert_eq!(catalog["listed"]["table"], 1);
        assert_eq!(pkg.artifacts[1].content.data["object"]["kind"], "table");
    }

    #[tokio::test]
    async fn a_reader_failure_names_the_database() {
        struct Failing;
        impl CatalogReader for Failing {
            fn read(&self) -> Result<DatabaseReading, OraError> {
                Err(OraError::DriverNotBuilt)
            }
        }
        let obs = OracleObserver::new("erp", Arc::new(Failing));
        let Err(err) = obs.scan(&ScanContext::new(".")).await else {
            panic!("a failing reader must fail the scan");
        };
        assert!(err.to_string().contains("oracle database erp"), "{err}");
    }
}

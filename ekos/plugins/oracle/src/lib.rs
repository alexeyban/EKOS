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
//! No row value is read here. The database work runs on a blocking thread: the driver is
//! synchronous, and `ekos build` drives observers from an async runtime.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use ekos_ora_live::{CatalogSnapshot, ObjectKind, OraError, OraSource, SessionPolicy, WriteCheck};
use std::sync::Arc;

pub use ekos_ora_live::OraConnection;

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
}

impl CatalogReader for LiveReader {
    fn read(&self) -> Result<DatabaseReading, OraError> {
        let source = OraSource::connect(&self.connection, &self.run_id, &self.policy)?;
        let family = source.family();
        let mut snapshots = Vec::new();
        for owner in &self.owners {
            snapshots.push(ekos_ora_live::introspect(
                &source,
                owner,
                family,
                &self.redaction,
            )?);
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
        })
    }
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

/// The artifacts of one reading. Pure, so the mapping is tested without a database.
pub fn artifacts(database: &str, reading: &DatabaseReading) -> Vec<ObservationArtifact> {
    let mut out = Vec::new();
    for s in &reading.snapshots {
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
        });
        out.push(
            ObservationArtifact::new(OBSERVER, catalog_target(database, &s.owner), data)
                .with_producer("ekos-plugin-oracle"),
        );
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

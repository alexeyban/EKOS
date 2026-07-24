//! DeFi Sentinel crypto entity/relationship/evidence observer plugin (RFC 0017).
//!
//! Reads the Parquet export a separate Python pipeline (DeFi Sentinel) writes every
//! 15-minute batch — see `docs/ekos-export-contract.md` in that repo for the schema.
//! `ParquetExportReader` is written to that documented schema and exercised against
//! real producer output in `tests/fixtures/` (both hand-seeded and live-API-sourced).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One row of `entities.parquet`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityRecord {
    pub entity_id: String,
    pub kind: String,
    pub name: String,
    /// Raw JSON object, kind-specific.
    pub attrs: String,
}

/// One row of `relationships.parquet`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipRecord {
    pub src_entity_id: String,
    pub dst_entity_id: String,
    pub kind: String,
    pub attrs: String,
    pub evidence_ids: Vec<String>,
}

/// One row of `evidence.parquet`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub evidence_id: String,
    pub evidence_type: String,
    pub reference: String,
    pub payload: String,
    pub source: String,
}

/// One `batch_id=<ts>/` export directory, fully read into memory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportBatch {
    pub batch_id: String,
    pub entities: Vec<EntityRecord>,
    pub relationships: Vec<RelationshipRecord>,
    pub evidence: Vec<EvidenceRecord>,
}

#[derive(Debug, Error)]
pub enum CryptoReaderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("unexpected column type for `{column}` in {file}")]
    UnexpectedType { file: String, column: &'static str },
}

/// Interface for retrieving the latest export batch. Constructor-injected into
/// `CryptoObserver`, mirroring `SalesforceClient` in `ekos-plugin-salesforce`
/// (RFC 0012) — locating/opening the data source is the caller's job, not the
/// observer's.
#[async_trait]
pub trait CryptoExportReader: Send + Sync {
    /// Read the lexicographically-latest `batch_id=<ts>/` directory under
    /// `export_root`. Returns `Ok(None)` if `export_root` has no batches yet —
    /// that is a normal state (DeFi Sentinel hasn't run a cycle yet), not an error.
    async fn read_latest_batch(
        &self,
        export_root: &Path,
    ) -> Result<Option<ExportBatch>, CryptoReaderError>;
}

/// Real reader: parses the Parquet files DeFi Sentinel's Python exporter writes.
/// Uses `parquet`'s row-level `record` API only — no `arrow` dependency, since
/// nothing here needs Arrow's compute/kernel surface, just column access.
pub struct ParquetExportReader;

impl ParquetExportReader {
    fn latest_batch_dir(export_root: &Path) -> Result<Option<PathBuf>, CryptoReaderError> {
        if !export_root.is_dir() {
            return Ok(None);
        }
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(export_root)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("batch_id="))
            })
            .collect();
        dirs.sort();
        Ok(dirs.pop())
    }

    fn read_entities(path: &Path) -> Result<Vec<EntityRecord>, CryptoReaderError> {
        read_rows(path, |row| {
            Ok(EntityRecord {
                entity_id: get_string(row, path, "entity_id", 0)?,
                kind: get_string(row, path, "kind", 1)?,
                name: get_string(row, path, "name", 2)?,
                attrs: get_string(row, path, "attrs", 3)?,
            })
        })
    }

    fn read_relationships(path: &Path) -> Result<Vec<RelationshipRecord>, CryptoReaderError> {
        read_rows(path, |row| {
            Ok(RelationshipRecord {
                src_entity_id: get_string(row, path, "src_entity_id", 0)?,
                dst_entity_id: get_string(row, path, "dst_entity_id", 1)?,
                kind: get_string(row, path, "kind", 2)?,
                attrs: get_string(row, path, "attrs", 3)?,
                evidence_ids: get_string_list(row, path, "evidence_ids", 4)?,
            })
        })
    }

    fn read_evidence(path: &Path) -> Result<Vec<EvidenceRecord>, CryptoReaderError> {
        read_rows(path, |row| {
            Ok(EvidenceRecord {
                evidence_id: get_string(row, path, "evidence_id", 0)?,
                evidence_type: get_string(row, path, "evidence_type", 1)?,
                reference: get_string(row, path, "reference", 2)?,
                payload: get_string(row, path, "payload", 3)?,
                source: get_string(row, path, "source", 5)?,
            })
        })
    }
}

#[async_trait]
impl CryptoExportReader for ParquetExportReader {
    async fn read_latest_batch(
        &self,
        export_root: &Path,
    ) -> Result<Option<ExportBatch>, CryptoReaderError> {
        let Some(batch_dir) = Self::latest_batch_dir(export_root)? else {
            return Ok(None);
        };
        let batch_id = batch_dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("batch_id="))
            .unwrap_or_default()
            .to_string();

        let entities = Self::read_entities(&batch_dir.join("entities.parquet"))?;
        let relationships = Self::read_relationships(&batch_dir.join("relationships.parquet"))?;
        let evidence = Self::read_evidence(&batch_dir.join("evidence.parquet"))?;

        Ok(Some(ExportBatch {
            batch_id,
            entities,
            relationships,
            evidence,
        }))
    }
}

fn read_rows<T>(
    path: &Path,
    map_row: impl Fn(&parquet::record::Row) -> Result<T, CryptoReaderError>,
) -> Result<Vec<T>, CryptoReaderError> {
    use parquet::file::reader::{FileReader, SerializedFileReader};

    let file = std::fs::File::open(path)?;
    let reader = SerializedFileReader::new(file)?;
    let mut out = Vec::new();
    for row in reader.get_row_iter(None)? {
        out.push(map_row(&row?)?);
    }
    Ok(out)
}

fn get_string(
    row: &parquet::record::Row,
    path: &Path,
    column: &'static str,
    idx: usize,
) -> Result<String, CryptoReaderError> {
    use parquet::record::RowAccessor;
    row.get_string(idx)
        .map(|s| s.to_string())
        .map_err(|_| CryptoReaderError::UnexpectedType {
            file: path.display().to_string(),
            column,
        })
}

fn get_string_list(
    row: &parquet::record::Row,
    path: &Path,
    column: &'static str,
    idx: usize,
) -> Result<Vec<String>, CryptoReaderError> {
    use parquet::record::{Field, RowAccessor};

    let list = row
        .get_list(idx)
        .map_err(|_| CryptoReaderError::UnexpectedType {
            file: path.display().to_string(),
            column,
        })?;
    list.elements()
        .iter()
        .map(|f| match f {
            Field::Str(s) => Ok(s.clone()),
            _ => Err(CryptoReaderError::UnexpectedType {
                file: path.display().to_string(),
                column,
            }),
        })
        .collect()
}

/// In-process reader for unit tests — returns a fixed batch, no filesystem access.
pub struct MockCryptoExportReader {
    pub batch: Option<ExportBatch>,
}

impl MockCryptoExportReader {
    pub fn new(batch: Option<ExportBatch>) -> Self {
        Self { batch }
    }
}

#[async_trait]
impl CryptoExportReader for MockCryptoExportReader {
    async fn read_latest_batch(
        &self,
        _export_root: &Path,
    ) -> Result<Option<ExportBatch>, CryptoReaderError> {
        Ok(self.batch.clone())
    }
}

/// Observer emitting one `ObservationArtifact` per export batch.
pub struct CryptoObserver {
    reader: Arc<dyn CryptoExportReader>,
    export_root: PathBuf,
}

impl CryptoObserver {
    pub fn new(reader: Arc<dyn CryptoExportReader>, export_root: impl Into<PathBuf>) -> Self {
        Self {
            reader,
            export_root: export_root.into(),
        }
    }
}

#[async_trait]
impl Observer for CryptoObserver {
    fn name(&self) -> &str {
        "crypto"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let mut pkg = ObservationPackage::new("crypto", "defi-sentinel");

        let batch = self
            .reader
            .read_latest_batch(&self.export_root)
            .await
            .map_err(|e| ObserveError::connector(format!("crypto export read failed: {e}")))?;

        let Some(batch) = batch else {
            return Ok(pkg); // no export yet — normal, not an error
        };

        let data = serde_json::json!({
            "batch_id": batch.batch_id,
            "entities": batch.entities,
            "relationships": batch.relationships,
            "evidence": batch.evidence,
        });

        let artifact = ObservationArtifact::new("crypto", &batch.batch_id, data)
            .with_producer("ekos-plugin-crypto");
        pkg.push(artifact);

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_batch() -> ExportBatch {
        ExportBatch {
            batch_id: "20260720T1200".into(),
            entities: vec![EntityRecord {
                entity_id: "token:solana:Mint1".into(),
                kind: "Token".into(),
                name: "Fixture Token".into(),
                attrs: "{}".into(),
            }],
            relationships: vec![RelationshipRecord {
                src_entity_id: "wallet:solana:Dep1".into(),
                dst_entity_id: "token:solana:Mint1".into(),
                kind: "DEPLOYED".into(),
                attrs: "{}".into(),
                evidence_ids: vec!["ev1".into()],
            }],
            evidence: vec![EvidenceRecord {
                evidence_id: "ev1".into(),
                evidence_type: "api_response".into(),
                reference: "birdeye:...".into(),
                payload: "{}".into(),
                source: "birdeye".into(),
            }],
        }
    }

    #[tokio::test]
    async fn emits_one_artifact_for_the_batch() {
        let reader = Arc::new(MockCryptoExportReader::new(Some(sample_batch())));
        let observer = CryptoObserver::new(reader, "/tmp/does-not-matter");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.artifacts[0].content.target, "20260720T1200");
    }

    #[tokio::test]
    async fn no_batch_yet_produces_empty_package() {
        let reader = Arc::new(MockCryptoExportReader::new(None));
        let observer = CryptoObserver::new(reader, "/tmp/does-not-matter");
        let ctx = ScanContext::new(".");
        let pkg = observer.scan(&ctx).await.unwrap();
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn same_batch_same_artifact_id() {
        let ctx = ScanContext::new(".");
        let pkg1 = CryptoObserver::new(
            Arc::new(MockCryptoExportReader::new(Some(sample_batch()))),
            "/tmp/x",
        )
        .scan(&ctx)
        .await
        .unwrap();
        let pkg2 = CryptoObserver::new(
            Arc::new(MockCryptoExportReader::new(Some(sample_batch()))),
            "/tmp/x",
        )
        .scan(&ctx)
        .await
        .unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }

    #[tokio::test]
    async fn real_sample_fixture_parses_via_parquet_reader() {
        // tests/fixtures/sample_batch/ IS an export root: it contains one
        // batch_id=<ts>/ subdirectory, same layout `read_latest_batch` expects
        // in production.
        let export_root = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sample_batch"
        ));
        let reader = ParquetExportReader;
        let batch = reader
            .read_latest_batch(export_root)
            .await
            .unwrap()
            .expect("fixture batch should be found");

        assert_eq!(batch.entities.len(), 7);
        assert_eq!(batch.relationships.len(), 5);
        assert_eq!(batch.evidence.len(), 5);

        let deployed = batch
            .relationships
            .iter()
            .find(|r| r.kind == "DEPLOYED")
            .expect("fixture has a DEPLOYED relationship");
        assert!(!deployed.evidence_ids.is_empty());

        let token = batch
            .entities
            .iter()
            .find(|e| e.kind == "Token")
            .expect("fixture has a Token entity");
        assert!(token.attrs.contains("risk"));
    }

    #[tokio::test]
    async fn live_fixture_parses_real_producer_output() {
        let export_root = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/live_batch"
        ));
        let reader = ParquetExportReader;
        let batch = reader
            .read_latest_batch(export_root)
            .await
            .unwrap()
            .expect("live fixture batch should be found");

        assert!(
            batch.entities.len() > 50,
            "live batch should have dozens of real pump.fun entities, got {}",
            batch.entities.len()
        );
        assert!(batch.entities.iter().any(|e| e.kind == "Token"));
    }
}

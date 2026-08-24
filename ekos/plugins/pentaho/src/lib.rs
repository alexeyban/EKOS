//! Pentaho Kettle (`.ktr`/`.kjb`) observer plugin (RFC 0027 Phase 3).
//!
//! Walks the workspace tree the same way `LocalDocsObserver`/`FileObserver`
//! do, but only for `.ktr`/`.kjb` files, capturing the raw XML verbatim as a
//! fact — no XML parsing/step interpretation happens here. That deterministic
//! parsing step lives in `ekos_recovery::PentahoAnalyzerPass`, which reads
//! this observer's output, mirroring how `LocalDocsObserver` captures raw
//! document content and `LocalDocAnalyzerPass` does the structural
//! interpretation downstream.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Observer emitting one `ObservationArtifact` per `.ktr`/`.kjb` file found
/// under the workspace root.
#[derive(Debug, Default)]
pub struct PentahoObserver;

impl PentahoObserver {
    pub fn new() -> Self {
        Self
    }
}

/// Which top-level Kettle document this file is — a transformation (data
/// flow, `.ktr`) or a job (orchestration, `.kjb`). `PentahoAnalyzerPass`
/// uses this to decide whether step-to-`TransformNode` mapping applies at
/// all: job entries are orchestration, not data transformation, and are
/// recorded as `Unmapped` rather than forced into the mapping table.
fn kettle_kind(ext: &str) -> Option<&'static str> {
    if ext.eq_ignore_ascii_case("ktr") {
        Some("transformation")
    } else if ext.eq_ignore_ascii_case("kjb") {
        Some("job")
    } else {
        None
    }
}

#[async_trait]
impl Observer for PentahoObserver {
    fn name(&self) -> &str {
        "pentaho"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("pentaho", &target);

        for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
            if e.file_type().is_dir()
                && let Some(name) = e.file_name().to_str()
            {
                return !ctx.ignore_patterns.iter().any(|p| name == p.as_str());
            }
            true
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("pentaho observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let rel_path = match abs_path.strip_prefix(root) {
                // A real, valid `[observe] paths` entry can be a single bare file, not a
                // directory — `WalkDir::new(root)` then yields exactly one entry equal to
                // `root` itself, and stripping it from itself leaves an empty relative path
                // (the same real bug RFC 0088's own live verification found in `plugins/
                // file` and `plugins/localdocs` first). Falls back to the file's own name.
                Ok(r) if r.as_os_str().is_empty() => abs_path
                    .file_name()
                    .map(|n| n.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if ctx.is_ignored(&rel_path) {
                continue;
            }

            let Some(ext) = abs_path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let Some(kind) = kettle_kind(ext) else {
                continue;
            };

            let xml = match tokio::fs::read_to_string(abs_path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(
                        "pentaho observer: cannot read {}: {err}",
                        abs_path.display()
                    );
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            let size_bytes = xml.len();
            let content_sha256 = {
                let mut h = Sha256::new();
                h.update(xml.as_bytes());
                hex::encode(h.finalize())
            };

            let data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
                "kettle_kind": kind,
                "xml": xml,
            });

            let artifact = ObservationArtifact::new("pentaho", &rel_path, data)
                .with_producer("ekos-plugin-pentaho");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE_KTR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<transformation>
  <info><name>Load Customers</name></info>
  <step>
    <name>Read Customers</name>
    <type>TableInput</type>
    <sql>SELECT id, status FROM dbo.cust_mstr</sql>
  </step>
</transformation>
"#;

    #[tokio::test]
    async fn observer_emits_one_artifact_per_ktr_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("load_customers.ktr"), SAMPLE_KTR).unwrap();
        std::fs::write(dir.path().join("readme.md"), "not pentaho").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PentahoObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["path"], "load_customers.ktr");
        assert_eq!(data["kettle_kind"], "transformation");
        assert!(data["xml"].as_str().unwrap().contains("TableInput"));
    }

    #[tokio::test]
    async fn a_single_bare_file_observe_path_gets_its_own_real_name_not_an_empty_one() {
        // Real bug found live (RFC 0088's own verification): a real, valid `[observe] paths`
        // entry can be a single bare file, not a directory. `WalkDir::new(root)` then yields
        // exactly one entry equal to `root` itself, and stripping it from itself used to
        // leave an empty relative path.
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("load_customers.ktr");
        std::fs::write(&file_path, SAMPLE_KTR).unwrap();

        let ctx = ScanContext::new(&file_path);
        let pkg = PentahoObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        assert_eq!(pkg.artifacts[0].content.data["path"], "load_customers.ktr");
    }

    #[tokio::test]
    async fn observer_recognizes_kjb_as_job() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("orchestrate.kjb"),
            "<?xml version=\"1.0\"?><job><name>Orchestrate</name></job>",
        )
        .unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PentahoObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        assert_eq!(pkg.artifacts[0].content.data["kettle_kind"], "job");
    }

    #[tokio::test]
    async fn observer_ignores_unrelated_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("query.sql"), "SELECT 1").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PentahoObserver::new().scan(&ctx).await.unwrap();

        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn same_file_produces_same_content_hash_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.ktr"), SAMPLE_KTR).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg1 = PentahoObserver::new().scan(&ctx).await.unwrap();
        let pkg2 = PentahoObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}

//! Python (`.py`) observer plugin (RFC 0038/0040 Phase 2).
//!
//! Walks the workspace tree the same way `PentahoObserver`/`LocalDocsObserver` do, but only for
//! `.py` files, capturing the raw source verbatim as a fact — no parsing happens here. That
//! deterministic parsing step lives in `ekos_recovery::PythonAnalyzerPass`, which reads this
//! observer's output, mirroring how `PentahoObserver` captures raw XML and `PentahoAnalyzerPass`
//! does the structural interpretation downstream.
//!
//! `.ipynb` notebooks are explicitly out of scope for this observer (RFC 0038 Phase 3, not this
//! phase) — only real `.py` files are discovered.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Observer emitting one `ObservationArtifact` per `.py` file found under the workspace root.
#[derive(Debug, Default)]
pub struct PythonObserver;

impl PythonObserver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Observer for PythonObserver {
    fn name(&self) -> &str {
        "python"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("python", &target);

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
                    tracing::warn!("python observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let rel_path = match abs_path.strip_prefix(root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if ctx.is_ignored(&rel_path) {
                continue;
            }

            let is_py = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("py"));
            if !is_py {
                continue;
            }

            let source = match tokio::fs::read_to_string(abs_path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("python observer: cannot read {}: {err}", abs_path.display());
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            let size_bytes = source.len();
            let content_sha256 = {
                let mut h = Sha256::new();
                h.update(source.as_bytes());
                hex::encode(h.finalize())
            };

            let data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
                "source": source,
            });

            let artifact = ObservationArtifact::new("python", &rel_path, data)
                .with_producer("ekos-plugin-python");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE_PY: &str = r#"# Databricks notebook source
import sys
from dp.io.delta import write_delta

dbutils.widgets.text("catalog", "dp_catalog")
catalog = dbutils.widgets.get("catalog")
"#;

    #[tokio::test]
    async fn observer_emits_one_artifact_per_py_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notebook.py"), SAMPLE_PY).unwrap();
        std::fs::write(dir.path().join("readme.md"), "not python").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PythonObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["path"], "notebook.py");
        assert!(
            data["source"]
                .as_str()
                .unwrap()
                .contains("dbutils.widgets.get")
        );
    }

    #[tokio::test]
    async fn observer_ignores_unrelated_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("query.sql"), "SELECT 1").unwrap();
        std::fs::write(dir.path().join("notebook.ipynb"), "{}").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PythonObserver::new().scan(&ctx).await.unwrap();

        assert!(
            pkg.artifacts.is_empty(),
            ".ipynb is explicitly out of scope for this phase (RFC 0038 Phase 3)"
        );
    }

    #[tokio::test]
    async fn same_file_produces_same_content_hash_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.py"), SAMPLE_PY).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg1 = PythonObserver::new().scan(&ctx).await.unwrap();
        let pkg2 = PythonObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}

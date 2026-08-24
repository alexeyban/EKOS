//! Rust (`.rs`) observer plugin (RFC 0041).
//!
//! Walks the workspace tree the same way `PythonObserver`/`PentahoObserver` do, but only for
//! `.rs` files, capturing the raw source verbatim as a fact — no parsing happens here. That
//! deterministic parsing step lives in `ekos_recovery::RustAnalyzerPass`, which reads this
//! observer's output.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Observer emitting one `ObservationArtifact` per `.rs` file found under the workspace root.
#[derive(Debug, Default)]
pub struct RustObserver;

impl RustObserver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Observer for RustObserver {
    fn name(&self) -> &str {
        "rust"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("rust", &target);

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
                    tracing::warn!("rust observer: skipping unreadable entry: {err}");
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

            let is_rs = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("rs"));
            if !is_rs {
                continue;
            }

            let source = match tokio::fs::read_to_string(abs_path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("rust observer: cannot read {}: {err}", abs_path.display());
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

            let artifact =
                ObservationArtifact::new("rust", &rel_path, data).with_producer("ekos-plugin-rust");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE_RS: &str = r#"use std::collections::HashMap;

pub fn greet(name: &str) -> String {
    format!("hello {name}")
}
"#;

    #[tokio::test]
    async fn observer_emits_one_artifact_per_rs_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), SAMPLE_RS).unwrap();
        std::fs::write(dir.path().join("readme.md"), "not rust").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = RustObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["path"], "lib.rs");
        assert!(data["source"].as_str().unwrap().contains("fn greet"));
    }

    #[tokio::test]
    async fn a_single_bare_file_observe_path_gets_its_own_real_name_not_an_empty_one() {
        // Real bug found live (RFC 0088's own verification): a real, valid `[observe] paths`
        // entry can be a single bare file, not a directory. `WalkDir::new(root)` then yields
        // exactly one entry equal to `root` itself, and stripping it from itself used to
        // leave an empty relative path.
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("lib.rs");
        std::fs::write(&file_path, SAMPLE_RS).unwrap();

        let ctx = ScanContext::new(&file_path);
        let pkg = RustObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        assert_eq!(pkg.artifacts[0].content.data["path"], "lib.rs");
    }

    #[tokio::test]
    async fn observer_ignores_unrelated_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("query.sql"), "SELECT 1").unwrap();
        std::fs::write(dir.path().join("script.py"), "print(1)").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = RustObserver::new().scan(&ctx).await.unwrap();

        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn same_file_produces_same_content_hash_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), SAMPLE_RS).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg1 = RustObserver::new().scan(&ctx).await.unwrap();
        let pkg2 = RustObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}

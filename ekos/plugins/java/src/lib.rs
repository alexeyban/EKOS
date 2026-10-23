//! Java (`.java`) source observer plugin (RFC 0209).
//!
//! Walks the workspace tree the same way `PerlObserver`/`ElixirObserver` do, capturing each Java
//! source file verbatim as a fact — no parsing happens here. The deterministic declaration-level
//! parse lives in `ekos_recovery::JavaAnalyzerPass`, which reads this observer's output.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Observer emitting one `ObservationArtifact` per `.java` file found under the workspace root.
#[derive(Debug, Default)]
pub struct JavaObserver;

impl JavaObserver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Observer for JavaObserver {
    fn name(&self) -> &str {
        "java"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("java", &target);

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
                    tracing::warn!("java observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let is_java = abs_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|e| e.eq_ignore_ascii_case("java"));
            if !is_java {
                continue;
            }

            let rel_path = match abs_path.strip_prefix(root) {
                // A bare-file `[observe] paths` entry: `WalkDir::new(root)` yields `root` itself,
                // and stripping it from itself leaves an empty path (RFC 0088's live finding).
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

            let source = match tokio::fs::read(abs_path).await {
                // Legacy Java trees still carry ISO-8859-1 sources; a lossy decode keeps the file
                // (declarations are ASCII) instead of dropping it over one accented comment.
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(err) => {
                    tracing::warn!("java observer: cannot read {}: {err}", abs_path.display());
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            let content_sha256 = {
                let mut h = Sha256::new();
                h.update(source.as_bytes());
                hex::encode(h.finalize())
            };

            let data = serde_json::json!({
                "path": rel_path,
                "size_bytes": source.len(),
                "content_sha256": content_sha256,
                "source": source,
            });

            let artifact =
                ObservationArtifact::new("java", &rel_path, data).with_producer("ekos-plugin-java");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE: &str = "package org.example;\n\npublic class Invoice {\n}\n";

    fn paths_of(pkg: &ObservationPackage) -> Vec<&str> {
        pkg.artifacts
            .iter()
            .map(|a| a.content.data["path"].as_str().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn observer_emits_one_artifact_per_java_file() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src/org/example")).unwrap();
        std::fs::write(dir.path().join("src/org/example/Invoice.java"), SAMPLE).unwrap();
        std::fs::write(dir.path().join("Main.JAVA"), SAMPLE).unwrap();
        std::fs::write(dir.path().join("Invoice.class"), [0xCA, 0xFE, 0xBA, 0xBE]).unwrap();
        std::fs::write(dir.path().join("readme.md"), "not java").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = JavaObserver::new().scan(&ctx).await.unwrap();

        let mut paths = paths_of(&pkg);
        paths.sort_unstable();
        assert_eq!(paths, vec!["Main.JAVA", "src/org/example/Invoice.java"]);
    }

    #[tokio::test]
    async fn a_latin1_source_is_kept_not_dropped() {
        let dir = TempDir::new().unwrap();
        let mut bytes = b"// Gr\xfc\xdfe\n".to_vec();
        bytes.extend_from_slice(SAMPLE.as_bytes());
        std::fs::write(dir.path().join("Legacy.java"), bytes).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = JavaObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        let source = pkg.artifacts[0].content.data["source"].as_str().unwrap();
        assert!(source.contains("public class Invoice"));
    }

    #[tokio::test]
    async fn a_single_bare_file_observe_path_gets_its_own_name() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("Invoice.java");
        std::fs::write(&file_path, SAMPLE).unwrap();

        let ctx = ScanContext::new(&file_path);
        let pkg = JavaObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(paths_of(&pkg), vec!["Invoice.java"]);
    }

    #[tokio::test]
    async fn same_file_produces_same_artifact_id_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("A.java"), SAMPLE).unwrap();

        let ctx = ScanContext::new(dir.path());
        let a = JavaObserver::new().scan(&ctx).await.unwrap();
        let b = JavaObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(a.artifacts[0].id, b.artifacts[0].id);
    }
}

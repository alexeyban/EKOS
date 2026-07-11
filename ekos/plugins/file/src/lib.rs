//! File-system observer plugin.
//!
//! Walks the workspace directory tree and emits one `ObservationArtifact` per
//! regular file. The artifact data contains the file's relative path, byte size,
//! and SHA-256 content hash — enough for downstream passes to detect changes
//! and to produce deterministic `KirObject` IDs.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObserveError, ObservationPackage, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Reference file-system observer.
///
/// Scans the `workspace_root` recursively, skipping any directory whose name
/// matches an entry in `ctx.ignore_patterns`. Emits one `ObservationArtifact`
/// per regular file.
pub struct FileObserver;

impl FileObserver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Observer for FileObserver {
    fn name(&self) -> &str {
        "file"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("file", &target);

        for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
            // Skip ignored directory names (e.g. .git, target).
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
                    tracing::warn!("file observer: skipping unreadable entry: {err}");
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

            // Double-check: skip if any path component is ignored.
            if ctx.is_ignored(&rel_path) {
                continue;
            }

            let content = match tokio::fs::read(abs_path).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    tracing::warn!("file observer: cannot read {}: {err}", abs_path.display());
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            let size_bytes = content.len();
            let content_sha256 = {
                let mut h = Sha256::new();
                h.update(&content);
                hex::encode(h.finalize())
            };

            let data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
            });

            let artifact = ObservationArtifact::new("file", &rel_path, data)
                .with_producer("ekos-plugin-file");

            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    async fn scan_temp(setup: impl FnOnce(&TempDir)) -> ObservationPackage {
        let dir = TempDir::new().unwrap();
        setup(&dir);
        let ctx = ScanContext::new(dir.path());
        FileObserver::new().scan(&ctx).await.unwrap()
    }

    #[tokio::test]
    async fn empty_dir_produces_no_artifacts() {
        let pkg = scan_temp(|_| {}).await;
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn single_file_produces_one_artifact() {
        let pkg = scan_temp(|dir| {
            std::fs::write(dir.path().join("hello.txt"), b"hello").unwrap();
        })
        .await;
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.artifacts[0].content.connector_name, "file");
        assert_eq!(pkg.artifacts[0].content.target, "hello.txt");
    }

    #[tokio::test]
    async fn same_file_same_artifact_id() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("f.rs"), b"fn main() {}").unwrap();
        let ctx = ScanContext::new(dir.path());
        let obs = FileObserver::new();
        let pkg1 = obs.scan(&ctx).await.unwrap();
        let pkg2 = obs.scan(&ctx).await.unwrap();
        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id, "same file must yield same artifact ID");
    }

    #[tokio::test]
    async fn changed_file_changes_artifact_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.rs");
        std::fs::write(&path, b"version 1").unwrap();
        let ctx = ScanContext::new(dir.path());
        let obs = FileObserver::new();
        let id1 = obs.scan(&ctx).await.unwrap().artifacts[0].id.clone();
        std::fs::write(&path, b"version 2").unwrap();
        let id2 = obs.scan(&ctx).await.unwrap().artifacts[0].id.clone();
        assert_ne!(id1, id2, "different file content must yield different artifact ID");
    }

    #[tokio::test]
    async fn git_dir_is_skipped() {
        let pkg = scan_temp(|dir| {
            let git = dir.path().join(".git");
            std::fs::create_dir_all(&git).unwrap();
            std::fs::write(git.join("HEAD"), b"ref: refs/heads/main").unwrap();
            std::fs::write(dir.path().join("src.rs"), b"fn main() {}").unwrap();
        })
        .await;
        assert_eq!(pkg.len(), 1, "only src.rs, .git/HEAD must be skipped");
        assert_eq!(pkg.artifacts[0].content.target, "src.rs");
    }

    #[tokio::test]
    async fn data_contains_expected_fields() {
        let dir = TempDir::new().unwrap();
        let payload = b"hello world";
        std::fs::write(dir.path().join("readme.md"), payload).unwrap();
        let ctx = ScanContext::new(dir.path());
        let pkg = FileObserver::new().scan(&ctx).await.unwrap();
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["path"], "readme.md");
        assert_eq!(data["size_bytes"], payload.len());
        assert!(data["content_sha256"].as_str().unwrap().len() == 64);
    }
}

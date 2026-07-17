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

            let mut data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
            });
            // RFC 0014: for text files, the opening excerpt is an observation
            // fact — it feeds the ledger's content FTS. Binary files get none.
            if let Some(excerpt) = text_excerpt(&content) {
                data["excerpt"] = serde_json::Value::String(excerpt);
            }

            let artifact = ObservationArtifact::new("file", &rel_path, data)
                .with_producer("ekos-plugin-file");

            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

/// Cap on the excerpt captured from text files (RFC 0014). 600 chars covers
/// headings and preamble — where a document says what it is — without
/// bloating the FTS index with entire file bodies.
const EXCERPT_MAX_CHARS: usize = 600;

/// The opening excerpt of a text file, or `None` for binary content.
/// Truncates on a char boundary so the result is always valid UTF-8.
fn text_excerpt(content: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(content).ok()?;
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(EXCERPT_MAX_CHARS).collect())
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
    async fn text_files_carry_an_excerpt_binary_files_do_not() {
        let long_text = "# Title\n".to_string() + &"x".repeat(2000);
        let pkg = scan_temp(move |dir| {
            std::fs::write(dir.path().join("note.md"), long_text.as_bytes()).unwrap();
            std::fs::write(dir.path().join("blob.bin"), [0xff, 0xfe, 0x00, 0x9f]).unwrap();
        })
        .await;

        let note = pkg.artifacts.iter().find(|a| a.content.target == "note.md").unwrap();
        let excerpt = note.content.data["excerpt"].as_str().unwrap();
        assert!(excerpt.starts_with("# Title"), "excerpt keeps the opening");
        assert_eq!(excerpt.chars().count(), EXCERPT_MAX_CHARS, "excerpt is capped");

        let blob = pkg.artifacts.iter().find(|a| a.content.target == "blob.bin").unwrap();
        assert!(blob.content.data.get("excerpt").is_none(), "binary files carry no excerpt");
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

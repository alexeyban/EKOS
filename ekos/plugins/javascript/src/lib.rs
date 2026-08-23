//! JavaScript/TypeScript (`.js`/`.jsx`/`.ts`/`.tsx`/`.mjs`/`.cjs`) observer plugin (RFC 0085).
//!
//! Walks the workspace tree the same way `RustObserver`/`PythonObserver`/`ElixirObserver` do,
//! capturing the raw source verbatim as a fact — no parsing happens here. That deterministic
//! parsing step lives in `ekos_recovery::JavaScriptAnalyzerPass`, which reads this observer's
//! output. `node_modules` is excluded via the workspace's own default `ignore_patterns`
//! (`ObserveConfig::default`), not a special case here.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs"];

/// Observer emitting one `ObservationArtifact` per real JS/TS source file found under the
/// workspace root.
#[derive(Debug, Default)]
pub struct JavaScriptObserver;

impl JavaScriptObserver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Observer for JavaScriptObserver {
    fn name(&self) -> &str {
        "javascript"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("javascript", &target);

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
                    tracing::warn!("javascript observer: skipping unreadable entry: {err}");
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

            let is_js = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.iter().any(|ext| e.eq_ignore_ascii_case(ext)));
            if !is_js {
                continue;
            }

            let source = match tokio::fs::read_to_string(abs_path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(
                        "javascript observer: cannot read {}: {err}",
                        abs_path.display()
                    );
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

            let artifact = ObservationArtifact::new("javascript", &rel_path, data)
                .with_producer("ekos-plugin-javascript");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE_JS: &str = r#"import React from "react";

export function Greeting(name) {
  return "hello " + name;
}
"#;

    #[tokio::test]
    async fn observer_emits_one_artifact_per_real_js_ts_extension() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("greeting.js"), SAMPLE_JS).unwrap();
        std::fs::write(dir.path().join("app.tsx"), "export const App = () => null;").unwrap();
        std::fs::write(dir.path().join("readme.md"), "not js").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = JavaScriptObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 2);
        let paths: Vec<&str> = pkg
            .artifacts
            .iter()
            .map(|a| a.content.data["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"greeting.js"));
        assert!(paths.contains(&"app.tsx"));
    }

    #[tokio::test]
    async fn observer_ignores_unrelated_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("script.py"), "print(1)").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = JavaScriptObserver::new().scan(&ctx).await.unwrap();

        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn same_file_produces_same_content_hash_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.js"), SAMPLE_JS).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg1 = JavaScriptObserver::new().scan(&ctx).await.unwrap();
        let pkg2 = JavaScriptObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}

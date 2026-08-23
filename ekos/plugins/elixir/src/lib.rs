//! Elixir (`.ex`/`.exs`) observer plugin (RFC 0081).
//!
//! Walks the workspace tree the same way `RustObserver`/`PythonObserver` do, but for Elixir
//! source files, capturing the raw source verbatim as a fact — no parsing happens here. That
//! deterministic parsing step lives in `ekos_recovery::ElixirAnalyzerPass`, which reads this
//! observer's output.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Observer emitting one `ObservationArtifact` per `.ex`/`.exs` file found under the workspace
/// root.
#[derive(Debug, Default)]
pub struct ElixirObserver;

impl ElixirObserver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Observer for ElixirObserver {
    fn name(&self) -> &str {
        "elixir"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("elixir", &target);

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
                    tracing::warn!("elixir observer: skipping unreadable entry: {err}");
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

            let is_elixir = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ex") || e.eq_ignore_ascii_case("exs"));
            if !is_elixir {
                continue;
            }

            let source = match tokio::fs::read_to_string(abs_path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("elixir observer: cannot read {}: {err}", abs_path.display());
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

            let artifact = ObservationArtifact::new("elixir", &rel_path, data)
                .with_producer("ekos-plugin-elixir");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE_EX: &str = r#"defmodule Plausible.Greeting do
  def hello(name) do
    "hello " <> name
  end
end
"#;

    #[tokio::test]
    async fn observer_emits_one_artifact_per_ex_and_exs_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("greeting.ex"), SAMPLE_EX).unwrap();
        std::fs::write(dir.path().join("mix.exs"), "defmodule Mix.Foo do\nend\n").unwrap();
        std::fs::write(dir.path().join("readme.md"), "not elixir").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = ElixirObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 2);
        let paths: Vec<&str> = pkg
            .artifacts
            .iter()
            .map(|a| a.content.data["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"greeting.ex"));
        assert!(paths.contains(&"mix.exs"));
    }

    #[tokio::test]
    async fn observer_ignores_unrelated_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("script.py"), "print(1)").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = ElixirObserver::new().scan(&ctx).await.unwrap();

        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn same_file_produces_same_content_hash_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.ex"), SAMPLE_EX).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg1 = ElixirObserver::new().scan(&ctx).await.unwrap();
        let pkg2 = ElixirObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}

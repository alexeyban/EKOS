//! Observation SDK — contract between the compiler and connectors.
//!
//! Every connector (file system, git, SQL, Confluence …) implements the `Observer`
//! trait and returns an `ObservationPackage`. The package is a typed, content-
//! addressable collection of `ObservationArtifact`s that the compiler then turns
//! into KIR during the knowledge-recovery phase.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use thiserror::Error;

/// Error returned by an observer during scanning.
#[derive(Debug, Error)]
pub enum ObserveError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("connector error: {0}")]
    Connector(String),
}

impl ObserveError {
    pub fn connector(msg: impl Into<String>) -> Self {
        Self::Connector(msg.into())
    }
}

/// Opaque configuration map passed to an observer from `ekos.toml`'s
/// `[connectors.<name>]` section.
#[derive(Debug, Clone, Default)]
pub struct ConnectorConfig(pub HashMap<String, serde_json::Value>);

impl ConnectorConfig {
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.as_str()
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.0.get(key)?.as_bool()
    }
}

/// Context provided to an observer when the compiler initiates a scan.
pub struct ScanContext {
    /// Root of the workspace being compiled.
    pub workspace_root: PathBuf,
    /// Connector-specific configuration from ekos.toml.
    pub config: ConnectorConfig,
    /// Path components to skip (`.git`, `target`, …).
    pub ignore_patterns: Vec<String>,
}

impl ScanContext {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            config: ConnectorConfig::default(),
            ignore_patterns: vec![
                ".ekos".into(),
                ".git".into(),
                "target".into(),
                "node_modules".into(),
            ],
        }
    }

    pub fn with_config(mut self, config: ConnectorConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_ignore_patterns(mut self, patterns: Vec<String>) -> Self {
        self.ignore_patterns = patterns;
        self
    }

    /// Returns true if any path component matches an ignore pattern.
    pub fn is_ignored(&self, rel_path: &str) -> bool {
        for component in rel_path.split('/') {
            if self.ignore_patterns.iter().any(|p| component == p.as_str()) {
                return true;
            }
        }
        false
    }
}

/// Metadata about a completed scan.
#[derive(Debug, Clone)]
pub struct PackageMeta {
    pub observer_name: String,
    pub scanned_at: chrono::DateTime<chrono::Utc>,
    pub file_count: usize,
    pub error_count: usize,
}

/// Collection of observation artifacts produced by one connector scan.
pub struct ObservationPackage {
    pub observer: String,
    pub target: String,
    pub artifacts: Vec<ObservationArtifact>,
    pub meta: PackageMeta,
}

impl ObservationPackage {
    pub fn new(observer: impl Into<String>, target: impl Into<String>) -> Self {
        let observer = observer.into();
        let target = target.into();
        let meta = PackageMeta {
            observer_name: observer.clone(),
            scanned_at: chrono::Utc::now(),
            file_count: 0,
            error_count: 0,
        };
        Self { observer, target, artifacts: Vec::new(), meta }
    }

    pub fn push(&mut self, artifact: ObservationArtifact) {
        self.meta.file_count += 1;
        self.artifacts.push(artifact);
    }

    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }
}

/// Core connector extension point.
///
/// # Contract
/// - `scan` must not modify the workspace.
/// - `scan` is safe to call multiple times; identical workspace state → identical artifact IDs.
#[async_trait]
pub trait Observer: Send + Sync {
    fn name(&self) -> &str;

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError>;
}

// Keep old name as an alias so other stubs that use ObserverError still compile.
pub use ObserveError as ObserverError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_ignored_catches_prefix_segments() {
        let ctx = ScanContext::new(".");
        assert!(ctx.is_ignored(".git/config"));
        assert!(ctx.is_ignored("target/debug/build/foo"));
        assert!(!ctx.is_ignored("src/main.rs"));
    }

    #[test]
    fn observation_package_counts() {
        let mut pkg = ObservationPackage::new("test", "/tmp");
        assert!(pkg.is_empty());
        pkg.push(ObservationArtifact::new("test", "a", serde_json::json!({})));
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.meta.file_count, 1);
    }
}

//! Artifact cache invalidation for compiler passes (Phase 13 — Optimizer).
//!
//! A pass's cached output is valid only if all three of the following are
//! unchanged since it last ran: the pass's own logic (`version`), the
//! relevant slice of `ekos.toml` (`config_hash`), and its inputs
//! (`cache_inputs`). This mirrors RFC 0002's three invalidation rules, but the
//! manifest is a plain JSON file rather than a content-addressed artifact —
//! `ArtifactStore::write` is a no-op once an id already exists, which fits
//! immutable content but not a "latest known state" pointer like this.

use crate::pass::CompilerPass;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PassManifest {
    version: String,
    config_hash: String,
    inputs: Vec<String>,
}

fn manifest_path(manifest_dir: &Path, pass_name: &str) -> std::path::PathBuf {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, pass_name.as_bytes());
    let hex = hex::encode(sha2::Digest::finalize(hasher));
    manifest_dir.join(format!("{hex}.json"))
}

/// SHA-256 of the canonical JSON of a config value — used as the
/// `config_hash` half of a pass's recomputation identity.
pub fn config_hash(value: &serde_json::Value) -> String {
    ekos_artifact::ArtifactId::compute(value)
        .as_str()
        .to_string()
}

/// Should this pass re-run, or is its previously cached output still valid?
///
/// Returns `true` (recompute) if no manifest exists yet, or if `version`,
/// `config_hash`, or `cache_inputs()` differ from what was recorded the last
/// time this pass ran successfully.
pub fn should_recompute(pass: &dyn CompilerPass, config_hash: &str, manifest_dir: &Path) -> bool {
    let path = manifest_path(manifest_dir, pass.name());
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return true;
    };
    let Ok(manifest) = serde_json::from_str::<PassManifest>(&raw) else {
        return true;
    };

    manifest.version != pass.version()
        || manifest.config_hash != config_hash
        || manifest.inputs != pass.cache_inputs()
}

/// Record this pass's recomputation identity after it has run successfully.
pub fn record_manifest(pass: &dyn CompilerPass, config_hash: &str, manifest_dir: &Path) {
    let manifest = PassManifest {
        version: pass.version().to_string(),
        config_hash: config_hash.to_string(),
        inputs: pass.cache_inputs(),
    };
    if std::fs::create_dir_all(manifest_dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(&manifest) {
        let _ = std::fs::write(manifest_path(manifest_dir, pass.name()), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::PassContext;
    use async_trait::async_trait;

    struct StubPass {
        name: &'static str,
        version: &'static str,
        inputs: Vec<String>,
    }

    #[async_trait]
    impl CompilerPass for StubPass {
        fn name(&self) -> &str {
            self.name
        }
        fn version(&self) -> &str {
            self.version
        }
        fn cache_inputs(&self) -> Vec<String> {
            self.inputs.clone()
        }
        async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), crate::pass::PassError> {
            Ok(())
        }
    }

    #[test]
    fn no_manifest_means_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let pass = StubPass {
            name: "p",
            version: "v1",
            inputs: vec!["a".into()],
        };
        assert!(should_recompute(&pass, "cfg1", dir.path()));
    }

    #[test]
    fn unchanged_identity_skips_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let pass = StubPass {
            name: "p",
            version: "v1",
            inputs: vec!["a".into()],
        };
        record_manifest(&pass, "cfg1", dir.path());
        assert!(!should_recompute(&pass, "cfg1", dir.path()));
    }

    #[test]
    fn changed_config_hash_forces_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let pass = StubPass {
            name: "p",
            version: "v1",
            inputs: vec!["a".into()],
        };
        record_manifest(&pass, "cfg1", dir.path());
        assert!(should_recompute(&pass, "cfg2", dir.path()));
    }

    #[test]
    fn changed_inputs_forces_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let pass = StubPass {
            name: "p",
            version: "v1",
            inputs: vec!["a".into()],
        };
        record_manifest(&pass, "cfg1", dir.path());
        let changed = StubPass {
            name: "p",
            version: "v1",
            inputs: vec!["b".into()],
        };
        assert!(should_recompute(&changed, "cfg1", dir.path()));
    }

    #[test]
    fn changed_version_forces_recompute() {
        let dir = tempfile::tempdir().unwrap();
        let pass = StubPass {
            name: "p",
            version: "v1",
            inputs: vec!["a".into()],
        };
        record_manifest(&pass, "cfg1", dir.path());
        let bumped = StubPass {
            name: "p",
            version: "v2",
            inputs: vec!["a".into()],
        };
        assert!(should_recompute(&bumped, "cfg1", dir.path()));
    }
}

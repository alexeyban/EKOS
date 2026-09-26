//! RFC 0161 — the approval policy, as a versioned file.
//!
//! `migrate.policy.toml` sits beside `ekos.toml` and is committed, so "the thresholds were different
//! then" is answerable rather than arguable: every approval record carries the policy file's own
//! content hash.

use crate::risk::Thresholds;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Policy {
    #[serde(default)]
    pub thresholds: Thresholds,
    /// Which groups may approve each class, keyed `r2` / `r3` / `r4`.
    #[serde(default)]
    pub approvers: BTreeMap<String, Vec<String>>,
    /// Rows a P2 scan may read without an approval.
    #[serde(default = "default_scan_budget")]
    pub scan_budget_rows: f64,
}

fn default_scan_budget() -> f64 {
    50_000_000.0
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            thresholds: Thresholds::default(),
            approvers: BTreeMap::new(),
            scan_budget_rows: default_scan_budget(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid policy file {path}: {message}")]
    Parse { path: String, message: String },
}

/// A loaded policy and the hash of the file it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPolicy {
    pub policy: Policy,
    /// Recorded on every approval, so a decision can be read against the rules that were in force.
    pub content_hash: String,
    /// `false` when no file exists and the defaults are in use — which a report must be able to
    /// say, because "the default policy allowed it" and "our policy allowed it" are different
    /// statements.
    pub from_file: bool,
}

pub fn load(path: &std::path::Path) -> Result<LoadedPolicy, PolicyError> {
    if !path.exists() {
        return Ok(LoadedPolicy {
            policy: Policy::default(),
            content_hash: ekos_common::ContentHash::of_str("").as_str().to_string(),
            from_file: false,
        });
    }
    let text = std::fs::read_to_string(path).map_err(|source| PolicyError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let policy: Policy = toml::from_str(&text).map_err(|e| PolicyError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(LoadedPolicy {
        content_hash: ekos_common::ContentHash::of_str(&text).as_str().to_string(),
        policy,
        from_file: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_yields_defaults_and_says_so() {
        let p = load(std::path::Path::new("/nonexistent/migrate.policy.toml")).unwrap();
        assert!(
            !p.from_file,
            "a report must be able to say the defaults were used"
        );
        assert_eq!(p.policy.thresholds, Thresholds::default());
    }

    #[test]
    fn a_real_file_is_parsed_and_hashed() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("migrate.policy.toml");
        std::fs::write(
            &path,
            "[thresholds]\nblast_radius = 40\naffected_rows = 500\n\n\
             [approvers]\nr4 = [\"group:data-owner\", \"group:cto\"]\n",
        )
        .unwrap();
        let p = load(&path).unwrap();
        assert!(p.from_file);
        assert_eq!(p.policy.thresholds.blast_radius, 40);
        assert_eq!(p.policy.thresholds.affected_rows, 500);
        assert_eq!(p.policy.approvers["r4"].len(), 2);
        assert!(!p.content_hash.is_empty());
    }

    /// The hash is what makes "the thresholds were different then" answerable.
    #[test]
    fn a_changed_policy_changes_its_hash() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("p.toml");
        std::fs::write(
            &path,
            "[thresholds]\nblast_radius = 10\naffected_rows = 1000\n",
        )
        .unwrap();
        let a = load(&path).unwrap();
        std::fs::write(
            &path,
            "[thresholds]\nblast_radius = 99\naffected_rows = 1000\n",
        )
        .unwrap();
        let b = load(&path).unwrap();
        assert_ne!(a.content_hash, b.content_hash);
    }

    #[test]
    fn an_invalid_policy_file_is_an_error_not_a_silent_default() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("p.toml");
        std::fs::write(&path, "this is not toml = = =").unwrap();
        assert!(
            load(&path).is_err(),
            "falling back to defaults here would silently loosen the rules"
        );
    }
}

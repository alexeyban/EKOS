pub mod compress;
pub mod project;
pub mod redaction;

use sha2::{Digest, Sha256};

/// RFC 0135 Part A — version of EKOS's own observe-and-redact logic.
///
/// `ekos build` skips re-scanning an observe path whose source files are byte-for-byte unchanged
/// (`fingerprints.json`). That fingerprint is `(path, size, mtime)` only — it says nothing about
/// whether *EKOS's own code* that processes the bytes has changed. Before this constant existed, a
/// fix to [`redaction`] or an analyzer had no effect on an unchanged file until a manual full
/// `.ekos` wipe (see `devlog_100` / `devlog_112`).
///
/// This number is mixed into `build`'s fingerprint **cache key** (never the fingerprint value,
/// never any user-facing output). Bumping it invalidates every path's cache once, forcing a
/// single real re-scan; the re-scan re-derives artifact ids from post-redaction content
/// (RFC 0072), so genuinely-changed artifacts get persisted.
///
/// ## Bump it whenever any of these change in a way that alters observed/stored content:
/// - `ekos_common::redaction` (patterns, exclusions, the redact algorithm)
/// - any `Observer::scan` body, or `observation_sdk::walk_observed`
/// - the inline `File`-object construction in `crates/cli/src/commands/build.rs`
///
/// Per-workspace `[security]` config changes are handled automatically (the redaction config is
/// hashed into the same key) — this constant is only for changes to the code itself.
///
/// ## Changelog
/// - **v1** (RFC 0135, 2026-09-04): introduced.
pub const PIPELINE_LOGIC_VERSION: u32 = 1;

/// SHA-256 content hash used to address artifacts and ledger entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(hex::encode(hasher.finalize()))
    }

    pub fn of_str(s: &str) -> Self {
        Self::of(s.as_bytes())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_same_hash() {
        let h1 = ContentHash::of(b"hello");
        let h2 = ContentHash::of(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_content_different_hash() {
        let h1 = ContentHash::of(b"hello");
        let h2 = ContentHash::of(b"world");
        assert_ne!(h1, h2);
    }
}

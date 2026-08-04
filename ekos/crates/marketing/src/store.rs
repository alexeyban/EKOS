//! `marketing/posted/tweets.json` — the duplicate-detection store (RFC 0027).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid tweets.json: {0}")]
    Json(#[from] serde_json::Error),
}

/// One devlog's publish record. `devlog` is stored zero-padded to 3 digits to match the
/// source design doc's example (`"028"`), even though `DevlogSummary::number` is a plain u32.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostedTweet {
    pub devlog: String,
    pub tweet_id: String,
    pub date: String,
    pub feature: String,
}

pub struct PostedStore {
    path: PathBuf,
    entries: Vec<PostedTweet>,
}

impl PostedStore {
    /// Load `<marketing_dir>/posted/tweets.json`, treating a missing file as an empty store
    /// (the store doesn't exist until the first tweet is published).
    pub fn load(marketing_dir: &Path) -> Result<Self, StoreError> {
        let path = marketing_dir.join("posted").join("tweets.json");
        let entries = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            serde_json::from_str(&content)?
        } else {
            Vec::new()
        };
        Ok(Self { path, entries })
    }

    pub fn is_posted(&self, devlog_number: u32) -> Option<&PostedTweet> {
        let key = format!("{devlog_number:03}");
        self.entries.iter().find(|e| e.devlog == key)
    }

    pub fn record(&mut self, devlog_number: u32, tweet_id: String, date: String, feature: String) {
        self.entries.push(PostedTweet {
            devlog: format!("{devlog_number:03}"),
            tweet_id,
            date,
            feature,
        });
    }

    pub fn save(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_is_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = PostedStore::load(dir.path()).unwrap();
        assert!(store.is_posted(28).is_none());
    }

    #[test]
    fn record_then_save_then_reload_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PostedStore::load(dir.path()).unwrap();
        store.record(
            28,
            "19381283712".to_string(),
            "2026-08-04".to_string(),
            "Incremental compiler".to_string(),
        );
        store.save().unwrap();

        let reloaded = PostedStore::load(dir.path()).unwrap();
        let entry = reloaded.is_posted(28).unwrap();
        assert_eq!(entry.tweet_id, "19381283712");
        assert_eq!(entry.devlog, "028");
    }

    #[test]
    fn is_posted_is_false_for_a_different_devlog_number() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PostedStore::load(dir.path()).unwrap();
        store.record(
            28,
            "1".to_string(),
            "2026-08-04".to_string(),
            "x".to_string(),
        );
        assert!(store.is_posted(29).is_none());
        assert!(store.is_posted(28).is_some());
    }

    #[test]
    fn tweets_json_file_lands_at_marketing_posted_tweets_json() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PostedStore::load(dir.path()).unwrap();
        store.record(
            1,
            "id".to_string(),
            "2026-01-01".to_string(),
            "f".to_string(),
        );
        store.save().unwrap();
        assert!(dir.path().join("posted").join("tweets.json").exists());
    }
}

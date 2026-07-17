//! Compressed-JSON file helpers (RFC 0015).
//!
//! Machine-read JSON files (`ckm/model.json`, build snapshots) are stored as
//! compact JSON inside a zstd frame, with a `.zst` suffix appended to the
//! logical name (`model.json` → `model.json.zst`). Readers accept either the
//! compressed form or a legacy plain-JSON file so pre-RFC-0015 workspaces
//! keep working without migration.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

/// zstd level 3: measured 6–10× on real EKOS JSON at negligible CPU cost.
pub const ZSTD_LEVEL: i32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum CompressError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// The compressed sibling of a plain-JSON path: `model.json` → `model.json.zst`.
pub fn zst_sibling(plain_path: &Path) -> PathBuf {
    let mut name = plain_path.as_os_str().to_os_string();
    name.push(".zst");
    PathBuf::from(name)
}

/// Resolve which on-disk file backs `plain_path`: the `.zst` sibling if
/// present, else the plain file itself, else `None`.
pub fn resolve_auto(plain_path: &Path) -> Option<PathBuf> {
    let zst = zst_sibling(plain_path);
    if zst.exists() {
        Some(zst)
    } else if plain_path.exists() {
        Some(plain_path.to_path_buf())
    } else {
        None
    }
}

/// Serialize `value` as compact JSON and write it zstd-compressed to `path`
/// (callers pass the full `.zst` path, typically via [`zst_sibling`]).
pub fn write_json_zst<T: Serialize>(path: &Path, value: &T) -> Result<(), CompressError> {
    let json = serde_json::to_vec(value)?;
    let compressed = zstd::encode_all(&json[..], ZSTD_LEVEL).map_err(CompressError::Io)?;
    std::fs::write(path, compressed)?;
    Ok(())
}

/// Read a zstd-compressed JSON file written by [`write_json_zst`].
pub fn read_json_zst<T: DeserializeOwned>(path: &Path) -> Result<T, CompressError> {
    let bytes = std::fs::read(path)?;
    let json = zstd::decode_all(&bytes[..]).map_err(CompressError::Io)?;
    Ok(serde_json::from_slice(&json)?)
}

/// Read the value behind a logical plain-JSON path, accepting either the
/// compressed `.zst` sibling (preferred) or a legacy plain file.
pub fn read_json_auto<T: DeserializeOwned>(plain_path: &Path) -> Result<T, CompressError> {
    let zst = zst_sibling(plain_path);
    if zst.exists() {
        read_json_zst(&zst)
    } else {
        Ok(serde_json::from_slice(&std::fs::read(plain_path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_compressed() {
        let dir = std::env::temp_dir().join(format!("ekos-compress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.json.zst");

        let value = serde_json::json!({"objects": [1, 2, 3], "name": "orders"});
        write_json_zst(&path, &value).unwrap();
        let back: serde_json::Value = read_json_zst(&path).unwrap();
        assert_eq!(back, value);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_auto_prefers_zst_and_falls_back_to_plain() {
        let dir = std::env::temp_dir().join(format!("ekos-compress-auto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("model.json");

        // Legacy plain file only.
        std::fs::write(&plain, br#"{"v": 1}"#).unwrap();
        let v: serde_json::Value = read_json_auto(&plain).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(resolve_auto(&plain), Some(plain.clone()));

        // Compressed sibling wins once present.
        write_json_zst(&zst_sibling(&plain), &serde_json::json!({"v": 2})).unwrap();
        let v: serde_json::Value = read_json_auto(&plain).unwrap();
        assert_eq!(v["v"], 2);
        assert_eq!(resolve_auto(&plain), Some(zst_sibling(&plain)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compressed_is_smaller_than_plain_on_repetitive_json() {
        let dir = std::env::temp_dir().join(format!("ekos-compress-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.json.zst");

        let rows: Vec<_> = (0..500)
            .map(|i| serde_json::json!({"path": format!("src/file_{i}.rs"), "kind": "File"}))
            .collect();
        let value = serde_json::json!({ "objects": rows });
        write_json_zst(&path, &value).unwrap();

        let plain_len = serde_json::to_vec(&value).unwrap().len() as u64;
        let zst_len = std::fs::metadata(&path).unwrap().len();
        assert!(
            zst_len * 3 < plain_len,
            "expected ≥3× compression, got {plain_len}→{zst_len}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

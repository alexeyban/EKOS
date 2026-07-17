//! RFC 0016 §6 — the one audited `unsafe` surface of the fact engine.
//!
//! [`MappedSegment`] memory-maps a **sealed** segment or index-run file and
//! exposes it as `&[u8]`. Mapping a file is `unsafe` in Rust because the
//! borrow checker cannot see other mutators: if the underlying file were
//! truncated while mapped, reads through the map would fault.
//!
//! # Safety argument (accepted in RFC 0016's review)
//!
//! - EKOS only maps files that are **sealed**: written once, fsynced,
//!   recorded (with their SHA-256) in the manifest, and never modified,
//!   truncated, or rewritten by any EKOS code path afterwards. The active
//!   segment — the only file EKOS ever appends to or truncates (crash
//!   recovery) — is read with ordinary `read` calls, never mapped.
//! - A *hostile external* truncation of a sealed file turns reads into
//!   SIGBUS — the same failure class SQLite accepts in its own mmap mode,
//!   and outside the threat model (an actor who can truncate the store can
//!   also delete it).
//! - The map is private to this type and only ever exposed as an immutable
//!   byte slice tied to `self`'s lifetime.
//!
//! Every other module consumes the safe [`MappedSegment::bytes`] API; no
//! other `unsafe` exists in the crate (enforced by the crate-level
//! `#![deny(unsafe_code)]` with this module's targeted allow).

use std::fs::File;
use std::path::Path;

use super::SegmentError;

/// A read-only memory map of a sealed, immutable file.
pub struct MappedSegment {
    map: memmap2::Mmap,
}

impl MappedSegment {
    /// Map a sealed file. `expected_len` (from the manifest) is verified
    /// first — a length mismatch means the seal contract was violated and
    /// the file must not be mapped.
    pub fn open(path: &Path, expected_len: u64) -> Result<Self, SegmentError> {
        let file = File::open(path)?;
        let actual = file.metadata()?.len();
        if actual != expected_len {
            return Err(SegmentError::Corrupt(format!(
                "sealed file {} is {actual} bytes, manifest says {expected_len} — refusing to map",
                path.display()
            )));
        }
        // SAFETY: `path` names a sealed file — written once, hashed into the
        // manifest, never mutated or truncated by EKOS again (see module
        // docs; justification formally accepted in RFC 0016 §6). The map is
        // exposed only as an immutable slice borrowed from `self`.
        #[allow(unsafe_code)]
        let map = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Self { map })
    }

    /// The mapped bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn maps_sealed_file_and_verifies_length() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("seg");
        File::create(&path)
            .unwrap()
            .write_all(b"sealed bytes")
            .unwrap();

        let map = MappedSegment::open(&path, 12).unwrap();
        assert_eq!(map.bytes(), b"sealed bytes");

        // A manifest/length mismatch refuses to map.
        assert!(matches!(
            MappedSegment::open(&path, 99),
            Err(SegmentError::Corrupt(_))
        ));
    }
}

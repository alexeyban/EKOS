use sha2::{Digest, Sha256};

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

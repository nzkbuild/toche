//! Content-addressed storage for raw, unreduced tool outputs.
//!
//! Files are stored under `~/.toche/cas/<first2>/<remaining>` keyed by
//! SHA-256 hex digest.  This is the "source of truth" that `toche expand`
//! reads from to recover byte-identical originals.

use anyhow::Context;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

use crate::profiles::loader::config_dir;

/// Directory under which all content-addressed blobs live.
pub fn cas_dir() -> PathBuf {
    config_dir().join("cas")
}

/// Persist raw bytes and return the hex-encoded SHA-256 hash.
pub fn store(raw: &[u8]) -> anyhow::Result<String> {
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(raw);
        hex::encode(hasher.finalize())
    };

    let path = cas_dir().join(&hash[..2]).join(&hash[2..]);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create CAS directory {:?}", parent))?;
    }
    fs::write(&path, raw)
        .with_context(|| format!("failed to write CAS blob {:?}", path))?;

    Ok(hash)
}

/// Retrieve raw bytes for a hex-encoded SHA-256 hash.
pub fn retrieve(hash: &str) -> anyhow::Result<Vec<u8>> {
    // Basic validation to avoid path traversal outside cas_dir
    if hash.len() < 2 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("invalid hash: {}", hash);
    }
    let path = cas_dir().join(&hash[..2]).join(&hash[2..]);
    fs::read(&path).with_context(|| format!("CAS blob not found: {}", hash))
}

/// Check whether a blob exists for the given hash.
pub fn exists(hash: &str) -> bool {
    if hash.len() < 2 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    cas_dir().join(&hash[..2]).join(&hash[2..]).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_store_retrieve() {
        let data = b"hello world from toche 0.5.0";
        let hash = store(data).expect("store should succeed");
        let retrieved = retrieve(&hash).expect("retrieve should succeed");
        assert_eq!(retrieved, data);
    }

    #[test]
    fn identical_content_same_hash() {
        let data = b"deterministic test payload";
        let hash1 = store(data).expect("store 1");
        let hash2 = store(data).expect("store 2");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn different_content_different_hash() {
        let hash_a = store(b"content a").expect("store a");
        let hash_b = store(b"content b").expect("store b");
        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn exists_true_for_stored() {
        let data = b"existence check";
        let hash = store(data).expect("store");
        assert!(exists(&hash));
    }

    #[test]
    fn exists_false_for_unknown() {
        assert!(!exists("a".repeat(64).as_str()));
    }

    #[test]
    fn retrieve_invalid_hash_errors() {
        assert!(retrieve("nothex").is_err());
        assert!(retrieve("g").is_err());
    }
}

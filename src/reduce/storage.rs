//! Content-addressed storage for raw, unreduced tool outputs.
//!
//! Files are stored under `~/.toche/cas/<first2>/<remaining>` keyed by
//! SHA-256 hex digest.  This is the "source of truth" that `toche expand`
//! reads from to recover byte-identical originals.

use anyhow::Context;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::profiles::loader::config_dir;

/// Directory under which all content-addressed blobs live (default path).
/// Prefer `store_at` / `retrieve_at` / `delete_at` with an explicit CAS
/// directory from `StorageConfig::resolve_paths` when available.
pub fn cas_dir() -> PathBuf {
    config_dir().join("cas")
}

/// Persist raw bytes under the default CAS directory.
#[allow(dead_code)] // test/public default-path helper
pub fn store(raw: &[u8]) -> anyhow::Result<String> {
    store_at(raw, &cas_dir())
}

/// Persist raw bytes under an explicit CAS directory.
pub fn store_at(raw: &[u8], cas_root: &Path) -> anyhow::Result<String> {
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(raw);
        hex::encode(hasher.finalize())
    };

    let path = cas_root.join(&hash[..2]).join(&hash[2..]);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create CAS directory {:?}", parent))?;
    }
    fs::write(&path, raw).with_context(|| format!("failed to write CAS blob {:?}", path))?;

    Ok(hash)
}

/// Persist raw bytes without overwriting an existing matching CAS file.
/// Returns `(hash, created)`.
pub fn store_new_at(raw: &[u8], cas_root: &Path) -> anyhow::Result<(String, bool)> {
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(raw);
        hex::encode(hasher.finalize())
    };
    let path = cas_root.join(&hash[..2]).join(&hash[2..]);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create CAS directory {:?}", parent))?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(raw)
                .with_context(|| format!("failed to write CAS blob {:?}", path))?;
            Ok((hash, true))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok((hash, false)),
        Err(e) => Err(e).with_context(|| format!("failed to create CAS blob {:?}", path)),
    }
}

/// Retrieve raw bytes for a hex-encoded SHA-256 hash (default CAS dir).
#[allow(dead_code)] // test/public default-path helper
pub fn retrieve(hash: &str) -> anyhow::Result<Vec<u8>> {
    retrieve_at(hash, &cas_dir())
}

/// Retrieve raw bytes for a hex-encoded SHA-256 hash from an explicit CAS dir.
pub fn retrieve_at(hash: &str, cas_root: &Path) -> anyhow::Result<Vec<u8>> {
    if hash.len() < 2 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("invalid hash: {}", hash);
    }
    let path = cas_root.join(&hash[..2]).join(&hash[2..]);
    fs::read(&path).with_context(|| format!("CAS blob not found: {}", hash))
}

/// Delete a blob by hash (default CAS dir). Returns true if removed.
#[allow(dead_code)] // test/public default-path helper
pub fn delete(hash: &str) -> bool {
    delete_at(hash, &cas_dir())
}

/// Delete a blob by hash from an explicit CAS dir.
pub fn delete_at(hash: &str, cas_root: &Path) -> bool {
    if hash.len() < 2 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let path = cas_root.join(&hash[..2]).join(&hash[2..]);
    match std::fs::remove_file(&path) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            tracing::warn!("Failed to delete CAS blob {}: {e}", hash);
            false
        }
    }
}

/// Check whether a blob exists for the given hash (default CAS dir).
#[allow(dead_code)] // public API, may be used by cache management UIs
pub fn exists(hash: &str) -> bool {
    exists_at(hash, &cas_dir())
}

/// Check whether a blob exists at an explicit CAS dir.
#[allow(dead_code)]
pub fn exists_at(hash: &str, cas_root: &Path) -> bool {
    if hash.len() < 2 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    cas_root.join(&hash[..2]).join(&hash[2..]).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_store_retrieve() {
        let data = b"hello world from toche";
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
    fn store_new_at_keeps_existing_blob() {
        let dir = tempfile::tempdir().unwrap();
        let (hash, created) = store_new_at(b"first", dir.path()).unwrap();
        assert!(created);
        let path = dir.path().join(&hash[..2]).join(&hash[2..]);
        assert_eq!(std::fs::read(&path).unwrap(), b"first");

        let (_, created_again) = store_new_at(b"first", dir.path()).unwrap();
        assert!(!created_again);
        assert_eq!(std::fs::read(path).unwrap(), b"first");
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

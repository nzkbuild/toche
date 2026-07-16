use sha2::{Digest, Sha256};
use std::path::Path;

/// Compute a SHA-256 fingerprint of the current workspace state.
///
/// Hashes key project files (Cargo.toml, Cargo.lock, package.json,
/// .git/HEAD) to produce a value that changes when the workspace changes.
/// Returns hex-encoded hash (64 chars), or a fallback if no files are found.
pub fn compute_workspace_fingerprint() -> String {
    let project = std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let project_path = Path::new(&project);

    let watch_files = [
        "Cargo.toml",
        "Cargo.lock",
        "package.json",
        "package-lock.json",
        ".git/HEAD",
    ];

    let mut hashes: Vec<(String, String)> = Vec::new();

    for rel in &watch_files {
        let abs = project_path.join(rel);
        if abs.is_file() {
            if let Ok(contents) = std::fs::read(&abs) {
                let mut hasher = Sha256::new();
                hasher.update(&contents);
                let hash = hex::encode(hasher.finalize());
                hashes.push((rel.to_string(), hash));
            }
        }
    }

    // If no watch files found, return a stable fallback
    if hashes.is_empty() {
        return "no-workspace-files-detected".repeat(2); // 64 chars
    }

    // Sort by path for deterministic ordering
    hashes.sort_by(|a, b| a.0.cmp(&b.0));

    let mut combined = Sha256::new();
    for (path, hash) in &hashes {
        combined.update(path.as_bytes());
        combined.update(b":");
        combined.update(hash.as_bytes());
        combined.update(b"\n");
    }
    hex::encode(combined.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_fingerprint_is_64_hex_chars() {
        let fp = compute_workspace_fingerprint();
        assert_eq!(fp.len(), 64);
        assert!(
            fp.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint should be hex: {}",
            fp
        );
    }

    #[test]
    fn workspace_fingerprint_is_deterministic() {
        let fp1 = compute_workspace_fingerprint();
        let fp2 = compute_workspace_fingerprint();
        assert_eq!(fp1, fp2);
    }
}

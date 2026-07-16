use std::fs;
use std::path::{Path, PathBuf};

/// Atomically write content to path: temp file + rename
pub fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Atomically write content to path with restrictive permissions (0o600 on Unix).
/// Use for sensitive configuration files.
pub fn atomic_write_secure(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    restrict_permissions(&tmp)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Set restrictive permissions on the config directory (0o700 on Unix).
pub fn secure_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    restrict_dir_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_dir_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_dir_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Return the user's home directory, falling back to the current directory.
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        tracing::warn!("Cannot determine home directory, using current directory");
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    })
}

/// JSONC-tolerant settings.json read: strip // comments, then parse
pub fn read_jsonc(path: &Path) -> anyhow::Result<serde_json::Value> {
    let raw = fs::read_to_string(path)?;
    let cleaned: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(serde_json::from_str(&cleaned)?)
}

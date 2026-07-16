use anyhow::Context;
use std::path::PathBuf;

use super::types::Profiles;

/// Hierarchical config loading: env var -> default path
pub fn config_dir() -> PathBuf {
    let dir = std::env::var("TOCHE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                })
                .join(".toche")
        });

    // Apply restrictive permissions on the config directory
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("Failed to create config directory {:?}: {}", dir, e);
        }
        if let Err(e) = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)) {
            tracing::warn!("Failed to set permissions on {:?}: {}", dir, e);
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("Failed to create config directory {:?}: {}", dir, e);
        }
    }

    dir
}

pub fn load_profiles() -> anyhow::Result<Profiles> {
    let path = config_dir().join("profiles.toml");
    if !path.exists() {
        anyhow::bail!(
            "No profiles found at {}. Run `toche setup` first.",
            path.display()
        );
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&content).context("Failed to parse profiles.toml")
}

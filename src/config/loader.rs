use anyhow::Context;
use std::path::PathBuf;

use super::migration::detect_and_load;
use super::resolver::resolve_default;
use super::toche_config::TocheConfig;

/// Hierarchical config loading: env var -> default path
pub fn config_dir() -> PathBuf {
    let dir = std::env::var("TOCHE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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

/// Load the full TocheConfig, auto-migrating from profiles.toml if needed.
pub fn load_config() -> anyhow::Result<TocheConfig> {
    let dir = config_dir();
    match detect_and_load(&dir).context("Failed to load configuration")? {
        super::migration::ConfigSource::V2(config)
        | super::migration::ConfigSource::V1Migrated(config) => Ok(config),
        super::migration::ConfigSource::Missing => {
            anyhow::bail!(
                "No configuration found at {}. Run `toche setup` first.",
                dir.display()
            );
        }
    }
}

/// Load config and resolve the default integration.
/// Replaces the old `load_profiles()` → `Profiles::default_profile()` pattern.
pub fn load_default_integration() -> anyhow::Result<super::resolver::ResolvedIntegration> {
    let config = load_config()?;
    resolve_default(&config).context("No default integration configured")
}

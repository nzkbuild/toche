use anyhow::Context;
use std::path::PathBuf;

use super::types::Profiles;

/// Hierarchical config loading: env var -> default path
pub fn config_dir() -> PathBuf {
    std::env::var("TOCHE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".toche"))
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

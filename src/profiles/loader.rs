use std::path::PathBuf;

use crate::config::loader::{config_dir as new_config_dir, load_config};
use crate::profiles::types::Profiles;

/// Deprecated: use `crate::config::loader::config_dir()` instead.
pub fn config_dir() -> PathBuf {
    new_config_dir()
}

/// Deprecated: use `crate::config::loader::load_config()` instead.
/// This function loads the new config.toml and converts it back to the legacy
/// `Profiles` shape for callers that have not yet been updated.
#[allow(dead_code)]
pub fn load_profiles() -> anyhow::Result<Profiles> {
    let config = load_config()?;
    crate::config::migration::config_to_legacy_profiles(&config)
}

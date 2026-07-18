use anyhow::Context;

use crate::integrations::{
    ConnectOutcome, DisconnectOutcome, OwnedFragment, apply_owned_fragment, claude_backup_path,
    claude_settings_path, points_to_toche, remove_owned_fragment,
};

/// Connect Claude Code to Toche (persistent mode).
pub fn connect() -> anyhow::Result<ConnectOutcome> {
    let settings_path = claude_settings_path();

    // Check if already connected
    if settings_path.exists() {
        let current = crate::config::utils::read_jsonc(&settings_path)
            .context("Failed to parse settings.json")?;
        if points_to_toche(&current) {
            return Ok(ConnectOutcome::AlreadyConnected);
        }
    }

    let fragment = OwnedFragment::default_toche();
    let saved_url = apply_owned_fragment(&settings_path, &fragment)?;

    if saved_url.is_none() && settings_path.exists() {
        // Already connected case handled above; if saved_url is None here
        // it means apply_owned_fragment detected already-connected state.
        return Ok(ConnectOutcome::AlreadyConnected);
    }

    let backup_path = claude_backup_path();
    Ok(ConnectOutcome::Connected {
        settings_path: settings_path.display().to_string(),
        backup_path: if backup_path.exists() {
            Some(backup_path.display().to_string())
        } else {
            None
        },
    })
}

/// Disconnect Claude Code from Toche (remove persistent fragment).
pub fn disconnect() -> anyhow::Result<DisconnectOutcome> {
    let settings_path = claude_settings_path();
    remove_owned_fragment(&settings_path)
}

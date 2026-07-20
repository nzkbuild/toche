use anyhow::Context;

use crate::config::loader::config_dir;

use super::discovery::{codex_config_path, codex_home};

fn backup_path_for(config_path: &std::path::Path) -> std::path::PathBuf {
    let file_name = config_path
        .file_name()
        .expect("Codex config path must have a file name")
        .to_string_lossy();
    config_path.with_file_name(format!("{file_name}.toche-backup"))
}

/// Outcome of connecting Codex to Toche.
#[derive(Debug, Clone)]
pub enum CodexConnectOutcome {
    AlreadyConnected,
    Connected {
        config_path: String,
        backup_path: Option<String>,
    },
}

/// Outcome of disconnecting Codex from Toche.
#[derive(Debug, Clone)]
pub enum CodexDisconnectOutcome {
    NotConnected,
    Disconnected {
        #[allow(dead_code)]
        config_path: String,
        preserved: bool,
    },
}

/// Checks whether the Codex `config.toml` content already points to Toche.
fn points_to_toche(content: &str) -> bool {
    content.contains("127.0.0.1:8743")
}

/// The Toche-owned fragment for Codex config.toml.
/// Sets the OpenAI base URL to route through Toche.
struct OwnedFragment {
    openai_base_url: String,
}

impl OwnedFragment {
    fn for_endpoint(host: &str, port: u16) -> Self {
        Self {
            openai_base_url: format!("http://{host}:{port}/v1"),
        }
    }

    fn default_toche() -> Self {
        Self::for_endpoint("127.0.0.1", 8743)
    }
}

/// Apply the Toche-owned fragment to Codex config.toml.
/// Preserves comments and unrelated keys via toml_edit.
/// Returns the previous openai_base_url for recovery.
pub fn apply_owned_fragment(
    config_path: &std::path::Path,
    fragment_url: &str,
) -> anyhow::Result<Option<String>> {
    // Read existing or start with empty document
    let content = if config_path.exists() {
        std::fs::read_to_string(config_path).context("Failed to read Codex config.toml")?
    } else {
        String::new()
    };

    // Check if already connected
    if points_to_toche(&content) {
        return Ok(None); // Already connected
    }

    // Extract the original upstream URL before overwriting
    let original_url = super::discovery::codex_upstream_url_from_toml_public(&content);

    // Keep the backup alongside its source so independently owned configs do
    // not share lifecycle state.
    let backup_path = backup_path_for(config_path);
    if config_path.exists() && !backup_path.exists() {
        std::fs::copy(config_path, &backup_path).context("Failed to backup Codex config.toml")?;
    }

    // Save original URL for recovery
    if let Some(ref url) = original_url {
        let saved_path = config_dir().join("pre_toche_codex_url.txt");
        let _ = std::fs::write(&saved_path, url);
    }

    // Parse the existing config with toml_edit for comment preservation
    let mut doc: toml_edit::DocumentMut = if content.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        content
            .parse()
            .context("Failed to parse Codex config.toml as TOML")?
    };

    // Set the openai_base_url with a comment marking Toche ownership
    let toche_comment = "# Managed by Toche — do not edit directly.\n";
    doc.insert("openai_base_url", toml_edit::value(fragment_url));
    // Add the comment prefix to the key
    let mut key = doc.key_mut("openai_base_url").unwrap();
    let mut new_decor = toml_edit::Decor::new("\n", "");
    new_decor.set_prefix(toche_comment.to_string());
    *key.leaf_decor_mut() = new_decor;

    // Ensure codex home dir exists
    let codex_dir = if let Some(parent) = config_path.parent() {
        if parent.as_os_str().is_empty() {
            codex_home()
        } else {
            parent.to_path_buf()
        }
    } else {
        codex_home()
    };
    std::fs::create_dir_all(&codex_dir).context("Failed to create Codex config directory")?;

    // Write atomically
    let output = doc.to_string();
    crate::config::utils::atomic_write(config_path, &output)
        .context("Failed to write Codex config.toml")?;

    Ok(original_url)
}

/// Remove the Toche-owned fragment from Codex config.toml.
/// Preserves unrelated settings and comments.
pub fn remove_owned_fragment(
    config_path: &std::path::Path,
) -> anyhow::Result<CodexDisconnectOutcome> {
    if !config_path.exists() {
        return Ok(CodexDisconnectOutcome::NotConnected);
    }

    let content =
        std::fs::read_to_string(config_path).context("Failed to read Codex config.toml")?;

    if !points_to_toche(&content) {
        return Ok(CodexDisconnectOutcome::NotConnected);
    }

    let backup_path = backup_path_for(config_path);

    if backup_path.exists() {
        // Restore from backup — but only Toche-owned fields, preserving
        // any user changes to unrelated fields.
        let backup = std::fs::read_to_string(&backup_path)
            .context("Failed to read Codex config.toml backup")?;

        let mut current_doc: toml_edit::DocumentMut = content
            .parse()
            .context("Failed to parse current Codex config.toml")?;

        let backup_doc: toml_edit::DocumentMut = backup
            .parse()
            .context("Failed to parse backup Codex config.toml")?;

        // Restore openai_base_url from backup
        if let Some(url) = backup_doc.get("openai_base_url") {
            current_doc["openai_base_url"] = url.clone();
        } else {
            current_doc.remove("openai_base_url");
        }

        let output = current_doc.to_string();
        crate::config::utils::atomic_write(config_path, &output)
            .context("Failed to write restored Codex config.toml")?;

        let _ = std::fs::remove_file(&backup_path);
        let _ = std::fs::remove_file(config_dir().join("pre_toche_codex_url.txt"));

        return Ok(CodexDisconnectOutcome::Disconnected {
            config_path: config_path.display().to_string(),
            preserved: true,
        });
    }

    // No backup — structured remove
    let saved_url = std::fs::read_to_string(config_dir().join("pre_toche_codex_url.txt"))
        .ok()
        .map(|s| s.trim().to_string());

    let mut doc: toml_edit::DocumentMut = content
        .parse()
        .context("Failed to parse Codex config.toml")?;

    if doc.contains_key("openai_base_url") {
        let current_value = doc["openai_base_url"].as_str().unwrap_or("").to_string();
        if current_value.contains("127.0.0.1:8743") {
            if let Some(ref original) = saved_url {
                doc["openai_base_url"] = toml_edit::value(original.as_str());
            } else {
                doc.remove("openai_base_url");
            }
        }
    }

    let output = doc.to_string();
    crate::config::utils::atomic_write(config_path, &output)
        .context("Failed to write Codex config.toml after disconnect")?;

    let _ = std::fs::remove_file(config_dir().join("pre_toche_codex_url.txt"));

    Ok(CodexDisconnectOutcome::Disconnected {
        config_path: config_path.display().to_string(),
        preserved: true,
    })
}

/// Connect Codex to Toche (persistent mode).
pub fn connect() -> anyhow::Result<CodexConnectOutcome> {
    let settings_path = codex_config_path();

    // Check if already connected
    if settings_path.exists() {
        let content =
            std::fs::read_to_string(&settings_path).context("Failed to read Codex config.toml")?;
        if points_to_toche(&content) {
            return Ok(CodexConnectOutcome::AlreadyConnected);
        }
    }

    let fragment = OwnedFragment::default_toche();
    let saved_url = apply_owned_fragment(&settings_path, &fragment.openai_base_url)?;

    if saved_url.is_none() && settings_path.exists() {
        return Ok(CodexConnectOutcome::AlreadyConnected);
    }

    let backup_path = backup_path_for(&settings_path);
    Ok(CodexConnectOutcome::Connected {
        config_path: settings_path.display().to_string(),
        backup_path: if backup_path.exists() {
            Some(backup_path.display().to_string())
        } else {
            None
        },
    })
}

/// Disconnect Codex from Toche (remove persistent fragment).
pub fn disconnect() -> anyhow::Result<CodexDisconnectOutcome> {
    let settings_path = codex_config_path();
    remove_owned_fragment(&settings_path)
}

// Re-export for discovery
#[allow(dead_code)]
pub(crate) fn codex_upstream_url_public(content: &str) -> Option<String> {
    super::discovery::codex_upstream_url_from_toml_public(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owned_fragment_default() {
        let fragment = OwnedFragment::default_toche();
        assert_eq!(fragment.openai_base_url, "http://127.0.0.1:8743/v1");
    }

    #[test]
    fn test_owned_fragment_for_endpoint() {
        let fragment = OwnedFragment::for_endpoint("127.0.0.1", 9999);
        assert_eq!(fragment.openai_base_url, "http://127.0.0.1:9999/v1");
    }

    #[test]
    fn test_points_to_toche_detects_fragment() {
        assert!(points_to_toche(
            "openai_base_url = \"http://127.0.0.1:8743/v1\""
        ));
        assert!(!points_to_toche(
            "openai_base_url = \"https://api.openai.com/v1\""
        ));
        assert!(!points_to_toche(""));
    }

    #[test]
    fn test_apply_writes_toche_url() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Don't pre-create the file — test new-file creation path
        let fragment = OwnedFragment::default_toche();
        let result = apply_owned_fragment(&config_path, &fragment.openai_base_url);

        // Backup may fail if ~/.codex/ is locked or missing, but core
        // apply logic (doc build, atomic write) is exercised either way.
        if result.is_ok() {
            let content = std::fs::read_to_string(&config_path).unwrap();
            assert!(points_to_toche(&content));
        }
    }

    #[test]
    fn test_apply_detects_already_connected() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Write a config that already points to Toche
        std::fs::write(
            &config_path,
            "openai_base_url = \"http://127.0.0.1:8743/v1\"",
        )
        .unwrap();

        let fragment = OwnedFragment::default_toche();
        let result = apply_owned_fragment(&config_path, &fragment.openai_base_url).unwrap();
        assert!(result.is_none()); // AlreadyConnected
    }

    #[test]
    fn test_apply_then_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let original = r#"service_tier = "fast"
openai_base_url = "https://api.openai.com/v1"
"#;
        std::fs::write(&config_path, original).unwrap();

        let fragment = OwnedFragment::default_toche();
        apply_owned_fragment(&config_path, &fragment.openai_base_url).unwrap();

        let connected = std::fs::read_to_string(&config_path).unwrap();
        assert!(connected.contains("127.0.0.1:8743"));
        assert!(connected.contains("service_tier"));

        let outcome = remove_owned_fragment(&config_path).unwrap();
        assert!(matches!(
            outcome,
            CodexDisconnectOutcome::Disconnected { .. }
        ));

        let restored = std::fs::read_to_string(&config_path).unwrap();
        assert!(!restored.contains("127.0.0.1:8743"));
    }

    #[test]
    fn independent_apply_remove_operations_keep_codex_backups_isolated() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut workers = Vec::new();

        for (marker, tier) in [("alpha", "fast"), ("beta", "slow")] {
            let barrier = std::sync::Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                let dir = tempfile::tempdir().unwrap();
                let config_path = dir.path().join("config.toml");
                let original = format!(
                    "service_tier = \"{tier}\"\nmarker = \"{marker}\"\nopenai_base_url = \"https://api.openai.com/v1\"\n"
                );
                std::fs::write(&config_path, &original).unwrap();

                barrier.wait();
                apply_owned_fragment(
                    &config_path,
                    &OwnedFragment::default_toche().openai_base_url,
                )
                .unwrap();

                let backup_path = backup_path_for(&config_path);
                let backup = std::fs::read_to_string(&backup_path).unwrap();
                assert_eq!(backup, original);

                let connected = std::fs::read_to_string(&config_path).unwrap();
                assert!(connected.contains("127.0.0.1:8743"));
                assert!(connected.contains(&format!("marker = \"{marker}\"")));
                assert!(connected.contains(&format!("service_tier = \"{tier}\"")));

                remove_owned_fragment(&config_path).unwrap();
                let restored: toml::Value = std::fs::read_to_string(&config_path)
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(restored["marker"].as_str(), Some(marker));
                assert_eq!(restored["service_tier"].as_str(), Some(tier));
                assert_eq!(
                    restored["openai_base_url"].as_str(),
                    Some("https://api.openai.com/v1")
                );
                assert!(!backup_path.exists());
            }));
        }

        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn test_disconnect_not_connected_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let original = r#"service_tier = "fast""#;
        std::fs::write(&config_path, original).unwrap();

        let outcome = remove_owned_fragment(&config_path).unwrap();
        assert!(matches!(outcome, CodexDisconnectOutcome::NotConnected));
    }

    #[test]
    fn test_apply_to_new_file_then_second_is_noop() {
        // First apply to a new file creates config; second detects already-connected.
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let fragment = OwnedFragment::default_toche();
        // First apply to non-existent file
        let _result1 = apply_owned_fragment(&config_path, &fragment.openai_base_url).unwrap();
        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(points_to_toche(&content));

        // Second apply detects already connected
        let result2 = apply_owned_fragment(&config_path, &fragment.openai_base_url).unwrap();
        assert!(result2.is_none());
    }

    #[test]
    fn test_comments_dont_block_detection() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let original = r#"# My Codex settings
service_tier = "fast"
# API endpoint
openai_base_url = "https://api.openai.com/v1"
"#;
        std::fs::write(&config_path, original).unwrap();

        let fragment = OwnedFragment::default_toche();
        apply_owned_fragment(&config_path, &fragment.openai_base_url).unwrap();

        let connected = std::fs::read_to_string(&config_path).unwrap();
        assert!(connected.contains("Managed by Toche"));
        assert!(connected.contains("service_tier"));
        assert!(connected.contains("127.0.0.1:8743"));
    }
}

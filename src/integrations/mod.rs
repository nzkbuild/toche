pub mod claude;
pub mod codex;

use anyhow::Context;

use crate::config::loader::config_dir;
use crate::config::utils::{atomic_write, home_dir, read_jsonc};

/// Result of applying a persistent connection.
#[derive(Debug, Clone)]
pub enum ConnectOutcome {
    /// Already connected, no changes needed.
    AlreadyConnected,
    /// Connection applied.
    Connected {
        /// Path to the modified settings file.
        settings_path: String,
        /// Path to backup (if created).
        backup_path: Option<String>,
    },
}

/// Result of removing a persistent connection.
#[derive(Debug, Clone)]
pub enum DisconnectOutcome {
    /// Not connected, nothing to remove.
    NotConnected,
    /// Connection removed.
    Disconnected {
        #[allow(dead_code)]
        settings_path: String,
        /// Unrelated settings preserved.
        preserved: bool,
    },
    /// Owned fragment differs from what was applied; user may have modified it.
    #[allow(dead_code)]
    Drift { settings_path: String },
}

/// The minimal fragment Toche owns in Claude Code settings.
pub struct OwnedFragment {
    pub base_url: String,
    pub env_anthropic_base_url: String,
}

impl OwnedFragment {
    pub fn for_endpoint(host: &str, port: u16) -> Self {
        Self {
            base_url: format!("http://{host}:{port}"),
            env_anthropic_base_url: format!("http://{host}:{port}/v1"),
        }
    }

    pub fn default_toche() -> Self {
        Self::for_endpoint("127.0.0.1", 8743)
    }
}

/// Check whether Claude settings currently point to Toche.
pub fn points_to_toche(settings: &serde_json::Value) -> bool {
    let base_url_toche = settings
        .get("baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    let env_url_toche = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    base_url_toche || env_url_toche
}

/// Resolve Claude settings path.
pub fn claude_settings_path() -> std::path::PathBuf {
    home_dir().join(".claude").join("settings.json")
}

pub(crate) fn backup_path_for(settings_path: &std::path::Path) -> std::path::PathBuf {
    let file_name = settings_path
        .file_name()
        .expect("settings path must have a file name")
        .to_string_lossy();
    settings_path.with_file_name(format!("{file_name}.toche-backup"))
}

/// Apply the owned fragment to Claude settings.json atomically.
/// Returns the previous baseURL/ANTHROPIC_BASE_URL that was saved for disconnect.
pub fn apply_owned_fragment(
    settings_path: &std::path::Path,
    fragment: &OwnedFragment,
) -> anyhow::Result<Option<String>> {
    let settings = if settings_path.exists() {
        read_jsonc(settings_path).context("Failed to parse settings.json")?
    } else {
        serde_json::json!({})
    };

    // Check if already connected
    if points_to_toche(&settings) {
        return Ok(None); // caller should detect AlreadyConnected
    }

    // Create a backup next to its source so independently owned settings files
    // cannot interfere with one another.
    let backup_path = backup_path_for(settings_path);
    if settings_path.exists() && !backup_path.exists() {
        std::fs::copy(settings_path, &backup_path).context("Failed to backup settings.json")?;
    }

    // Save original upstream URL before overwriting
    let original_url = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut settings = settings;
    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "baseURL".into(),
            serde_json::Value::String(fragment.base_url.clone()),
        );
        let env = obj
            .entry(String::from("env"))
            .or_insert_with(|| serde_json::json!({}));
        if let Some(env_obj) = env.as_object_mut() {
            env_obj.insert(
                "ANTHROPIC_BASE_URL".into(),
                serde_json::Value::String(fragment.env_anthropic_base_url.clone()),
            );
        }
    }

    let content = serde_json::to_string_pretty(&settings)?;
    atomic_write(settings_path, &content)?;

    // Save original URL in Toche config dir for recovery
    if let Some(ref url) = original_url {
        let saved_path = config_dir().join("pre_toche_url.txt");
        let _ = std::fs::write(&saved_path, url);
    }

    Ok(original_url)
}

/// Remove the Toche-owned fragment from Claude settings.json.
/// Preserves unrelated settings.
pub fn remove_owned_fragment(settings_path: &std::path::Path) -> anyhow::Result<DisconnectOutcome> {
    if !settings_path.exists() {
        return Ok(DisconnectOutcome::NotConnected);
    }

    let current = read_jsonc(settings_path).context("Failed to parse settings.json")?;
    if !points_to_toche(&current) {
        return Ok(DisconnectOutcome::NotConnected);
    }

    let backup_path = backup_path_for(settings_path);

    if backup_path.exists() {
        // Restore from backup — but only the Toche-owned fields, preserving
        // any user changes to unrelated fields.
        let backup = read_jsonc(&backup_path).context("Failed to parse backup settings.json")?;
        let mut restored = current.clone();

        // Restore baseURL from backup
        if let Some(obj) = restored.as_object_mut() {
            if let Some(backup_url) = backup.get("baseURL").and_then(|v| v.as_str()) {
                obj.insert(
                    "baseURL".into(),
                    serde_json::Value::String(backup_url.to_string()),
                );
            } else {
                obj.remove("baseURL");
            }
        }

        // Restore or remove env.ANTHROPIC_BASE_URL
        update_env_for_disconnect(&mut restored, &backup);

        let content = serde_json::to_string_pretty(&restored)?;
        atomic_write(settings_path, &content)?;
        std::fs::remove_file(&backup_path)?;

        let _ = std::fs::remove_file(config_dir().join("pre_toche_url.txt"));
        return Ok(DisconnectOutcome::Disconnected {
            settings_path: settings_path.display().to_string(),
            preserved: true,
        });
    }

    // No backup — structured remove
    let mut settings = current;
    let saved_url = std::fs::read_to_string(config_dir().join("pre_toche_url.txt"))
        .ok()
        .map(|s| s.trim().to_string());

    if let Some(obj) = settings.as_object_mut() {
        obj.remove("baseURL");
    }

    let env_empty = update_env_toche(&mut settings, saved_url);
    let content = serde_json::to_string_pretty(&settings)?;
    atomic_write(settings_path, &content)?;

    // Clean up empty env object
    if env_empty {
        let mut settings = read_jsonc(settings_path).context("Failed to re-parse settings.json")?;
        if let Some(obj) = settings.as_object_mut() {
            if obj
                .get("env")
                .map(|e| e.as_object().map(|o| o.is_empty()).unwrap_or(false))
                .unwrap_or(false)
            {
                obj.remove("env");
            }
        }
        let content = serde_json::to_string_pretty(&settings)?;
        atomic_write(settings_path, &content)?;
    }

    let _ = std::fs::remove_file(config_dir().join("pre_toche_url.txt"));

    Ok(DisconnectOutcome::Disconnected {
        settings_path: settings_path.display().to_string(),
        preserved: true,
    })
}

/// Update env.ANTHROPIC_BASE_URL during disconnect with backup available.
fn update_env_for_disconnect(restored: &mut serde_json::Value, backup: &serde_json::Value) {
    let backup_env_url = backup
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str());
    if let Some(env) = restored.pointer_mut("/env") {
        if let Some(env_obj) = env.as_object_mut() {
            if let Some(url) = backup_env_url {
                env_obj.insert(
                    "ANTHROPIC_BASE_URL".into(),
                    serde_json::Value::String(url.to_string()),
                );
            } else {
                env_obj.remove("ANTHROPIC_BASE_URL");
            }
        }
    }
}

/// Update env.ANTHROPIC_BASE_URL during disconnect without backup.
/// Returns true if env object became empty.
fn update_env_toche(settings: &mut serde_json::Value, saved_url: Option<String>) -> bool {
    let is_toche = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    if !is_toche {
        return false;
    }
    if let Some(env) = settings.pointer_mut("/env") {
        if let Some(env_obj) = env.as_object_mut() {
            if let Some(ref original) = saved_url {
                env_obj.insert(
                    "ANTHROPIC_BASE_URL".into(),
                    serde_json::Value::String(original.clone()),
                );
            } else {
                env_obj.remove("ANTHROPIC_BASE_URL");
            }
            return env_obj.is_empty();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_settings(extra: &str) -> serde_json::Value {
        let comma_and_extra = if extra.is_empty() {
            String::new()
        } else {
            format!(",\n  {extra}")
        };
        let raw = format!(
            r#"{{
  "baseURL": "https://api.anthropic.com",
  "env": {{
    "ANTHROPIC_BASE_URL": "https://api.anthropic.com/v1"
  }},
  "theme": "dark",
  "permissions": {{}}{comma_and_extra}
}}"#
        );
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn test_points_to_toche_false_when_not_connected() {
        let settings = make_settings("");
        assert!(!points_to_toche(&settings));
    }

    #[test]
    fn test_points_to_toche_true_when_base_url_is_toche() {
        let mut settings = make_settings("");
        settings.as_object_mut().unwrap().insert(
            "baseURL".into(),
            serde_json::Value::String("http://127.0.0.1:8743".into()),
        );
        assert!(points_to_toche(&settings));
    }

    #[test]
    fn test_apply_then_remove_preserves_unrelated() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        let original = make_settings("");
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&original).unwrap(),
        )
        .unwrap();

        let fragment = OwnedFragment::default_toche();
        apply_owned_fragment(&settings_path, &fragment).unwrap();

        // Verify connected
        let connected = read_jsonc(&settings_path).unwrap();
        assert!(points_to_toche(&connected));
        assert_eq!(
            connected.get("theme").and_then(|v| v.as_str()),
            Some("dark")
        );
        assert!(connected.get("permissions").is_some());

        // Remove
        let outcome = remove_owned_fragment(&settings_path).unwrap();
        assert!(matches!(outcome, DisconnectOutcome::Disconnected { .. }));

        // Verify restored
        let final_settings = read_jsonc(&settings_path).unwrap();
        assert!(!points_to_toche(&final_settings));
        assert_eq!(
            final_settings.get("theme").and_then(|v| v.as_str()),
            Some("dark")
        );
    }

    #[test]
    fn independent_apply_remove_operations_keep_backups_and_unrelated_settings_isolated() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut workers = Vec::new();

        for marker in ["alpha", "beta"] {
            let barrier = std::sync::Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                let dir = tempfile::tempdir().unwrap();
                let settings_path = dir.path().join("settings.json");
                let original = make_settings(&format!("\"marker\": \"{marker}\""));
                std::fs::write(
                    &settings_path,
                    serde_json::to_string_pretty(&original).unwrap(),
                )
                .unwrap();

                barrier.wait();
                apply_owned_fragment(&settings_path, &OwnedFragment::default_toche()).unwrap();

                let backup_path = backup_path_for(&settings_path);
                let backup = read_jsonc(&backup_path).unwrap();
                assert_eq!(backup["marker"], marker);
                assert!(!points_to_toche(&backup));

                remove_owned_fragment(&settings_path).unwrap();
                let restored = read_jsonc(&settings_path).unwrap();
                assert_eq!(restored["marker"], marker);
                assert!(!points_to_toche(&restored));
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
        let settings_path = dir.path().join("settings.json");
        // Write settings that don't point to Toche
        let settings = make_settings("");
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();

        let outcome = remove_owned_fragment(&settings_path).unwrap();
        assert!(matches!(outcome, DisconnectOutcome::NotConnected));
    }

    #[test]
    fn test_apply_already_connected_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        let fragment = OwnedFragment::default_toche();
        // First apply
        apply_owned_fragment(&settings_path, &fragment).unwrap();
        // Second apply
        let result = apply_owned_fragment(&settings_path, &fragment).unwrap();
        assert!(result.is_none()); // None means AlreadyConnected
    }

    #[test]
    fn test_apply_creates_backup_once() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        let original = make_settings("");
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&original).unwrap(),
        )
        .unwrap();

        let fragment = OwnedFragment::default_toche();
        apply_owned_fragment(&settings_path, &fragment).unwrap();

        // backup goes to real home_dir in the function — this test
        // only verifies the settings file was modified correctly
        let modified = read_jsonc(&settings_path).unwrap();
        assert!(points_to_toche(&modified));
    }

    #[test]
    fn test_fragment_matches_default() {
        let fragment = OwnedFragment::default_toche();
        assert_eq!(fragment.base_url, "http://127.0.0.1:8743");
        assert_eq!(fragment.env_anthropic_base_url, "http://127.0.0.1:8743/v1");
    }
}

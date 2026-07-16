use anyhow::Context;

use crate::config::utils;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => disconnect_claude().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude"),
    }
}

async fn disconnect_claude() -> anyhow::Result<()> {
    let settings_path = utils::home_dir().join(".claude").join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Verify current settings actually point to Toche before restoring
    if settings_path.exists() {
        let current = utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?;
        if !super::connect::points_to_toche(&current) {
            println!("settings.json does not point to Toche. Nothing to disconnect.");
            return Ok(());
        }
    }

    if backup_path.exists() {
        std::fs::copy(&backup_path, &settings_path)
            .context("Failed to restore settings.json backup")?;
        std::fs::remove_file(&backup_path)?;
        println!("Restored previous Claude Code configuration.");
    } else {
        // No backup — remove Toche URL entries
        let mut settings =
            utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?;
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("baseURL");
        }

        // Retrieve the saved original URL if available (F4)
        let saved_url = std::fs::read_to_string(
            crate::profiles::loader::config_dir().join("pre_toche_url.txt"),
        )
        .ok()
        .map(|s| s.trim().to_string());

        // Replace or remove env.ANTHROPIC_BASE_URL if it points to Toche
        let env_is_empty_after = update_env_url(&mut settings, saved_url);
        let content = serde_json::to_string_pretty(&settings)?;
        utils::atomic_write(&settings_path, &content)?;

        // Clean up empty env object (F5) and saved URL file
        if env_is_empty_after {
            // Re-read, remove empty env, re-write
            let mut settings =
                utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?;
            if let Some(obj) = settings.as_object_mut() {
                if obj.get("env").map(|e| e.as_object().map(|o| o.is_empty()).unwrap_or(false)).unwrap_or(false) {
                    obj.remove("env");
                }
            }
            let content = serde_json::to_string_pretty(&settings)?;
            utils::atomic_write(&settings_path, &content)?;
        }
        let _ = std::fs::remove_file(
            crate::profiles::loader::config_dir().join("pre_toche_url.txt"),
        );
        println!("Removed Toche baseURL from Claude Code settings.");
    }

    Ok(())
}

/// Modify the env.ANTHROPIC_BASE_URL field to restore the original or remove
/// the Toche URL. Returns true if the env object became empty (caller should
/// clean it up to avoid leaving `"env": {}`).
fn update_env_url(settings: &mut serde_json::Value, saved_url: Option<String>) -> bool {
    let _env_url = match settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
    {
        Some(url) if url.contains("127.0.0.1:8743") => url.to_string(),
        _ => return false,
    };

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

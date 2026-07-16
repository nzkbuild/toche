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
        // Also clear env.ANTHROPIC_BASE_URL if pointed at Toche
        if let Some(env_url) = settings
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
        {
            if env_url.contains("127.0.0.1:8743") {
                if let Some(env) = settings.pointer_mut("/env") {
                    if let Some(env_obj) = env.as_object_mut() {
                        env_obj.remove("ANTHROPIC_BASE_URL");
                    }
                }
            }
        }
        let content = serde_json::to_string_pretty(&settings)?;
        utils::atomic_write(&settings_path, &content)?;
        println!("Removed Toche baseURL from Claude Code settings.");
    }

    Ok(())
}

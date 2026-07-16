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
    let settings_path = utils::home_dir()
        .join(".claude")
        .join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Verify current settings actually point to Toche before restoring
    if settings_path.exists() {
        let current = utils::read_jsonc(&settings_path)
            .context("Failed to parse settings.json")?;
        let points_to_toche = current
            .get("baseURL")
            .and_then(|v| v.as_str())
            .map(|s| s.contains("127.0.0.1:8743"))
            .unwrap_or(false);
        if !points_to_toche {
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
        // No backup — remove the baseURL field Toche added
        let mut settings = utils::read_jsonc(&settings_path)
            .context("Failed to parse settings.json")?;
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("baseURL");
        }
        let content = serde_json::to_string_pretty(&settings)?;
        utils::atomic_write(&settings_path, &content)?;
        println!("Removed Toche baseURL from Claude Code settings.");
    }

    Ok(())
}

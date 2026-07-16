use anyhow::Context;

use crate::config::utils;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => connect_claude().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude"),
    }
}

async fn connect_claude() -> anyhow::Result<()> {
    let settings_path = utils::home_dir().join(".claude").join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Check if already connected
    if settings_path.exists() {
        let current = utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?;
        let already_toche = current
            .get("baseURL")
            .and_then(|v| v.as_str())
            .map(|s| s.contains("127.0.0.1:8743"))
            .unwrap_or(false);
        if already_toche {
            println!("Claude Code is already connected to Toche.");
            return Ok(());
        }
        // Only backup if NOT already connected to Toche
        std::fs::copy(&settings_path, &backup_path).context("Failed to backup settings.json")?;
    }

    // Read settings (JSONC-tolerant)
    let mut settings = if settings_path.exists() {
        utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?
    } else {
        serde_json::json!({})
    };

    // Set Toche as base URL
    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "baseURL".into(),
            serde_json::Value::String("http://127.0.0.1:8743".into()),
        );
    }

    // Atomic write back
    let content = serde_json::to_string_pretty(&settings)?;
    utils::atomic_write(&settings_path, &content)?;

    println!("Claude Code now routing through Toche (http://127.0.0.1:8743).");
    println!("Backup saved to: {}", backup_path.display());
    Ok(())
}

use anyhow::Context;

use crate::config::utils::{atomic_write, read_jsonc};

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => connect_claude().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude"),
    }
}

async fn connect_claude() -> anyhow::Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");

    // 1. Backup existing settings
    let backup_path = settings_path.with_extension("json.toche-backup");
    if settings_path.exists() {
        std::fs::copy(&settings_path, &backup_path)
            .context("Failed to backup settings.json")?;
    }

    // 2. Read settings (JSONC-tolerant)
    let mut settings = if settings_path.exists() {
        read_jsonc(&settings_path).context("Failed to parse settings.json")?
    } else {
        serde_json::json!({})
    };

    // 3. Set Toche as base URL
    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "baseURL".into(),
            serde_json::Value::String("http://127.0.0.1:8743".into()),
        );
    }

    // 4. Atomic write back
    let content = serde_json::to_string_pretty(&settings)?;
    atomic_write(&settings_path, &content)?;

    println!("Claude Code now routing through Toche (http://127.0.0.1:8743).");
    println!("Backup saved to: {}", backup_path.display());
    Ok(())
}

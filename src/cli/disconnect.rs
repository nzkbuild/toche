use anyhow::Context;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => disconnect_claude().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude"),
    }
}

async fn disconnect_claude() -> anyhow::Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    if backup_path.exists() {
        std::fs::copy(&backup_path, &settings_path)
            .context("Failed to restore settings.json backup")?;
        std::fs::remove_file(&backup_path)?;
        println!("Restored previous Claude Code configuration.");
    } else {
        println!("No Toche backup found — settings.json was not modified by Toche.");
    }

    Ok(())
}

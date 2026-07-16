use anyhow::Context;

use crate::config::utils;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => connect_claude().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude"),
    }
}

const TOCHE_URL: &str = "http://127.0.0.1:8743";

pub(crate) fn points_to_toche(settings: &serde_json::Value) -> bool {
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

async fn connect_claude() -> anyhow::Result<()> {
    let settings_path = utils::home_dir().join(".claude").join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Verify the gateway is running and ready before modifying anything.
    let ready_url = format!("{TOCHE_URL}/ready");
    match reqwest::get(&ready_url).await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let status = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if status != "ready" {
                let checks: Vec<&str> = body
                    .get("checks")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|c| c.as_str()).collect())
                    .unwrap_or_default();
                anyhow::bail!(
                    "Toche gateway is running but not ready.\n  {}\n  Fix the issues above and try again.",
                    checks.join("\n  ")
                );
            }
        }
        _ => {
            anyhow::bail!(
                "Toche gateway is not running at {TOCHE_URL}. Run `toche` without arguments to start it first."
            );
        }
    }

    // Check if already connected
    if settings_path.exists() {
        let current = utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?;
        if points_to_toche(&current) {
            println!("Claude Code is already connected to Toche.");
            return Ok(());
        }
        // Only create a backup if one doesn't already exist — the first
        // backup is the user's original upstream config and must not be
        // overwritten by subsequent connect runs.
        if !backup_path.exists() {
            std::fs::copy(&settings_path, &backup_path)
                .context("Failed to backup settings.json")?;
        }
    }

    // Read settings (JSONC-tolerant)
    let mut settings = if settings_path.exists() {
        utils::read_jsonc(&settings_path).context("Failed to parse settings.json")?
    } else {
        serde_json::json!({})
    };

    // Set Toche as base URL (both top-level and env block — env takes precedence
    // in Claude Code, but we set both so disconnect can restore cleanly).
    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "baseURL".into(),
            serde_json::Value::String(TOCHE_URL.into()),
        );
        // Also set env.ANTHROPIC_BASE_URL since it overrides top-level baseURL
        let env = obj
            .entry(String::from("env"))
            .or_insert_with(|| serde_json::json!({}));
        if let Some(env_obj) = env.as_object_mut() {
            env_obj.insert(
                "ANTHROPIC_BASE_URL".into(),
                serde_json::Value::String(format!("{TOCHE_URL}/v1")),
            );
        }
    }

    // Atomic write back
    let content = serde_json::to_string_pretty(&settings)?;
    utils::atomic_write(&settings_path, &content)?;

    println!("Claude Code now routing through Toche ({TOCHE_URL}).");
    println!("Backup saved to: {}", backup_path.display());
    Ok(())
}

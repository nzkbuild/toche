use crate::integrations::ConnectOutcome;
use crate::integrations::claude;
use crate::integrations::codex;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => connect_claude().await,
        "codex" => connect_codex().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude, codex"),
    }
}

async fn connect_claude() -> anyhow::Result<()> {
    // Pre-connect gateway readiness check
    let ready_url = "http://127.0.0.1:8743/ready";
    match reqwest::get(ready_url).await {
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
                "Toche gateway is not running at http://127.0.0.1:8743. Run `toche` without arguments to start it first."
            );
        }
    }

    match claude::config::connect()? {
        ConnectOutcome::AlreadyConnected => {
            println!("Claude Code is already connected to Toche.");
        }
        ConnectOutcome::Connected {
            settings_path,
            backup_path,
        } => {
            println!("Claude Code now routing through Toche (http://127.0.0.1:8743).");
            if let Some(bak) = backup_path {
                println!("Backup saved to: {bak}");
            }
            let _ = &settings_path; // used in message context
        }
    }

    Ok(())
}

async fn connect_codex() -> anyhow::Result<()> {
    // Pre-connect gateway readiness check
    let ready_url = "http://127.0.0.1:8743/ready";
    match reqwest::get(ready_url).await {
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
                "Toche gateway is not running at http://127.0.0.1:8743. Run `toche` without arguments to start it first."
            );
        }
    }

    match codex::config::connect()? {
        codex::config::CodexConnectOutcome::AlreadyConnected => {
            println!("Codex is already connected to Toche.");
        }
        codex::config::CodexConnectOutcome::Connected {
            config_path,
            backup_path,
        } => {
            println!("Codex now routing through Toche (http://127.0.0.1:8743).");
            if let Some(bak) = backup_path {
                println!("Backup saved to: {bak}");
            }
            let _ = &config_path;
        }
    }

    Ok(())
}

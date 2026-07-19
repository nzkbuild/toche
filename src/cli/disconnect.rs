use crate::integrations::DisconnectOutcome;
use crate::integrations::claude;
use crate::integrations::codex;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => disconnect_claude().await,
        "codex" => disconnect_codex().await,
        _ => anyhow::bail!("Unknown agent: {agent}. Supported: claude, codex"),
    }
}

async fn disconnect_claude() -> anyhow::Result<()> {
    match claude::config::disconnect()? {
        DisconnectOutcome::NotConnected => {
            println!("Claude Code is not connected to Toche. Nothing to disconnect.");
        }
        DisconnectOutcome::Disconnected { preserved, .. } => {
            println!("Removed Toche routing from Claude Code settings.");
            if preserved {
                println!("Unrelated settings preserved.");
            }
        }
        DisconnectOutcome::Drift { .. } => {
            println!("Toche-owned fragment has been modified since setup.");
            println!(
                "Run `toche setup` to repair, or `toche disconnect` with --force to override."
            );
        }
    }

    Ok(())
}

async fn disconnect_codex() -> anyhow::Result<()> {
    match codex::config::disconnect()? {
        codex::config::CodexDisconnectOutcome::NotConnected => {
            println!("Codex is not connected to Toche. Nothing to disconnect.");
        }
        codex::config::CodexDisconnectOutcome::Disconnected { preserved, .. } => {
            println!("Removed Toche routing from Codex config.");
            if preserved {
                println!("Unrelated settings preserved.");
            }
        }
    }

    Ok(())
}

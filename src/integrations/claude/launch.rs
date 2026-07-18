use std::collections::HashMap;
use std::process::ExitStatus;

use anyhow::Context;
use tokio::process::Command;

/// Managed launch result.
pub struct ManagedLaunch {
    pub exit_status: ExitStatus,
    #[allow(dead_code)]
    pub used_existing_runtime: bool,
}

/// Resolve the `claude` executable path.
fn resolve_claude_executable() -> anyhow::Result<String> {
    which::which("claude")
        .map(|p| p.display().to_string())
        .context("Claude Code executable not found. Is claude installed?")
}

/// Build a child-only environment for managed mode.
/// Reads the current process environment, adds the Toche endpoint, and
/// removes upstream auth fields that Toche handles.
fn build_child_env(toche_port: u16) -> anyhow::Result<HashMap<String, String>> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    let toche_endpoint = format!("http://127.0.0.1:{toche_port}");

    // Route Claude to Toche
    env.insert("ANTHROPIC_BASE_URL".into(), format!("{toche_endpoint}/v1"));

    Ok(env)
}

/// Check whether a Toche runtime is already healthy at the configured port.
async fn runtime_is_healthy(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    match reqwest::get(&url).await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Check whether a port is already occupied by a non-Toche process.
async fn port_is_occupied(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    match reqwest::get(&url).await {
        Ok(_) => !runtime_is_healthy(port).await,
        Err(_) => {
            // Try binding to check port occupancy
            tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
                .await
                .is_err()
        }
    }
}

/// Run Claude Code in managed mode.
///
/// Arguments:
/// - `claude_args`: forwarded Claude arguments (after `--`)
/// - `toche_port`: the Toche gateway port (default 8743)
pub async fn run_managed(
    claude_args: Vec<String>,
    toche_port: u16,
) -> anyhow::Result<ManagedLaunch> {
    let claude_bin = resolve_claude_executable()?;

    // Detect runtime state
    let existing_runtime = runtime_is_healthy(toche_port).await;

    if !existing_runtime && port_is_occupied(toche_port).await {
        anyhow::bail!(
            "Port {toche_port} is occupied by a non-Toche process. \
             Stop that process or configure a different port in config.toml."
        );
    }

    let mut spawned_runtime = None;
    let used_existing_runtime = existing_runtime;

    if !existing_runtime {
        // Start a temporary in-process runtime
        let toche_bin = std::env::current_exe().context("Cannot resolve Toche executable path")?;
        let child = Command::new(&toche_bin)
            .kill_on_drop(true)
            .spawn()
            .context("Failed to start Toche runtime")?;

        // Wait for runtime to be ready
        for _ in 0..30 {
            if runtime_is_healthy(toche_port).await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        if !runtime_is_healthy(toche_port).await {
            anyhow::bail!("Toche runtime did not become ready within 6 seconds");
        }

        spawned_runtime = Some(child);
    }

    // Build child environment
    let child_env = build_child_env(toche_port)?;

    // Spawn Claude
    let mut cmd = Command::new(&claude_bin);
    cmd.args(&claude_args);
    cmd.env_clear();
    for (key, value) in &child_env {
        cmd.env(key, value);
    }

    let status = cmd.status().await.context("Failed to spawn Claude Code")?;

    // Stop temporary runtime if we started it
    if let Some(mut child) = spawned_runtime {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    Ok(ManagedLaunch {
        exit_status: status,
        used_existing_runtime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragment_arguments_are_forwarded() {
        // This is a unit test — doesn't actually spawn anything
        let fragment = super::super::super::OwnedFragment::default_toche();
        assert_eq!(fragment.base_url, "http://127.0.0.1:8743");
        assert_eq!(fragment.env_anthropic_base_url, "http://127.0.0.1:8743/v1");
    }

    #[test]
    fn test_resolve_claude_not_found_error() {
        // Sanity: if claude not on PATH, error should be descriptive
        if which::which("claude").is_err() {
            let err = resolve_claude_executable().unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("claude") || msg.contains("not found"));
        }
    }
}

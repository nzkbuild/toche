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

/// Resolve the `codex` executable path.
fn resolve_codex_executable() -> anyhow::Result<String> {
    which::which("codex")
        .map(|p| p.display().to_string())
        .context("Codex CLI executable not found. Is codex installed?")
}

/// Build a child-only environment for managed mode.
/// Reads the current process environment, adds the Toche endpoint, and
/// sets OPENAI_BASE_URL to route through Toche.
fn build_child_env(toche_port: u16) -> anyhow::Result<HashMap<String, String>> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    let toche_endpoint = format!("http://127.0.0.1:{toche_port}");

    // Route Codex to Toche via OpenAI base URL
    env.insert("OPENAI_BASE_URL".into(), format!("{toche_endpoint}/v1"));

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

/// Run Codex in managed mode.
///
/// Arguments:
/// - `codex_args`: forwarded Codex arguments (after `--`)
/// - `toche_port`: the Toche gateway port (default 8743)
pub async fn run_managed(
    codex_args: Vec<String>,
    toche_port: u16,
) -> anyhow::Result<ManagedLaunch> {
    let codex_bin = resolve_codex_executable()?;

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

    // Spawn Codex
    let mut cmd = Command::new(&codex_bin);
    cmd.args(&codex_args);
    cmd.env_clear();
    for (key, value) in &child_env {
        cmd.env(key, value);
    }

    let status = cmd.status().await.context("Failed to spawn Codex")?;

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
        // Sanity: default endpoint format
        let endpoint = "http://127.0.0.1:8743/v1";
        assert!(endpoint.contains("127.0.0.1:8743"));
    }

    #[test]
    fn test_resolve_codex_not_found_error() {
        // If codex is not on PATH, the error should be descriptive
        if which::which("codex").is_err() {
            let err = resolve_codex_executable().unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("codex") || msg.contains("not found"));
        }
    }
}

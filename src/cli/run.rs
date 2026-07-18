use crate::integrations::claude::launch;

pub async fn run(client: &str, args: Vec<String>) -> anyhow::Result<()> {
    match client {
        "claude" => {
            let result = launch::run_managed(args, 8743).await?;
            // Propagate Claude's exit code
            if !result.exit_status.success() {
                let code = result.exit_status.code().unwrap_or(1);
                std::process::exit(code);
            }
            Ok(())
        }
        _ => {
            anyhow::bail!("Unknown client: {client}. Supported: claude")
        }
    }
}

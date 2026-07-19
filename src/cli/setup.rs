use tracing::info;

use crate::setup::{SetupOutcome, SetupTransaction};

pub async fn run(force: bool, dry_run: bool, json: bool) -> anyhow::Result<()> {
    info!("Starting Toche setup...");

    let tx = SetupTransaction::new(true, force)
        .with_dry_run(dry_run)
        .with_json(json);

    match tx.run()? {
        SetupOutcome::NoOp => {
            println!("Setup complete — no changes were necessary.");
        }
        SetupOutcome::DryRun { .. } => {
            // Preview was already printed by the transaction engine
        }
        SetupOutcome::Applied { config, record } => {
            let default_name = config
                .defaults
                .integration
                .as_ref()
                .and_then(|id| config.integrations.iter().find(|i| i.id == *id))
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "default".into());
            println!("Setup applied. Default integration: {default_name}");
            println!("Ownership record: {record:?}");
        }
    }

    detect_graphify();

    Ok(())
}

fn detect_graphify() {
    println!();
    let on_path = which::which("graphify").is_ok();
    let uv_path = crate::config::utils::home_dir()
        .join(".local")
        .join("bin")
        .join("graphify");

    if on_path || uv_path.exists() {
        println!(
            "Detected Graphify at: {}",
            which::which("graphify")
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| uv_path.display().to_string())
        );
        println!("Add [graphify] section to the integration in config.toml to configure:");
        println!("  [integrations.graphify]");
        println!("  enabled = true");
        println!("  # graph_path = \"path/to/graph.json\"  # if non-default");
        println!("  # auto_extract = false");
    } else {
        println!("Graphify not found.");
        println!("To enable knowledge graph queries, install Graphify:");
        println!("  uv tool install graphifyy");
        println!("  # or: pipx install graphifyy");
        println!("Then add a [graphify] section to the integration in config.toml.");
    }
}

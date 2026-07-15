use clap::{Parser, Subcommand};

mod cli;
mod config;
mod gateway;
mod meter;
mod profiles;

#[derive(Parser)]
#[command(name = "toche", about = "Local context-efficiency gateway for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Import existing Claude Code gateway configuration
    Setup,
    /// Point Claude Code to Toche
    Connect { agent: Option<String> },
    /// Restore Claude Code to direct upstream
    Disconnect { agent: Option<String> },
    /// Verify Toche installation and configuration
    Doctor,
    /// Show gateway status
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "toche=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Setup) => cli::setup::run().await,
        Some(Commands::Connect { agent }) => cli::connect::run(agent.as_deref()).await,
        Some(Commands::Disconnect { agent }) => cli::disconnect::run(agent.as_deref()).await,
        Some(Commands::Doctor) => cli::doctor::run().await,
        Some(Commands::Status) => cli::status::run().await,
        None => gateway::serve().await,
    }
}

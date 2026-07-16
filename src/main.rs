use clap::{Parser, Subcommand};

mod cache;
mod cli;
mod config;
mod efficiency;
mod gateway;
mod meter;
mod profiles;
mod reduce;
mod shield;

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
    /// Show usage statistics and cost breakdown
    Stats {
        /// Output in machine-readable JSON format
        #[arg(long)]
        json: bool,
        /// Show recent entries (last N, default 50)
        #[arg(long, default_value = "50")]
        entries: u32,
    },
    /// Restore original tool output from a reduction hash
    Expand {
        /// Hex-encoded SHA-256 hash of the original content
        hash: String,
        /// Output in machine-readable JSON format
        #[arg(long)]
        json: bool,
    },
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
        Some(Commands::Stats { json, entries }) => cli::stats::run(json, entries).await,
        Some(Commands::Expand { hash, json }) => cli::expand::run(hash, json).await,
        None => gateway::serve().await,
    }
}

use clap::{Parser, Subcommand};

mod cache;
mod cli;
mod config;
mod continuity;
mod efficiency;
mod gateway;
mod meter;
mod profiles;
mod reduce;
mod safe_cache;
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
    /// Manage the persistent safe cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Save, view, and manage session checkpoints
    Checkpoint {
        #[command(subcommand)]
        action: CheckpointAction,
    },
}

#[derive(Subcommand)]
enum CacheAction {
    /// List cache entries
    Inspect {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "50")]
        entries: u32,
    },
    /// Clear cache entries
    Clear {
        /// Only clear entries for the current project (default if no flag)
        #[arg(long)]
        project: bool,
        /// Clear all cache entries (all projects)
        #[arg(long)]
        all: bool,
    },
    /// Explain why a specific request was/wasn't cached
    Why {
        /// Hex-encoded SHA-256 request fingerprint
        fingerprint: String,
    },
}

#[derive(Subcommand)]
enum CheckpointAction {
    /// Save a new checkpoint
    Save {
        #[arg(long)]
        task: Option<String>,
        #[arg(long, value_delimiter = ',')]
        completed: Option<Vec<String>>,
        #[arg(long)]
        next: Option<String>,
        #[arg(long, value_delimiter = ',')]
        changed_files: Option<Vec<String>>,
        #[arg(long)]
        verification: Option<String>,
        #[arg(long, value_delimiter = ',')]
        open_risks: Option<Vec<String>>,
        #[arg(long)]
        model_assisted: bool,
    },
    /// Show a checkpoint (latest by default)
    Show {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// List checkpoints for the current project
    List {
        #[arg(long)]
        json: bool,
    },
    /// Delete a checkpoint by ID
    Delete {
        id: i64,
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
        Some(Commands::Cache { action }) => match action {
            CacheAction::Inspect { json, entries } => cli::cache::run_inspect(json, entries).await,
            CacheAction::Clear { project, all } => cli::cache::run_clear(project, all).await,
            CacheAction::Why { fingerprint } => cli::cache::run_why(&fingerprint).await,
        },
        Some(Commands::Checkpoint { action }) => match action {
            CheckpointAction::Save {
                task,
                completed,
                next,
                changed_files,
                verification,
                open_risks,
                model_assisted,
            } => {
                cli::checkpoint::run_save(
                    task,
                    completed,
                    next,
                    changed_files,
                    verification,
                    open_risks,
                    model_assisted,
                )
                .await
            }
            CheckpointAction::Show { id, json } => cli::checkpoint::run_show(id, json).await,
            CheckpointAction::List { json } => cli::checkpoint::run_list(json).await,
            CheckpointAction::Delete { id } => cli::checkpoint::run_delete(id).await,
        },
        None => gateway::serve().await,
    }
}

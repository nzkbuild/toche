use clap::{Parser, Subcommand};

mod cache;
mod cli;
mod config;
mod continuity;
mod efficiency;
mod gateway;
mod graphify;
mod identity;
mod integrations;
mod meter;
mod profiles;
mod protocol;
mod reduce;
mod safe_cache;
mod setup;
mod shield;

#[derive(Parser)]
#[command(
    name = "toche",
    version,
    about = "Local context-efficiency gateway for Claude Code and Codex"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure Toche integrations and upstreams
    Setup {
        /// Force overwrite of existing config.toml (backup is created)
        #[arg(long)]
        force: bool,
        /// Preview changes without writing anything
        #[arg(long)]
        dry_run: bool,
        /// Output machine-readable JSON (use with --dry-run)
        #[arg(long)]
        json: bool,
    },
    /// Route a client through Toche (persistent mode)
    Connect {
        /// Client to connect (default: claude, supported: claude, codex)
        agent: Option<String>,
    },
    /// Remove Toche routing from a client
    Disconnect {
        /// Client to disconnect (default: claude, supported: claude, codex)
        agent: Option<String>,
    },
    /// Run a client in managed mode through Toche
    Run {
        /// Client to run (supported: claude, codex)
        client: String,
        /// Arguments to forward to the client
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Verify Toche installation and configuration
    Doctor,
    /// Show gateway status
    Status {
        /// Output in machine-readable JSON format
        #[arg(long)]
        json: bool,
    },
    /// Show usage statistics and cost breakdown
    Stats {
        /// Output in machine-readable JSON format
        #[arg(long)]
        json: bool,
        /// Show recent entries (last N, default 50)
        #[arg(long, default_value = "50")]
        entries: u32,
        /// Filter by protocol (anthropic, openai-responses)
        #[arg(long)]
        protocol: Option<String>,
        /// Filter by integration name
        #[arg(long)]
        integration: Option<String>,
        /// Filter by trust domain hash
        #[arg(long)]
        trust_domain: Option<String>,
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
    /// Show storage status and CAS usage
    Storage {
        /// Output in machine-readable JSON format
        #[arg(long)]
        json: bool,
    },
    /// List orphan candidates (--dry-run) or delete them (--orphans)
    Cleanup {
        /// Dry-run: list orphan candidates only, do not delete
        #[arg(long, group = "action")]
        dry_run: bool,
        /// Delete confirmed orphan files
        #[arg(long, group = "action")]
        orphans: bool,
        /// Output in machine-readable JSON format
        #[arg(long)]
        json: bool,
    },
    /// Save, view, and manage session checkpoints
    Checkpoint {
        #[command(subcommand)]
        action: CheckpointAction,
    },
    /// Interact with the Graphify knowledge graph
    Graph {
        #[command(subcommand)]
        action: GraphAction,
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
    Delete { id: i64 },
}

#[derive(Subcommand)]
enum GraphAction {
    /// Query the knowledge graph
    Query {
        question: String,
        /// Token budget for response (default 2000)
        #[arg(long)]
        budget: Option<u32>,
    },
    /// Find the shortest path between two concepts
    Path { source: String, target: String },
    /// Explain a node (source file, community, connections)
    Explain { node: String },
    /// Show nodes affected by a change to a node
    Affected {
        node: String,
        /// Search depth
        #[arg(long)]
        depth: Option<u32>,
    },
    /// Show graph status (node/edge counts)
    Status,
    /// Build/rebuild the knowledge graph
    Extract,
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
        Some(Commands::Setup {
            force,
            dry_run,
            json,
        }) => cli::setup::run(force, dry_run, json).await,
        Some(Commands::Connect { agent }) => cli::connect::run(agent.as_deref()).await,
        Some(Commands::Disconnect { agent }) => cli::disconnect::run(agent.as_deref()).await,
        Some(Commands::Run { client, args }) => cli::run::run(&client, args).await,
        Some(Commands::Doctor) => cli::doctor::run().await,
        Some(Commands::Status { json }) => cli::status::run(json).await,
        Some(Commands::Stats {
            json,
            entries,
            protocol,
            integration,
            trust_domain,
        }) => {
            cli::stats::run(
                json,
                entries,
                protocol.as_deref(),
                integration.as_deref(),
                trust_domain.as_deref(),
            )
            .await
        }
        Some(Commands::Expand { hash, json }) => cli::expand::run(hash, json).await,
        Some(Commands::Cache { action }) => match action {
            CacheAction::Inspect { json, entries } => cli::cache::run_inspect(json, entries).await,
            CacheAction::Clear { project, all } => cli::cache::run_clear(project, all).await,
            CacheAction::Why { fingerprint } => cli::cache::run_why(&fingerprint).await,
        },
        Some(Commands::Storage { json }) => cli::storage::run_storage_status(json).await,
        Some(Commands::Cleanup {
            dry_run,
            orphans,
            json,
        }) => match (dry_run, orphans) {
            (true, false) => cli::storage::run_cleanup_dry_run(json).await,
            (false, true) => cli::storage::run_cleanup_orphans(json).await,
            _ => {
                anyhow::bail!("Exactly one of --dry-run or --orphans is required.");
            }
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
        Some(Commands::Graph { action }) => match action {
            GraphAction::Query { question, budget } => {
                cli::graph::run_query(question, budget).await
            }
            GraphAction::Path { source, target } => cli::graph::run_path(source, target).await,
            GraphAction::Explain { node } => cli::graph::run_explain(node).await,
            GraphAction::Affected { node, depth } => cli::graph::run_affected(node, depth).await,
            GraphAction::Status => cli::graph::run_status().await,
            GraphAction::Extract => cli::graph::run_extract().await,
        },
        None => gateway::serve().await,
    }
}

# Toche Execution Plan: Bootstrap through 1.0.0

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement each phase task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Toche 1.0.0 — a local context-efficiency gateway that observes, reduces, remembers, reuses, and coordinates Claude Code's upstream API usage.

**Architecture:** Rust single executable. HTTP gateway on `127.0.0.1:8743` that sits between Claude Code and upstream providers. SQLite for metadata, content-addressed blobs for cached payloads. Reuses patterns from ccusage (pricing), RTK (reduction pipeline, hooks), caveman (profiles, safe config writes), andrej-karpathy-skills (policy), and Graphify (external MCP adapter).

**Tech Stack:** Rust (stable), tokio (async HTTP), axum or hyper (HTTP server), rusqlite (SQLite with WAL), serde/serde_json (serialization), clap (CLI), sha2 (content addressing), TOML (config).

**Global Constraints:**
- Windows-first, single executable, loopback-only by default
- Never locally replay responses containing `tool_use`, destructive actions, or uncertain state (pre-1.0)
- Every third-party component recorded in `THIRD_PARTY_NOTICES.md`
- Reuse from `vendor_reuse/` is reference-only — adapted code lives in `src/` as original Toche work
- Atomic releases: gate must pass before next release begins
- Release never implements later-version behavior behind flags

---

## Phase 0: Pre-Implementation Baseline (current)

**Goal:** Lock in the bootstrapping state before any code is written.

### Task 0.1: Commit bootstrapping baseline

**Files:**
- Create: `.gitignore`
- Create: `THIRD_PARTY_NOTICES.md`
- Create: `docs/TOCHE_CONSOLIDATED_PLAN.md` (already exists, add to git)
- Create: `docs/REUSE_MAP.md` (already exists, add to git)
- Create: `docs/superpowers/plans/2026-07-15-toche-execution-plan.md` (this file)

- [ ] **Step 1: Verify vendor_reuse/ is gitignored**

```powershell
git status
```

Expected: `vendor_reuse/` does NOT appear in untracked files.

- [ ] **Step 2: Stage Toche-owned files only**

```powershell
git add .gitignore THIRD_PARTY_NOTICES.md docs/
```

- [ ] **Step 3: Commit baseline**

```powershell
git commit -m "chore: bootstrap project baseline

Initialize Toche repository with:
- .gitignore (vendor_reuse/, target/, IDE artifacts)
- THIRD_PARTY_NOTICES.md (5 reuse projects, licenses verified)
- docs/TOCHE_CONSOLIDATED_PLAN.md (10-release roadmap)
- docs/REUSE_MAP.md (vendor component to release mapping)
- docs/superpowers/plans/2026-07-15-toche-execution-plan.md

vendor_reuse/ is gitignored — reference copies of ccusage (MIT),
RTK (Apache-2.0), Graphify (MIT), caveman-claude (MIT),
andrej-karpathy-skills (MIT)."
```

- [ ] **Step 4: Push baseline**

```powershell
git push -u origin master
```

**Acceptance gate:** Remote has the baseline commit. `vendor_reuse/` is not in the remote tree.

---

## Phase 1: 0.1.0 — Transparent Gateway

**Entrance criteria:** Phase 0 committed and pushed.

**Outcome:** Claude Code operates normally through Toche with no caching or request rewriting.

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.1.0

### Task 1.1: Project scaffold and Cargo workspace

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `src/main.rs` (entry point — Clap CLI dispatch)
- Create: `src/gateway/mod.rs`
- Create: `src/profiles/mod.rs`
- Create: `src/adapters/mod.rs`
- Create: `src/cli/mod.rs`
- Create: `src/config/mod.rs`

- [ ] **Step 1: Initialize Cargo project**

```powershell
cargo init --name toche C:/Users/nbzkr/Coding/toche
```

Verify: `Cargo.toml` created with `[package]` section.

- [ ] **Step 2: Add core dependencies to Cargo.toml**

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.8"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
reqwest = { version = "0.12", features = ["stream", "rustls-tls"], default-features = false }
```

- [ ] **Step 3: Scaffold CLI entry point in `src/main.rs`**

```rust
use clap::{Parser, Subcommand};

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
    tracing_subscriber::fmt::init();
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
```

- [ ] **Step 4: Verify build**

```powershell
cargo check
```

Expected: compiles successfully (unimplemented modules will error — that's fine for scaffold).

- [ ] **Step 5: Commit scaffold**

```powershell
git add Cargo.toml src/
git commit -m "feat(0.1.0): scaffold Cargo project with CLI and module layout"
```

### Task 1.2: Gateway HTTP server — Anthropic Messages pass-through

**Files:**
- Create: `src/gateway/mod.rs`
- Create: `src/gateway/server.rs`
- Create: `src/gateway/routes.rs`

**Interfaces:**
- Produces: `gateway::serve() -> anyhow::Result<()>` — starts HTTP server on `127.0.0.1:8743`
- Produces: `gateway::routes::messages() -> axum::Router` — Anthropic-compatible `/v1/messages` endpoint with streaming

- [ ] **Step 1: Write HTTP server startup in `src/gateway/server.rs`**

```rust
use anyhow::Context;
use axum::Router;
use std::net::SocketAddr;
use tracing::info;

pub async fn serve(addr: SocketAddr) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/v1/messages", axum::routing::post(super::routes::messages));

    info!("Toche gateway listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind gateway address")?;

    axum::serve(listener, app)
        .await
        .context("Gateway server error")
}
```

- [ ] **Step 2: Write pass-through route in `src/gateway/routes.rs`**

```rust
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use reqwest::Client;
use std::convert::Infallible;

pub async fn messages(
    headers: HeaderMap,
    body: String,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, axum::http::StatusCode> {
    // TODO: Pass through to configured upstream
    // Phase 1 only forwards — no caching, no rewriting
    let upstream_url = "http://localhost:8080/v1/messages"; // Placeholder from config
    let client = Client::new();

    let response = client
        .post(upstream_url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|_| axum::http::StatusCode::BAD_GATEWAY)?;

    let stream = response
        .bytes_stream()
        .map(|result| {
            result
                .map(|bytes| Event::default().data(String::from_utf8_lossy(&bytes).to_string()))
                .map_err(|_| unreachable!())
        });

    Ok(Sse::new(stream))
}
```

- [ ] **Step 3: Wire server entry in `src/gateway/mod.rs`**

```rust
mod routes;
mod server;

pub use server::serve;
const DEFAULT_PORT: u16 = 8743;
const DEFAULT_HOST: &str = "127.0.0.1";
```

- [ ] **Step 4: Update `src/main.rs` to start gateway on no subcommand**

```rust
// Replace the None arm:
None => gateway::serve().await,
```

- [ ] **Step 5: Verify build and test with curl**

```powershell
cargo check
# Start gateway in one terminal: cargo run
# In another: curl -X POST http://127.0.0.1:8743/v1/messages -H "Content-Type: application/json" -d '{}'
```

Expected: 502 Bad Gateway (no upstream configured yet, but server responds).

- [ ] **Step 6: Commit gateway**

```powershell
git add src/gateway/
git commit -m "feat(0.1.0): minimal HTTP gateway with Anthropic Messages pass-through"
```

### Task 1.3: Profile configuration — import existing Claude Code gateway config

**Files:**
- Create: `src/profiles/mod.rs`
- Create: `src/profiles/types.rs`
- Create: `src/profiles/loader.rs`
- Create: `src/config/mod.rs`
- Create: `src/config/toche_config.rs`

**Interfaces:**
- Produces: `Profiles::load() -> anyhow::Result<Profiles>` — hierarchical config loading
- Produces: `Profile` struct — upstream endpoint, auth method, credential, headers, model mappings

- [ ] **Step 1: Define profile types in `src/profiles/types.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub upstream_url: String,
    pub auth_method: AuthMethod,
    pub headers: HashMap<String, String>,
    pub models: HashMap<String, String>, // caller ID -> raw model ID
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthMethod {
    ApiKey { header_name: String, key: String },
    BearerToken { token: String },
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profiles {
    pub default: Option<String>,
    pub profiles: Vec<Profile>,
}
```

- [ ] **Step 2: Implement config loader in `src/config/toche_config.rs`**

```rust
use anyhow::Context;
use std::path::PathBuf;

/// Resolves Toche config directory: TOCHE_CONFIG_DIR env -> ~/.toche
pub fn config_dir() -> PathBuf {
    std::env::var("TOCHE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".toche"))
}

pub fn load_profiles() -> anyhow::Result<super::super::profiles::Profiles> {
    let path = config_dir().join("profiles.toml");
    if !path.exists() {
        anyhow::bail!("No profiles configured. Run `toche setup` first.");
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&content).context("Failed to parse profiles.toml")
}
```

- [ ] **Step 3: Adapt RTK's `atomic_write()` pattern for config writes**

```rust
use std::fs;
use std::path::Path;

/// Atomically write content to path: temp file + rename
pub fn atomic_write(path: &Path, content: &str) -> anyhow::Result<()> {
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
```

- [ ] **Step 4: Verify config loading**

```powershell
mkdir -Force C:/Users/nbzkr/.toche
echo '[default]' > C:/Users/nbzkr/.toche/profiles.toml
cargo check
```

- [ ] **Step 5: Commit profile and config modules**

```powershell
git add src/profiles/ src/config/
git commit -m "feat(0.1.0): profile types, config loader, atomic write utility"
```

### Task 1.4: `toche setup` wizard

**Files:**
- Create: `src/cli/mod.rs`
- Create: `src/cli/setup.rs`
- Create: `src/cli/doctor.rs`
- Create: `src/cli/status.rs`

**Interfaces:**
- Consumes: `Profiles`, `atomic_write()`
- Produces: `cli::setup::run() -> anyhow::Result<()>`

- [ ] **Step 1: Implement setup in `src/cli/setup.rs`**

Adapt caveman's `safeWriteFlag()` pattern — no direct `fs::write` to predictable paths.

```rust
use anyhow::Context;
use tracing::info;

pub async fn run() -> anyhow::Result<()> {
    info!("Starting Toche setup...");

    // 1. Create config directory
    let dir = crate::config::toche_config::config_dir();
    std::fs::create_dir_all(&dir).context("Failed to create config directory")?;

    // 2. Detect existing Claude Code gateway config
    let claude_settings = detect_claude_config()?;
    if claude_settings.is_none() {
        info!("No existing Claude Code gateway found — configure manually in {}/profiles.toml", dir.display());
        return Ok(());
    }

    // 3. Parse and import
    let settings = claude_settings.unwrap();
    let profile = import_from_claude_settings(&settings)?;

    // 4. Save with atomic write
    let toml_str = toml::to_string_pretty(&profile)?;
    let path = dir.join("profiles.toml");
    crate::config::toche_config::atomic_write(&path, &toml_str)?;
    info!("Profile '{}' saved to {}", profile.profiles[0].name, path.display());

    Ok(())
}

fn detect_claude_config() -> anyhow::Result<Option<serde_json::Value>> {
    // Read ~/.claude/settings.json, extract gateway configuration
    let path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");
    if !path.exists() {
        return Ok(None);
    }
    // JSONC-tolerant read: strip comments, then parse
    let raw = std::fs::read_to_string(&path)?;
    let cleaned: String = raw.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(serde_json::from_str(&cleaned)?))
}

fn import_from_claude_settings(settings: &serde_json::Value) -> anyhow::Result<crate::profiles::Profiles> {
    // Extract base URL, auth, headers, model mappings from Claude Code config
    // Map to Toche Profile format
    todo!("Extract and map configuration fields")
}
```

- [ ] **Step 2: Implement doctor in `src/cli/doctor.rs`**

```rust
pub async fn run() -> anyhow::Result<()> {
    println!("Toche Doctor");
    println!("============");
    println!();

    // Check config directory
    let dir = crate::config::toche_config::config_dir();
    println!("Config directory: {}", dir.display());
    println!("  exists: {}", dir.exists());

    // Check profiles
    let profiles_path = dir.join("profiles.toml");
    println!("Profiles file: {}", profiles_path.display());
    println!("  exists: {}", profiles_path.exists());

    // Check Claude Code integration
    let claude_dir = dirs::home_dir().unwrap().join(".claude");
    println!("Claude Code directory: {}", claude_dir.display());
    println!("  exists: {}", claude_dir.exists());

    // Check if Claude Code points to Toche
    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path)?;
        println!("Claude settings.json: {}", if raw.contains("127.0.0.1:8743") { "points to Toche" } else { "not pointing to Toche" });
    }

    Ok(())
}
```

- [ ] **Step 3: Implement status in `src/cli/status.rs`**

```rust
pub async fn run() -> anyhow::Result<()> {
    let profiles = crate::config::toche_config::load_profiles()?;
    println!("Toche Status");
    println!("============");
    println!("Default profile: {}", profiles.default.as_deref().unwrap_or("none"));
    for p in &profiles.profiles {
        println!("  {} -> {}", p.name, p.upstream_url);
    }
    Ok(())
}
```

- [ ] **Step 4: Wire CLI modules in `src/cli/mod.rs`**

```rust
pub mod setup;
pub mod doctor;
pub mod status;
pub mod connect;
pub mod disconnect;
```

- [ ] **Step 5: Commit**

```powershell
git add src/cli/
git commit -m "feat(0.1.0): setup wizard, doctor, and status commands"
```

### Task 1.5: `toche connect/disconnect claude`

**Files:**
- Create: `src/cli/connect.rs`
- Create: `src/cli/disconnect.rs`

**Interfaces:**
- Consumes: `Profiles::load()`, config_dir
- Produces: `cli::connect::run()` / `cli::disconnect::run()` — modifies `~/.claude/settings.json`

- [ ] **Step 1: Implement connect — adapt caveman's settings.json merge pattern**

```rust
use anyhow::Context;

pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");

    match agent {
        "claude" => connect_claude().await,
        _ => anyhow::bail!("Unknown agent: {}. Supported: claude", agent),
    }
}

async fn connect_claude() -> anyhow::Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");

    // 1. Backup existing settings
    let backup_path = settings_path.with_extension("json.toche-backup");
    if settings_path.exists() {
        std::fs::copy(&settings_path, &backup_path)
            .context("Failed to backup settings.json")?;
    }

    // 2. Read settings (JSONC-tolerant)
    let raw = std::fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".into());
    let cleaned: String = raw.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut settings: serde_json::Value = serde_json::from_str(&cleaned)
        .context("Failed to parse settings.json")?;

    // 3. Set Toche as base URL
    settings["apiKeyHelper"] = serde_json::Value::Null; // Clear if set
    if let Some(obj) = settings.as_object_mut() {
        if !obj.contains_key("baseURL") {
            obj.insert(
                "baseURL".into(),
                serde_json::Value::String("http://127.0.0.1:8743".into()),
            );
        }
    }

    // 4. Atomic write back
    let content = serde_json::to_string_pretty(&settings)?;
    crate::config::toche_config::atomic_write(&settings_path, &content)?;

    println!("Claude Code now routing through Toche.");
    println!("Backup saved to: {}", backup_path.display());
    Ok(())
}
```

- [ ] **Step 2: Implement disconnect**

```rust
pub async fn run(agent: Option<&str>) -> anyhow::Result<()> {
    let agent = agent.unwrap_or("claude");
    match agent {
        "claude" => disconnect_claude().await,
        _ => anyhow::bail!("Unknown agent: {}", agent),
    }
}

async fn disconnect_claude() -> anyhow::Result<()> {
    let settings_path = dirs::home_dir()
        .unwrap()
        .join(".claude")
        .join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Restore backup if it exists
    if backup_path.exists() {
        std::fs::copy(&backup_path, &settings_path)
            .context("Failed to restore settings.json backup")?;
        std::fs::remove_file(&backup_path)?;
        println!("Restored previous Claude Code configuration.");
    } else {
        println!("No Toche backup found — settings.json was not modified.");
    }

    Ok(())
}
```

- [ ] **Step 5: Commit**

```powershell
git add src/cli/connect.rs src/cli/disconnect.rs
git commit -m "feat(0.1.0): connect/disconnect claude — settings.json backup and restore"
```

### Task 1.6: 0.1.0 Acceptance gate

- [ ] **Step 1: Full Claude Code session through Toche**

```powershell
# Terminal 1: cargo run (starts gateway)
# Terminal 2: Run a representative Claude Code session with base URL = http://127.0.0.1:8743
```

Expected: Normal operation, streaming works, tool calls work.

- [ ] **Step 2: Disconnect restores exact previous config**

```powershell
toche disconnect claude
diff ~/.claude/settings.json ~/.claude/settings.json.toche-backup  # Should be identical
```

- [ ] **Step 3: Verify no cache, compression, or semantic modification**

Review all code in `src/` — confirm no request body is modified before forwarding.

- [ ] **Step 4: Commit 0.1.0 gate**

```powershell
git commit --allow-empty -m "gate(0.1.0): transparent gateway acceptance passed

Verified:
- Claude Code operates through Toche with equivalent protocol behavior
- Disconnect restores exact previous Claude Code configuration
- No cache, compression, or semantic modification occurs"
```

- [ ] **Step 5: Tag 0.1.0**

```powershell
git tag -a v0.1.0 -m "0.1.0: Transparent Gateway — Claude Code operates through Toche with no caching"
git push origin master --tags
```

---

## Phase 2: 0.2.0 — Usage Ledger

**Entrance criteria:** 0.1.0 gate passed and tagged.

**Outcome:** The user can explain where every request and token went.

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.2.0

### Task 2.1: SQLite ledger schema (adapt RTK tracking.rs)

**Files:**
- Create: `src/meter/mod.rs`
- Create: `src/meter/db.rs`
- Create: `src/meter/schema.rs`

**Interfaces:**
- Produces: `meter::db::Ledger` struct — open/create database, insert records, query by profile/model/session/project
- Consumes: RTK's WAL mode, busy_timeout, retention pattern

Extend RTK's schema to Toche's needs — tracking upstream requests, logical/billed tokens, cache reads/writes, latency, errors, and cost.

### Task 2.2: Cost calculation (adapt ccusage pricing.rs + cost.rs)

**Files:**
- Create: `src/meter/pricing.rs`
- Create: `src/meter/cost.rs`

**Interfaces:**
- Produces: `pricing::PricingMap` — 4-stage resolution (exact → alias → embedded snapshot → hardcoded)
- Produces: `cost::calculate(input_tokens, output_tokens, cache_creation, cache_read, model) -> CostResult`

### Task 2.3: Request interception and ledger recording

**Files:**
- Modify: `src/gateway/routes.rs` — inject ledger recording around upstream call
- Create: `src/meter/recorder.rs`

### Task 2.4: `toche stats` CLI (adapt ccusage output.rs)

**Files:**
- Create: `src/cli/stats.rs`
- Create: `src/meter/report.rs`

### Task 2.5: 0.2.0 Acceptance gate

- Recorded totals reconcile with upstream usage fields
- Unknown custom model pricing never invents cost
- Ledger adds no prompt or completion tokens
- Tag `v0.2.0`

---

## Phase 3: 0.3.0 — Provider Cache Coordinator

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.3.0

### Task 3.1: Provider capability detection

### Task 3.2: Anthropic cache-control insertion

### Task 3.3: Stable request serialization and breakpoint policy

### Task 3.4: Observe-only recommendation mode

### Task 3.5: 0.3.0 Acceptance gate — tag `v0.3.0`

---

## Phase 4: 0.4.0 — Request Shield

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.4.0

### Task 4.1: Canonical request fingerprinting

### Task 4.2: Single-flight coalescing

### Task 4.3: Safe idempotent replay (never tool_use)

### Task 4.4: 0.4.0 Acceptance gate — tag `v0.4.0`

---

## Phase 5: 0.5.0 — Safe Context Reduction

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.5.0

### Task 5.1: Adapt RTK's 8-stage filter pipeline into Rust

### Task 5.2: Lossiness tracking with tee recovery

### Task 5.3: Source code comment filtering (adapt RTK filter.rs)

### Task 5.4: `toche_expand` MCP tool

### Task 5.5: Per-command and global bypass

### Task 5.6: 0.5.0 Acceptance gate — tag `v0.5.0`

---

## Phase 6: 0.6.0 — Efficiency Profiles

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.6.0

### Task 6.1: Profile engine — normal, concise, custom

### Task 6.2: Adapt caveman's 6 intensity levels into structured rules

### Task 6.3: Adapt andrej-karpathy's 4 principles into careful-work policy

### Task 6.4: Profile preview and per-session override

### Task 6.5: Stable prompt placement for provider caching

### Task 6.6: 0.6.0 Acceptance gate — tag `v0.6.0`

---

## Phase 7: 0.7.0 — Persistent Safe Cache

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.7.0

### Task 7.1: Content-addressed SQLite metadata + compressed blob storage

### Task 7.2: Workspace, model, tool schema fingerprinting

### Task 7.3: Exact safe-response reuse across sessions

### Task 7.4: Cache TTL, eviction, inspection, invalidation

### Task 7.5: Default denial safety rules (no tool calls, no cross-model)

### Task 7.6: 0.7.0 Acceptance gate — tag `v0.7.0`

---

## Phase 8: 0.8.0 — Session Continuity

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.8.0

### Task 8.1: Structured checkpoint format

### Task 8.2: `toche checkpoint` / `toche resume` commands

### Task 8.3: Git/file-state validation on resume (adapt RTK integrity.rs)

### Task 8.4: Stale checkpoint detection

### Task 8.5: 0.8.0 Acceptance gate — tag `v0.8.0`

---

## Phase 9: 0.9.0 — Project Knowledge Adapter

**Reuse reference:** `docs/REUSE_MAP.md` Section 0.9.0

### Task 9.1: Graphify installation detection

### Task 9.2: MCP client adapter (10 tools — adapt Graphify's MCP contract)

### Task 9.3: Graceful operation when Graphify absent

### Task 9.4: 0.9.0 Acceptance gate — tag `v0.9.0`

---

## Phase 10: 1.0.0 — Stable Local Toche

**Reuse reference:** `docs/REUSE_MAP.md` Section 1.0.0

### Task 10.1: Signed Windows installer

### Task 10.2: Foreground/background operation

### Task 10.3: Crash recovery and corruption handling

### Task 10.4: Secure credential storage (DPAPI)

### Task 10.5: Cache privacy controls

### Task 10.6: End-to-end benchmark suite

### Task 10.7: Complete documentation

### Task 10.8: 1.0.0 Acceptance gate — tag `v1.0.0`

---

## Phase 11: 2nd-Round Comprehensive Audit

**Entrance criteria:** 1.0.0 gate passed and tagged.

**Goal:** A rigorous second-pass audit that goes beyond normal testing — edge cases, broader usage scenarios, stress conditions, and cross-cutting concerns not covered by per-release acceptance gates.

**Why this exists:** The per-release gates verify that each feature works in isolation. This audit verifies that all 10 releases work together under real-world pressure. It catches interactions between features, resource exhaustion scenarios, and behavior under malformed or adversarial input.

### Task 11.1: Cross-release interaction audit

**Scope:**
- [ ] Profile switching during an active cached session (0.1.0 + 0.7.0)
- [ ] Reduction producing different output after ledger migration (0.5.0 + 0.2.0)
- [ ] Cache hit with profile-driven concision active (0.7.0 + 0.6.0)
- [ ] Session resume after cache eviction of related entries (0.8.0 + 0.7.0)
- [ ] Request coalescing while provider cache coordinator modifies headers (0.4.0 + 0.3.0)
- [ ] Graphify query results interacting with reduction pipeline (0.9.0 + 0.5.0)
- [ ] Checkpoint restore after settings.json disconnect/reconnect cycle (0.8.0 + 0.1.0)
- [ ] Every combination of two releases that touch the same data path

**Method:** Matrix test — for each pair of releases (N×M where data paths overlap), run a representative scenario. Document interactions.

### Task 11.2: Edge case catalog

**Input edge cases:**
- [ ] Empty request body
- [ ] Maximum-size request body (Claude's 200K context limit)
- [ ] Unicode in every field (model name, message content, system prompt, tool names, metadata)
- [ ] Streaming chunk of exactly 1 byte
- [ ] Streaming chunk of exactly 65536 bytes (typical TCP buffer boundary)
- [ ] Duplicate message IDs
- [ ] Missing required Anthropic fields (`model`, `messages`, `max_tokens`)
- [ ] Unknown top-level JSON keys
- [ ] Nested nulls in optional fields
- [ ] Concurrent requests from multiple Claude Code processes
- [ ] Rapid connect/disconnect cycling
- [ ] Tool definitions with 0 tools, 1 tool, 256 tools
- [ ] System prompt with 0 characters, 1 character, 100K+ characters

**State edge cases:**
- [ ] Cold start (no config, no database, no cache)
- [ ] Database at capacity (disk full simulation)
- [ ] Corrupted SQLite file (recovery test)
- [ ] Corrupted cache blob (checksum mismatch)
- [ ] Stale checkpoint (workspace changed since checkpoint)
- [ ] Config file deleted while Toche is running
- [ ] Clock skew (system time jumps backward)

**Protocol edge cases:**
- [ ] Anthropic error responses (400, 401, 429, 500, 529)
- [ ] Upstream connection refused
- [ ] Upstream timeout mid-stream
- [ ] Upstream returns non-JSON-SSE line
- [ ] Upstream returns `event: ping` with no data
- [ ] Stream ends without `message_stop`
- [ ] Multiple `message_start` events in one stream

### Task 11.3: Broader usage scenarios

**Multi-project usage:**
- [ ] Two Claude Code projects using Toche simultaneously through different profiles
- [ ] Profile switching without restarting Toche
- [ ] Toche keeps running while Claude Code sessions start/stop in different directories
- [ ] Project A's cache never serves Project B's requests

**Long-running session:**
- [ ] 8-hour continuous Claude Code session through Toche
- [ ] Memory growth measurement (no unbounded accumulation)
- [ ] SQLite WAL growth under sustained writes
- [ ] Cache blob directory growth under sustained usage

**Recovery scenarios:**
- [ ] Kill Toche mid-request → restart → verify clean state
- [ ] Kill Toche mid-database-write → restart → verify WAL recovery
- [ ] Kill Toche mid-cache-write → restart → verify no partial blobs
- [ ] Power loss simulation (Windows crash dump verification)

**Configuration variations:**
- [ ] Every upstream protocol: Anthropic native, Anthropic via 9Router, OpenAI-compatible
- [ ] Every auth method: API key header, bearer token, custom header
- [ ] Custom model IDs with slash prefixes (e.g., `cx/gpt-5-sol`)
- [ ] Toche behind a corporate proxy

### Task 11.4: Performance and resource audit

- [ ] Measure per-request latency overhead (target: <5ms added)
- [ ] Memory baseline: idle Toche <50MB
- [ ] Memory under load: 10 concurrent requests
- [ ] SQLite query latency under 100K-request ledger
- [ ] Cache blob lookup under 10K entries
- [ ] Reduction pipeline latency per KB of tool output
- [ ] Startup time (cold cache, warm cache)
- [ ] Disk usage: 1K, 10K, 100K requests worth of ledger + cache

### Task 11.5: Security audit

- [ ] Symlink attack on config directory (adapt caveman's hardening)
- [ ] Path traversal in cache blob keys
- [ ] Path traversal in checkpoint file paths
- [ ] Large request body that compresses to a cache-busting hash collision attempt
- [ ] Settings.json injection via profile name
- [ ] Credential exposure in logs, stats output, error messages
- [ ] Loopback binding verified (not 0.0.0.0)
- [ ] Safe restore: disconnect never leaves credentials readable
- [ ] Thread safety: no data races in cache, ledger, or gateway under concurrent requests

### Task 11.6: Acceptance gate

- [ ] All interaction audit scenarios pass
- [ ] All edge cases documented (pass or documented-limitation)
- [ ] All broader scenarios pass or have documented workaround
- [ ] Performance targets met: <5ms latency overhead, <50MB idle memory
- [ ] All security items pass — no credential leaks, no injection, no traversal
- [ ] `docs/AUDIT_REPORT.md` published with full findings

**Commit:**

```powershell
git commit --allow-empty -m "audit: 2nd-round comprehensive audit complete

All cross-release interactions, edge cases, broader scenarios,
performance targets, and security items verified.

Full report: docs/AUDIT_REPORT.md"
git tag -a v1.0.0-audited -m "1.0.0: Stable Local Toche — comprehensive audit passed"
git push origin master --tags
```

---

## Execution strategy

### Commit discipline per release

Each phase ends with a signed tag. Between phases, all intermediate commits are on `master`. If a phase fails its acceptance gate, fix on `master` before tagging. No release branches — tags are the rollback points.

```text
master:  [0.0] -> [0.1.0 commits...] -> gate -> v0.1.0 -> [0.2.0 commits...] -> gate -> v0.2.0 -> ...
                                                                                              |
                                                                                         v1.0.0 -> [audit] -> v1.0.0-audited
```

### Rollback

If a release gate fails and can't be fixed forward, revert to the previous tag:

```powershell
git checkout v0.1.0  # Back to last known good
git checkout -b fix-0.2.0
# ... fix ...
git checkout master && git merge fix-0.2.0
git tag -a v0.2.0 -m "..."
```

### What gets detailed plans

Phases 0 and 1 are detailed in this document (bite-sized tasks with actual code). Phases 2-11 are outlined — each will get its own detailed plan at `docs/superpowers/plans/YYYY-MM-DD-<release>.md` before execution begins. This prevents premature detail on releases whose design may shift based on earlier-phase discoveries.

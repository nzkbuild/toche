use anyhow::Context;

use crate::config::loader::load_config;
use crate::profiles::loader::config_dir;
use crate::safe_cache;

fn resolve_cache_paths() -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let config = load_config().context("Failed to load configuration")?;
    Ok(config.storage.resolve_paths(&config_dir()))
}

pub async fn run_inspect(json: bool, entries: u32) -> anyhow::Result<()> {
    let (db_path, _) = resolve_cache_paths()?;
    let db = safe_cache::cache_db::CacheDb::open(&db_path)
        .with_context(|| format!("Failed to open cache DB at {}", db_path.display()))?;

    let list = db
        .list(None, entries)
        .context("Failed to list cache entries")?;

    if json {
        let output: Vec<serde_json::Value> = list
            .iter()
            .map(|e| {
                serde_json::json!({
                    "project_path": e.project_path,
                    "fingerprint": e.fingerprint,
                    "workspace_fingerprint": e.workspace_fingerprint,
                    "response_hash": e.response_hash,
                    "model": e.model,
                    "status": e.status,
                    "tokens_input": e.tokens_input,
                    "tokens_output": e.tokens_output,
                    "created_at": e.created_at,
                    "last_hit_at": e.last_hit_at,
                    "hit_count": e.hit_count,
                })
            })
            .collect();
        let json_str =
            serde_json::to_string_pretty(&output).context("Failed to serialize cache entries")?;
        println!("{json_str}");
    } else if list.is_empty() {
        println!("No cache entries found.");
    } else {
        println!("Safe Cache Entries ({}):", list.len());
        println!(
            "{:32}  {:16}  {:25}  {:>6}  {:>6}  {:>4}",
            "Project", "Fingerprint", "Model", "Tokens", "Hits", "Age"
        );
        for e in &list {
            let short_fp = &e.fingerprint[..16.min(e.fingerprint.len())];
            let short_project = if e.project_path.len() > 30 {
                format!("...{}", &e.project_path[e.project_path.len() - 29..])
            } else {
                e.project_path.clone()
            };
            let age = estimate_age(&e.created_at);
            println!(
                "{:32}  {:16}  {:25}  {:>4}in  {:>4}  {:>4}",
                short_project, short_fp, e.model, e.tokens_input, e.hit_count, age,
            );
        }
    }

    Ok(())
}

pub async fn run_clear(project_only: bool, all: bool) -> anyhow::Result<()> {
    let (db_path, _) = resolve_cache_paths()?;
    let db = safe_cache::cache_db::CacheDb::open(&db_path)
        .with_context(|| format!("Failed to open cache DB at {}", db_path.display()))?;

    let project = if project_only {
        Some(
            std::env::current_dir()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        )
    } else if all {
        None
    } else {
        // Default: just clear current project
        Some(
            std::env::current_dir()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        )
    };

    let scope_desc = match &project {
        Some(p) => format!("project '{}'", p),
        None => "all projects".to_string(),
    };
    let removed = db
        .clear(project.as_deref())
        .context("Failed to clear cache entries")?;
    println!("Removed {} cache entries from {}.", removed, scope_desc);

    Ok(())
}

pub async fn run_why(fingerprint: &str) -> anyhow::Result<()> {
    let (db_path, cas_dir) = resolve_cache_paths()?;
    let db = safe_cache::cache_db::CacheDb::open(&db_path)
        .with_context(|| format!("Failed to open cache DB at {}", db_path.display()))?;

    let project = std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    match db.lookup(&project, fingerprint) {
        Ok(Some(entry)) => {
            println!("Cache entry found:");
            println!("  Project:        {}", entry.project_path);
            println!("  Fingerprint:    {}", entry.fingerprint);
            println!("  Model:          {}", entry.model);
            println!("  Status:         {}", entry.status);
            println!(
                "  Tokens:         {} in / {} out",
                entry.tokens_input, entry.tokens_output
            );
            println!("  Response hash:  {}", entry.response_hash);
            println!("  Created:        {}", entry.created_at);
            println!("  Last hit:       {}", entry.last_hit_at);
            println!("  Hit count:      {}", entry.hit_count);

            let current_ws = safe_cache::workspace::compute_workspace_fingerprint();
            if current_ws != entry.workspace_fingerprint {
                println!();
                println!(
                    "  Workspace fingerprint MISMATCH — workspace changed since entry was cached."
                );
                println!("    Cached:   {}", entry.workspace_fingerprint);
                println!("    Current:  {}", current_ws);
            } else {
                println!("  Workspace fingerprint match: yes");
            }

            // Check CAS blob exists
            if crate::reduce::storage::retrieve_at(&entry.response_hash, &cas_dir).is_ok() {
                println!("  CAS blob:       present");
            } else {
                println!("  CAS blob:       MISSING (orphaned entry)");
            }
        }
        Ok(None) => {
            println!(
                "No cache entry found for fingerprint '{}' in project '{}'.",
                fingerprint, project
            );
        }
        Err(e) => {
            anyhow::bail!("Failed to query cache: {e}");
        }
    }

    Ok(())
}

fn estimate_age(rfc3339: &str) -> String {
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(rfc3339) else {
        return "?".into();
    };
    let now = chrono::Utc::now();
    let dur = now.signed_duration_since(ts.with_timezone(&chrono::Utc));
    if dur.num_days() > 0 {
        format!("{}d", dur.num_days())
    } else if dur.num_hours() > 0 {
        format!("{}h", dur.num_hours())
    } else {
        format!("{}m", dur.num_minutes())
    }
}

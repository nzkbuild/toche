use anyhow::Context;

use crate::config::loader::load_config;
use crate::config::toche_config::StorageConfig;
use crate::profiles::loader::config_dir;
use crate::safe_cache::cache_db::{self, CacheDb, OrphanCandidates};

/// Resolve storage paths from config. Relative paths are resolved under
/// `config_dir()`; absolute paths are preserved.
fn resolve_storage_paths(cfg: &StorageConfig) -> (std::path::PathBuf, std::path::PathBuf) {
    cfg.resolve_paths(&config_dir())
}

fn validate_storage_config(cfg: &StorageConfig) -> anyhow::Result<()> {
    let errors = cfg.validate();
    if errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "Storage configuration is invalid:\n  - {}",
            errors.join("\n  - ")
        )
    }
}

/// Show cache entries, CAS bytes, orphan candidates, configured limits,
/// ledger row count, and proposed retention action.
pub async fn run_storage_status(json: bool) -> anyhow::Result<()> {
    let config = load_config().context("Failed to load configuration")?;
    let storage_cfg = &config.storage;
    validate_storage_config(storage_cfg)?;
    let (db_path, cas_dir) = resolve_storage_paths(storage_cfg);

    let db = CacheDb::open(&db_path)
        .with_context(|| format!("Failed to open ledger DB at {}", db_path.display()))?;

    let stats = db
        .storage_stats(&cas_dir)
        .context("Failed to collect storage stats")?;

    let classified = db.orphan_candidates(&cas_dir).unwrap_or(OrphanCandidates {
        safe_to_delete: Vec::new(),
        legacy_untracked: Vec::new(),
    });

    let wal_action = "PRAGMA wal_checkpoint(TRUNCATE) — would run on confirmed cleanup --orphans";

    if json {
        let output = serde_json::json!({
            "cache_entries": stats.cache_entries,
            "registered_blobs": stats.registered_blobs,
            "cas_bytes_on_disk": stats.cas_bytes_on_disk,
            "free_disk_bytes": stats.free_disk_bytes,
            "ledger_rows": stats.ledger_rows,
            "orphan_safe_to_delete": classified.safe_to_delete.len(),
            "orphan_safe_to_delete_bytes": classified.safe_to_delete.iter().map(|o| o.bytes).sum::<u64>(),
            "legacy_untracked": classified.legacy_untracked.len(),
            "legacy_untracked_bytes": classified.legacy_untracked.iter().map(|o| o.bytes).sum::<u64>(),
            "configured_limits": {
                "max_cas_bytes": storage_cfg.max_cas_bytes,
                "max_entries": storage_cfg.max_entries,
                "min_free_disk_bytes": storage_cfg.min_free_disk_bytes,
                "ledger_retention_days": storage_cfg.ledger_retention_days,
            },
            "ledger_db_path": db_path.to_string_lossy(),
            "cas_dir_path": cas_dir.to_string_lossy(),
            "wal_action": wal_action,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).context("Failed to serialize storage stats")?
        );
    } else {
        println!("Toche Storage Status");
        println!("====================");
        println!();
        println!("  Cache entries:        {}", stats.cache_entries);
        println!("  Known CAS blobs:      {}", stats.registered_blobs);
        println!(
            "  CAS bytes on disk:    {}",
            format_bytes(stats.cas_bytes_on_disk)
        );
        if let Some(free) = stats.free_disk_bytes {
            println!("  Free disk bytes:      {}", format_bytes(free));
        } else {
            println!("  Free disk bytes:      (unavailable)");
        }
        println!("  Ledger rows:          {}", stats.ledger_rows);
        println!();
        println!(
            "  Orphans (safe to delete):  {} ({} bytes)",
            classified.safe_to_delete.len(),
            format_bytes(classified.safe_to_delete.iter().map(|o| o.bytes).sum())
        );
        println!(
            "  Legacy-untracked (SKIPPED): {} ({} bytes)",
            classified.legacy_untracked.len(),
            format_bytes(classified.legacy_untracked.iter().map(|o| o.bytes).sum())
        );
        if !classified.legacy_untracked.is_empty() {
            println!(
                "    ↳ Pre-M1B reduce blobs never registered — cleanup --orphans will NOT delete these."
            );
        }
        println!();
        println!("  Configured limits:");
        fmt_optional(
            "    max_cas_bytes",
            storage_cfg.max_cas_bytes.map(format_bytes),
        );
        fmt_optional(
            "    max_entries",
            storage_cfg.max_entries.map(|v| v.to_string()),
        );
        fmt_optional(
            "    min_free_disk_bytes",
            storage_cfg.min_free_disk_bytes.map(format_bytes),
        );
        fmt_optional(
            "    ledger_retention_days",
            storage_cfg
                .ledger_retention_days
                .map(|d| format!("{d} days")),
        );
        println!();
        println!("  Note: cache expiry/clear removes cache references only.");
        println!("  CAS disk space is only reclaimed by `toche cleanup --orphans`.");
    }

    Ok(())
}

/// Dry-run: report orphan candidates, legacy-untracked count, proposed
/// ledger deletion count, and WAL checkpoint action.  Does not modify anything.
pub async fn run_cleanup_dry_run(json: bool) -> anyhow::Result<()> {
    let config = load_config().context("Failed to load configuration")?;
    let storage_cfg = &config.storage;
    validate_storage_config(storage_cfg)?;
    let (db_path, cas_dir) = resolve_storage_paths(storage_cfg);

    let db = CacheDb::open(&db_path)
        .with_context(|| format!("Failed to open ledger DB at {}", db_path.display()))?;

    let classified = db
        .orphan_candidates(&cas_dir)
        .context("Failed to scan for orphan candidates")?;
    let safe_bytes: u64 = classified.safe_to_delete.iter().map(|o| o.bytes).sum();
    let legacy_bytes: u64 = classified.legacy_untracked.iter().map(|o| o.bytes).sum();

    // Proposed ledger deletion count for dry-run
    let (ledger_dry_deleted, ledger_retention_desc) = match storage_cfg.ledger_retention_days {
        Some(days) => {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
            let count: i64 = db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM ledger WHERE timestamp < ?1",
                    rusqlite::params![cutoff.to_rfc3339()],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            (
                count as u64,
                format!("delete ledger rows older than {days} days"),
            )
        }
        None => (0, "ledger retention is disabled".to_string()),
    };

    let wal_desc = "PRAGMA wal_checkpoint(TRUNCATE) — would run on `toche cleanup --orphans`";

    if json {
        let summary = serde_json::json!({
            "dry_run": true,
            "orphan_safe_to_delete": classified.safe_to_delete.len(),
            "orphan_safe_to_delete_bytes": safe_bytes,
            "orphan_safe_to_delete_list": classified.safe_to_delete.iter().map(|o| serde_json::json!({
                "hash": o.hash,
                "bytes": o.bytes,
            })).collect::<Vec<_>>(),
            "legacy_untracked": classified.legacy_untracked.len(),
            "legacy_untracked_bytes": legacy_bytes,
            "ledger_retention_days": storage_cfg.ledger_retention_days,
            "ledger_rows_that_would_delete": ledger_dry_deleted,
            "ledger_retention_desc": ledger_retention_desc,
            "ledger_db_path": db_path.to_string_lossy(),
            "cas_dir_path": cas_dir.to_string_lossy(),
            "wal_checkpoint_action": wal_desc,
            "would_perform_wal_checkpoint": true,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).context("Failed to serialize dry-run result")?
        );
    } else {
        println!("Toche Cleanup --dry-run");
        println!("=======================");
        println!();
        if classified.safe_to_delete.is_empty() && classified.legacy_untracked.is_empty() {
            println!("  No orphan candidates found.");
        } else {
            if !classified.safe_to_delete.is_empty() {
                println!(
                    "  Orphans safe to delete:  {} ({} bytes)",
                    classified.safe_to_delete.len(),
                    format_bytes(safe_bytes)
                );
                println!();
                for c in &classified.safe_to_delete {
                    println!("    {}  {}", c.hash, format_bytes(c.bytes));
                }
                println!();
                println!(
                    "  Run `toche cleanup --orphans` to delete these {} files.",
                    classified.safe_to_delete.len()
                );
            }
            if !classified.legacy_untracked.is_empty() {
                println!();
                println!(
                    "  Legacy-untracked (SKIPPED): {} ({} bytes)",
                    classified.legacy_untracked.len(),
                    format_bytes(legacy_bytes)
                );
                println!(
                    "    ↳ Pre-M1B reduce blobs never registered — cleanup --orphans will NOT delete these."
                );
            }
        }
        println!();
        println!(
            "  Ledger retention:   {} — {}",
            storage_cfg
                .ledger_retention_days
                .map(|d| format!("{d} days"))
                .unwrap_or_else(|| "disabled".to_string()),
            ledger_retention_desc
        );
        if ledger_dry_deleted > 0 {
            println!(
                "  Ledger rows that would be deleted: {}",
                ledger_dry_deleted
            );
        }
        println!();
        println!("  Would perform WAL checkpoint: yes ({wal_desc})");
    }

    Ok(())
}

/// Confirm: delete orphan CAS files, delete expired ledger rows if
/// retention configured, and run WAL checkpoint(TRUNCATE).
pub async fn run_cleanup_orphans(json: bool) -> anyhow::Result<()> {
    let config = load_config().context("Failed to load configuration")?;
    let storage_cfg = &config.storage;
    validate_storage_config(storage_cfg)?;
    let (db_path, cas_dir) = resolve_storage_paths(storage_cfg);

    let db = CacheDb::open(&db_path)
        .with_context(|| format!("Failed to open ledger DB at {}", db_path.display()))?;

    let candidates = db
        .orphan_candidates(&cas_dir)
        .context("Failed to scan for orphan candidates")?;

    let mut deleted = 0u64;
    let mut freed_bytes = 0u64;

    for c in &candidates.safe_to_delete {
        if crate::reduce::storage::delete_at(&c.hash, &cas_dir) {
            deleted += 1;
            freed_bytes += c.bytes;
        }
    }

    // Ledger retention cleanup
    let ledger_deleted = match storage_cfg.ledger_retention_days {
        Some(days) => cache_db::ledger_delete_older_than(&db.conn, days).unwrap_or(0),
        None => 0,
    };

    // WAL checkpoint
    let wal_result =
        cache_db::wal_checkpoint(&db.conn).unwrap_or_else(|e| format!("WAL checkpoint error: {e}"));

    if json {
        let output = serde_json::json!({
            "orphans_deleted": deleted,
            "orphans_freed_bytes": freed_bytes,
            "legacy_untracked_skipped": candidates.legacy_untracked.len(),
            "ledger_rows_deleted": ledger_deleted,
            "wal_checkpoint": wal_result,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&output).context("Failed to serialize cleanup result")?
        );
    } else {
        println!("Toche Cleanup");
        println!("=============");
        println!();
        println!(
            "  Orphan files deleted: {} ({} bytes)",
            deleted,
            format_bytes(freed_bytes)
        );
        if !candidates.legacy_untracked.is_empty() {
            println!(
                "  Legacy-untracked skipped: {} (not safe to delete automatically)",
                candidates.legacy_untracked.len()
            );
        }
        if ledger_deleted > 0 {
            println!("  Ledger rows deleted:  {}", ledger_deleted);
        } else {
            println!(
                "  Ledger retention:     {}",
                storage_cfg
                    .ledger_retention_days
                    .map(|d| format!("{d} days"))
                    .unwrap_or_else(|| "disabled".to_string())
            );
        }
        println!("  WAL checkpoint:       {}", wal_result);
    }

    Ok(())
}

fn fmt_optional(label: &str, value: Option<String>) {
    match value {
        Some(v) => println!("{label}: {v}"),
        None => println!("{label}: (unlimited)"),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

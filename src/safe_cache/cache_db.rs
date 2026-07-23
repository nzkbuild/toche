use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;

/// A row read back from the safe_cache table.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    #[allow(dead_code)] // public field for external inspection
    pub id: i64,
    pub project_path: String,
    pub fingerprint: String,
    pub workspace_fingerprint: String,
    pub response_hash: String,
    pub model: String,
    pub status: i32,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub created_at: String,
    pub last_hit_at: String,
    pub hit_count: i64,
}

/// Data needed to insert a new cache entry.
#[derive(Debug, Clone)]
pub struct NewCacheEntry {
    pub project_path: String,
    pub fingerprint: String,
    pub workspace_fingerprint: String,
    pub response_hash: String,
    pub model: String,
    pub status: i32,
    pub tokens_input: u64,
    pub tokens_output: u64,
}

/// Summary statistics for the storage subsystem.
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub cache_entries: u64,
    pub registered_blobs: u64,
    pub cas_bytes_on_disk: u64,
    pub free_disk_bytes: Option<u64>,
    pub ledger_rows: u64,
}

/// Classification of a CAS file found on disk that has no registry entry.
#[derive(Debug, Clone)]
pub struct OrphanCandidate {
    pub hash: String,
    pub bytes: u64,
}

/// Categorized candidates returned by orphan_candidates.
#[derive(Debug, Clone)]
pub struct OrphanCandidates {
    /// Blobs that were registered in cas_known but whose references are gone.
    /// Safe to delete with --orphans.
    pub safe_to_delete: Vec<OrphanCandidate>,
    /// Files on disk whose hash was never in cas_known (pre-existing reduce
    /// blobs from before M1B). Must NOT be deleted.
    pub legacy_untracked: Vec<OrphanCandidate>,
}

pub struct CacheDb {
    pub conn: Connection,
}

impl CacheDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let _ = conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;",
        );

        // Integrity check before any operations
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap_or_else(|_| "failed".into());
        if integrity != "ok" {
            anyhow::bail!("Database integrity check failed: {}", integrity);
        }

        // Schema version tracking (shared across all tables in this DB)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
            [],
        )?;

        let current_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        const EXPECTED_VERSION: i32 = 11;

        if current_version > EXPECTED_VERSION {
            anyhow::bail!(
                "Database was created by a newer version of Toche (schema version {} > {}). \
                 Please upgrade Toche or use a backup.",
                current_version,
                EXPECTED_VERSION
            );
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS safe_cache (
                id INTEGER PRIMARY KEY,
                project_path TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                workspace_fingerprint TEXT NOT NULL DEFAULT '',
                response_hash TEXT NOT NULL,
                model TEXT NOT NULL,
                status INTEGER NOT NULL DEFAULT 200,
                tokens_input INTEGER NOT NULL DEFAULT 0,
                tokens_output INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                last_hit_at TEXT NOT NULL,
                hit_count INTEGER NOT NULL DEFAULT 0,
                UNIQUE(project_path, fingerprint)
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_rejects (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                project_path TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                reason TEXT NOT NULL
            )",
            [],
        )?;

        // CAS known-blob set.  Every CAS blob (reduce or safe-cache)
        // must be registered here so orphan scans can exclude them.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cas_known (
                hash TEXT PRIMARY KEY
            )",
            [],
        )?;

        // Tracks which hashes were ever written via safe-cache insert().
        // Only hashes in this table can be safe_to_delete when their
        // safe_cache row is gone.  Reduce-only blobs are NOT here.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cas_cache_refs (
                hash TEXT PRIMARY KEY
            )",
            [],
        )?;

        // Backfill existing cache hashes into cas_known so older cache data
        // stays protected. Do not backfill cas_cache_refs: historical hashes
        // may be shared with reduction storage and cannot be proven safe to
        // remove automatically.
        let _ = conn.execute(
            "INSERT OR IGNORE INTO cas_known (hash) SELECT response_hash FROM safe_cache",
            [],
        );

        Ok(Self { conn })
    }

    // ── Registry ────────────────────────────────────────────────────────

    /// Record that a CAS blob hash exists.  Idempotent: repeated calls
    /// for the same hash do nothing.  Used by reduce and safe-cache writes.
    pub fn register_cas(&self, hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO cas_known (hash) VALUES (?1)",
            rusqlite::params![hash],
        )?;
        Ok(())
    }

    // ── Safe-cache CRUD ────────────────────────────────────────────────

    pub fn lookup(&self, project_path: &str, fingerprint: &str) -> Result<Option<CacheEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_path, fingerprint, workspace_fingerprint,
                    response_hash, model, status, tokens_input, tokens_output,
                    created_at, last_hit_at, hit_count
             FROM safe_cache
             WHERE project_path = ?1 AND fingerprint = ?2",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![project_path, fingerprint], |row| {
            Ok(CacheEntry {
                id: row.get(0)?,
                project_path: row.get(1)?,
                fingerprint: row.get(2)?,
                workspace_fingerprint: row.get(3)?,
                response_hash: row.get(4)?,
                model: row.get(5)?,
                status: row.get(6)?,
                tokens_input: row.get(7)?,
                tokens_output: row.get(8)?,
                created_at: row.get(9)?,
                last_hit_at: row.get(10)?,
                hit_count: row.get(11)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn insert(&self, entry: &NewCacheEntry) -> Result<()> {
        // Only a previously unknown hash is cache-owned. A known hash may
        // belong to reduction storage, so it must remain protected if cache
        // metadata later expires or clears.
        let known: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM cas_known WHERE hash = ?1)",
            rusqlite::params![entry.response_hash],
            |row| row.get(0),
        )?;
        self.register_cas(&entry.response_hash)?;
        if !known {
            self.conn.execute(
                "INSERT OR IGNORE INTO cas_cache_refs (hash) VALUES (?1)",
                rusqlite::params![entry.response_hash],
            )?;
        }

        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO safe_cache
             (project_path, fingerprint, workspace_fingerprint, response_hash,
              model, status, tokens_input, tokens_output, created_at, last_hit_at, hit_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)",
            rusqlite::params![
                entry.project_path,
                entry.fingerprint,
                entry.workspace_fingerprint,
                entry.response_hash,
                entry.model,
                entry.status,
                entry.tokens_input as i64,
                entry.tokens_output as i64,
                now,
                now,
            ],
        )?;

        Ok(())
    }

    /// Update last_hit_at and increment hit_count for a matched entry.
    pub fn touch(&self, project_path: &str, fingerprint: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE safe_cache SET last_hit_at = ?1, hit_count = hit_count + 1
             WHERE project_path = ?2 AND fingerprint = ?3",
            rusqlite::params![now, project_path, fingerprint],
        )?;
        Ok(())
    }

    /// Delete entries older than the given TTL in days. Returns count removed.
    /// CAS blobs are only deleted when no remaining safe_cache rows reference
    /// the hash.
    pub fn evict_expired(&self, ttl_days: u32) -> Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::days(ttl_days as i64);
        let n = self.conn.execute(
            "DELETE FROM safe_cache WHERE created_at < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
        )?;
        Ok(n as u64)
    }

    /// Delete entries, optionally filtered by project. Returns count removed.
    /// Also cleans the cache_rejects table for the same scope.
    /// CAS blobs are only deleted when no remaining safe_cache rows reference
    /// the hash.
    pub fn clear(&self, project_path: Option<&str>) -> Result<u64> {
        let n = match project_path {
            Some(p) => {
                let count = self.conn.execute(
                    "DELETE FROM safe_cache WHERE project_path = ?1",
                    rusqlite::params![p],
                )?;
                let _ = self.conn.execute(
                    "DELETE FROM cache_rejects WHERE project_path = ?1",
                    rusqlite::params![p],
                )?;
                count
            }
            None => {
                let count = self.conn.execute("DELETE FROM safe_cache", [])?;
                let _ = self.conn.execute("DELETE FROM cache_rejects", [])?;
                count
            }
        };
        Ok(n as u64)
    }

    /// List recent cache entries, optionally filtered by project.
    pub fn list(&self, project_path: Option<&str>, limit: u32) -> Result<Vec<CacheEntry>> {
        let rows = match project_path {
            Some(p) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, project_path, fingerprint, workspace_fingerprint,
                            response_hash, model, status, tokens_input, tokens_output,
                            created_at, last_hit_at, hit_count
                     FROM safe_cache
                     WHERE project_path = ?1
                     ORDER BY last_hit_at DESC LIMIT ?2",
                )?;
                stmt.query_map(rusqlite::params![p, limit as i64], |row| {
                    Ok(CacheEntry {
                        id: row.get(0)?,
                        project_path: row.get(1)?,
                        fingerprint: row.get(2)?,
                        workspace_fingerprint: row.get(3)?,
                        response_hash: row.get(4)?,
                        model: row.get(5)?,
                        status: row.get(6)?,
                        tokens_input: row.get(7)?,
                        tokens_output: row.get(8)?,
                        created_at: row.get(9)?,
                        last_hit_at: row.get(10)?,
                        hit_count: row.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, project_path, fingerprint, workspace_fingerprint,
                            response_hash, model, status, tokens_input, tokens_output,
                            created_at, last_hit_at, hit_count
                     FROM safe_cache
                     ORDER BY last_hit_at DESC LIMIT ?1",
                )?;
                stmt.query_map(rusqlite::params![limit as i64], |row| {
                    Ok(CacheEntry {
                        id: row.get(0)?,
                        project_path: row.get(1)?,
                        fingerprint: row.get(2)?,
                        workspace_fingerprint: row.get(3)?,
                        response_hash: row.get(4)?,
                        model: row.get(5)?,
                        status: row.get(6)?,
                        tokens_input: row.get(7)?,
                        tokens_output: row.get(8)?,
                        created_at: row.get(9)?,
                        last_hit_at: row.get(10)?,
                        hit_count: row.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    }

    /// Record a cache rejection for metrics tracking.
    pub fn insert_reject(&self, project_path: &str, fingerprint: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO cache_rejects (timestamp, project_path, fingerprint, reason)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![now, project_path, fingerprint, reason],
        )?;
        Ok(())
    }

    /// Count rejected candidates, optionally filtered by project.
    #[allow(dead_code)] // public API for stats/status
    pub fn count_rejects(&self, project_path: Option<&str>) -> Result<u64> {
        let count: i64 = match project_path {
            Some(p) => self.conn.query_row(
                "SELECT COUNT(*) FROM cache_rejects WHERE project_path = ?1",
                rusqlite::params![p],
                |row| row.get(0),
            )?,
            None => self
                .conn
                .query_row("SELECT COUNT(*) FROM cache_rejects", [], |row| row.get(0))?,
        };
        Ok(count as u64)
    }

    /// Delete rejected entries older than the given TTL. Returns count removed.
    pub fn evict_expired_rejects(&self, ttl_days: u32) -> Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::days(ttl_days as i64);
        let n = self.conn.execute(
            "DELETE FROM cache_rejects WHERE timestamp < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
        )?;
        Ok(n as u64)
    }

    /// Total number of cache entries, optionally filtered by project.
    #[allow(dead_code)] // public API for stats/status
    pub fn count(&self, project_path: Option<&str>) -> Result<u64> {
        let count: i64 = match project_path {
            Some(p) => self.conn.query_row(
                "SELECT COUNT(*) FROM safe_cache WHERE project_path = ?1",
                rusqlite::params![p],
                |row| row.get(0),
            )?,
            None => self
                .conn
                .query_row("SELECT COUNT(*) FROM safe_cache", [], |row| row.get(0))?,
        };
        Ok(count as u64)
    }

    // ── Storage stats ──────────────────────────────────────────────────

    /// Return storage subsystem summary: cache entries, known blob count,
    /// on-disk CAS bytes, free disk bytes, and ledger row count.
    pub fn storage_stats(&self, cas_dir: &Path) -> Result<StorageStats> {
        let cache_entries = self.count(None).unwrap_or(0);

        let registered_blobs: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM cas_known", [], |row| row.get(0))
            .unwrap_or(0);

        let cas_bytes_on_disk = if cas_dir.is_dir() {
            count_cas_bytes_recursive(cas_dir)
        } else {
            0
        };

        let free_disk_bytes = free_bytes_under(cas_dir);
        let ledger_rows = ledger_count(&self.conn).unwrap_or(0);

        Ok(StorageStats {
            cache_entries,
            registered_blobs: registered_blobs as u64,
            cas_bytes_on_disk,
            free_disk_bytes,
            ledger_rows,
        })
    }

    /// Scan the CAS directory and classify every hex-valid file:
    /// - `safe_to_delete`: once registered in `cas_known` but no longer
    ///   referenced by any `safe_cache` row.
    /// - `legacy_untracked`: hash was NEVER registered (pre-M1B reduce
    ///   blob). Must never be deleted by automated cleanup.
    ///
    /// Does NOT delete anything; manual confirmation is required.
    pub fn orphan_candidates(&self, cas_dir: &Path) -> Result<OrphanCandidates> {
        let mut safe_to_delete = Vec::new();
        let mut legacy_untracked = Vec::new();

        if !cas_dir.is_dir() {
            return Ok(OrphanCandidates {
                safe_to_delete,
                legacy_untracked,
            });
        }

        // Collect known hashes and hashes still referenced by safe_cache.
        // Also collect cas_cache_refs — only these can be safe_to_delete.
        let mut known_stmt = self.conn.prepare("SELECT hash FROM cas_known")?;
        let known: std::collections::HashSet<String> = known_stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut ref_stmt = self
            .conn
            .prepare("SELECT DISTINCT response_hash FROM safe_cache")?;
        let referenced: std::collections::HashSet<String> = ref_stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut cache_ref_stmt = self.conn.prepare("SELECT hash FROM cas_cache_refs")?;
        let cache_refs: std::collections::HashSet<String> = cache_ref_stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let entries = std::fs::read_dir(cas_dir)?;
        for entry in entries.flatten() {
            let sub_path = entry.path();
            if !sub_path.is_dir() {
                continue;
            }
            let first = sub_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if first.len() != 2 || !first.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            let Ok(sub_entries) = std::fs::read_dir(&sub_path) else {
                continue;
            };
            for file_entry in sub_entries.flatten() {
                let file_path = file_entry.path();
                if !file_path.is_file() {
                    continue;
                }
                let file_size = file_entry.metadata().map(|m| m.len()).unwrap_or(0);
                let second = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if second.len() != 62 || !second.chars().all(|c| c.is_ascii_hexdigit()) {
                    continue;
                }
                let hash = format!("{first}{second}");

                if !known.contains(&hash) {
                    // Never registered — pre-M1B blob, must not delete.
                    legacy_untracked.push(OrphanCandidate {
                        hash,
                        bytes: file_size,
                    });
                } else if cache_refs.contains(&hash) && !referenced.contains(&hash) {
                    // Hash was written via safe_cache insert(), entry was
                    // later evicted/cleared, but blob survived.  Safe.
                    safe_to_delete.push(OrphanCandidate {
                        hash,
                        bytes: file_size,
                    });
                }
                // else: known AND (reduce-only OR still-referenced) → skip
            }
        }

        safe_to_delete.sort_by_key(|b| std::cmp::Reverse(b.bytes));
        legacy_untracked.sort_by_key(|b| std::cmp::Reverse(b.bytes));
        Ok(OrphanCandidates {
            safe_to_delete,
            legacy_untracked,
        })
    }
}

/// Recursively count on-disk bytes for well-formed CAS files under
/// `<cas_dir>/<first2>/<remaining62>`.
/// Skips directories that aren't exactly two hex chars and files that
/// contain any non-hex characters.
fn count_cas_bytes_recursive(cas_dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = std::fs::read_dir(cas_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let sub_path = entry.path();
        if !sub_path.is_dir() {
            continue;
        }
        let first = sub_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if first.len() != 2 || !first.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(sub_entries) = std::fs::read_dir(&sub_path) else {
            continue;
        };
        for file_entry in sub_entries.flatten() {
            let file_path = file_entry.path();
            if !file_path.is_file() {
                continue;
            }
            let second = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if second.len() != 62 || !second.chars().all(|c| c.is_ascii_hexdigit()) {
                continue;
            }
            if let Ok(meta) = file_entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Best-effort free bytes on the filesystem containing `path`.
/// Uses `df -B1 --output=avail` on Unix.  Returns None when unavailable.
pub fn free_bytes_under(path: &Path) -> Option<u64> {
    let target = if path.exists() {
        path.to_string_lossy().to_string()
    } else {
        let ancestor = path.ancestors().find(|a| a.exists())?;
        ancestor.to_string_lossy().to_string()
    };
    #[cfg(unix)]
    {
        let output = std::process::Command::new("df")
            .args(["-B1", "--output=avail", &target])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = std::str::from_utf8(&output.stdout).ok()?;
        stdout.lines().nth(1)?.trim().parse::<u64>().ok()
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        None
    }
}

/// Count ledger rows for dry-run / status reporting.
pub fn ledger_count(conn: &Connection) -> Result<u64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM ledger", [], |row| row.get(0))?;
    Ok(n as u64)
}

/// Delete ledger rows older than `retention_days`.  Returns count removed.
pub fn ledger_delete_older_than(conn: &Connection, retention_days: u32) -> Result<u64> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let n = conn.execute(
        "DELETE FROM ledger WHERE timestamp < ?1",
        rusqlite::params![cutoff.to_rfc3339()],
    )?;
    Ok(n as u64)
}

/// Perform WAL checkpoint(TRUNCATE) on the shared DB connection.
/// Returns Ok(checkpointed_pages) or an error message.
pub fn wal_checkpoint(conn: &Connection) -> Result<String> {
    let (busy, log, ckpt) = {
        let mut stmt = conn.prepare_cached("PRAGMA wal_checkpoint(TRUNCATE)")?;
        stmt.query_row([], |row| {
            Ok((
                row.get::<_, i32>(0).unwrap_or(-1),
                row.get::<_, i32>(1).unwrap_or(-1),
                row.get::<_, i32>(2).unwrap_or(-1),
            ))
        })?
    };
    if ckpt >= 0 {
        Ok(format!(
            "WAL checkpoint TRUNCATE: busy={busy}, log={log}, checkpointed={ckpt}"
        ))
    } else {
        Ok("WAL checkpoint reported no progress".to_string())
    }
}

/// Check whether a free-disk measurement is available on this platform.
pub fn free_disk_measurable() -> bool {
    #[cfg(unix)]
    {
        true
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn test_db() -> CacheDb {
        CacheDb::open(Path::new(":memory:")).expect("in-memory cache db")
    }

    #[test]
    fn insert_and_lookup() {
        let db = test_db();
        db.insert(&NewCacheEntry {
            project_path: "/test/project".into(),
            fingerprint: "a".repeat(64),
            workspace_fingerprint: "b".repeat(64),
            response_hash: "c".repeat(64),
            model: "claude-sonnet-5".into(),
            status: 200,
            tokens_input: 1000,
            tokens_output: 200,
        })
        .unwrap();

        let entry = db
            .lookup("/test/project", &"a".repeat(64))
            .unwrap()
            .expect("entry should exist");
        assert_eq!(entry.model, "claude-sonnet-5");
        assert_eq!(entry.tokens_input, 1000);
        assert_eq!(entry.hit_count, 1);
    }

    #[test]
    fn insert_replaces_existing() {
        let db = test_db();
        let fp = "f".repeat(64);
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: fp.clone(),
            workspace_fingerprint: "w1".repeat(32),
            response_hash: "r1".repeat(32),
            model: "m1".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        // Insert again with different response
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: fp.clone(),
            workspace_fingerprint: "w2".repeat(32),
            response_hash: "r2".repeat(32),
            model: "m2".into(),
            status: 200,
            tokens_input: 200,
            tokens_output: 100,
        })
        .unwrap();

        let entry = db.lookup("/p", &fp).unwrap().expect("should exist");
        assert_eq!(entry.model, "m2"); // replaced
        assert_eq!(entry.hit_count, 1); // reset
    }

    #[test]
    fn lookup_miss_returns_none() {
        let db = test_db();
        let result = db.lookup("/nonexistent", &"a".repeat(64)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn touch_increments_hit_count() {
        let db = test_db();
        let fp = "t".repeat(64);
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: fp.clone(),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "r".repeat(64),
            model: "claude-sonnet-5".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        db.touch("/p", &fp).unwrap();
        db.touch("/p", &fp).unwrap();

        let entry = db.lookup("/p", &fp).unwrap().expect("should exist");
        assert_eq!(entry.hit_count, 3); // 1 initial + 2 touches
    }

    #[test]
    fn evict_expired_removes_old_entries() {
        let db = test_db();
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: "e".repeat(64),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "r".repeat(64),
            model: "claude-sonnet-5".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        let old_date = (Utc::now() - chrono::Duration::days(2)).to_rfc3339();
        db.conn
            .execute(
                "UPDATE safe_cache SET created_at = ?1",
                rusqlite::params![old_date],
            )
            .unwrap();

        let removed = db.evict_expired(1).unwrap();
        assert_eq!(removed, 1);
        assert!(db.lookup("/p", &"e".repeat(64)).unwrap().is_none());
    }

    #[test]
    fn clear_all_removes_everything() {
        let db = test_db();
        db.insert(&NewCacheEntry {
            project_path: "/p1".into(),
            fingerprint: "a".repeat(64),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "r".repeat(64),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();
        db.insert(&NewCacheEntry {
            project_path: "/p2".into(),
            fingerprint: "b".repeat(64),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "r".repeat(64),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        let removed = db.clear(None).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(db.count(None).unwrap(), 0);
    }

    #[test]
    fn clear_by_project_only_removes_scoped() {
        let db = test_db();
        db.insert(&NewCacheEntry {
            project_path: "/p1".into(),
            fingerprint: "a".repeat(64),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "r".repeat(64),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();
        db.insert(&NewCacheEntry {
            project_path: "/p2".into(),
            fingerprint: "b".repeat(64),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "r".repeat(64),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        let removed = db.clear(Some("/p1")).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(db.count(None).unwrap(), 1);
        assert!(db.lookup("/p1", &"a".repeat(64)).unwrap().is_none());
        assert!(db.lookup("/p2", &"b".repeat(64)).unwrap().is_some());
    }

    #[test]
    fn list_respects_limit() {
        let db = test_db();
        for i in 0..5 {
            db.insert(&NewCacheEntry {
                project_path: "/p".into(),
                fingerprint: format!("{}", i).repeat(32),
                workspace_fingerprint: "w".repeat(64),
                response_hash: "r".repeat(64),
                model: "m".into(),
                status: 200,
                tokens_input: 100,
                tokens_output: 50,
            })
            .unwrap();
        }
        let entries = db.list(None, 3).unwrap();
        assert_eq!(entries.len(), 3);
    }

    // ── CAS registry / known-blob-set tests ─────────────────────────────

    #[test]
    fn register_cas_is_idempotent() {
        let db = test_db();
        let hash = "ab".repeat(32);
        db.register_cas(&hash).unwrap();
        db.register_cas(&hash).unwrap(); // second call is no-op

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM cas_known WHERE hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn shared_cas_not_deleted_while_cache_references_remain() {
        let db = test_db();
        let shared_hash = "shared".repeat(16);

        // Two cache entries share the same CAS blob
        db.insert(&NewCacheEntry {
            project_path: "/p1".into(),
            fingerprint: "fp_a".repeat(32),
            workspace_fingerprint: "w1".repeat(32),
            response_hash: shared_hash.clone(),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();
        db.insert(&NewCacheEntry {
            project_path: "/p2".into(),
            fingerprint: "fp_b".repeat(32),
            workspace_fingerprint: "w2".repeat(32),
            response_hash: shared_hash.clone(),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        // Clear one project
        let removed = db.clear(Some("/p1")).unwrap();
        assert_eq!(removed, 1);

        // Hash should still be known (other project still references it)
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM cas_known WHERE hash = ?1",
                rusqlite::params![shared_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0, "shared CAS should still be known");
    }

    #[test]
    fn reduction_owned_hash_stays_protected_after_cache_clear() {
        let db = test_db();
        let dir = tempdir().unwrap();
        let cas = dir.path().join("cas");
        let hash = "de".repeat(32);
        std::fs::create_dir_all(cas.join(&hash[..2])).unwrap();
        std::fs::write(cas.join(&hash[..2]).join(&hash[2..]), b"reduce").unwrap();

        // Reduction registers ownership before a cache entry later reuses it.
        db.register_cas(&hash).unwrap();
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: "fp".repeat(32),
            workspace_fingerprint: "w".repeat(64),
            response_hash: hash,
            model: "m".into(),
            status: 200,
            tokens_input: 0,
            tokens_output: 0,
        })
        .unwrap();
        db.clear(None).unwrap();

        let classified = db.orphan_candidates(&cas).unwrap();
        assert!(classified.safe_to_delete.is_empty());
    }

    #[test]
    fn known_blob_remains_protected_after_last_cache_reference_gone() {
        let db = test_db();
        let hash = "solo".repeat(16);

        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: "fp".repeat(32),
            workspace_fingerprint: "w".repeat(64),
            response_hash: hash.clone(),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        db.clear(None).unwrap();

        // A known blob can be shared with reduction storage.  Cache removal
        // must not make it a deletion candidate; confirmed cleanup owns disk
        // reclamation.
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM cas_known WHERE hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn orphan_candidates_classifies_never_registered_as_legacy() {
        let db = test_db();
        let dir = tempdir().unwrap();
        let cas = dir.path().join("cas");
        let hex_file = "cd".repeat(32);
        let first2 = &hex_file[..2];
        std::fs::create_dir_all(cas.join(first2)).unwrap();
        std::fs::write(cas.join(first2).join(&hex_file[2..]), b"fake blob").unwrap();

        // Register a different hash
        db.register_cas(&"00".repeat(32)).unwrap();

        let classified = db.orphan_candidates(&cas).unwrap();
        let legacy_hashes: Vec<_> = classified
            .legacy_untracked
            .iter()
            .map(|o| &o.hash)
            .collect();
        assert!(
            legacy_hashes.contains(&&hex_file),
            "never-registered CAS file should be classified as legacy-untracked"
        );
        assert!(
            classified.safe_to_delete.is_empty(),
            "no safe-to-delete candidates expected"
        );
    }

    #[test]
    fn orphan_candidates_skips_known_referenced_blobs() {
        let db = test_db();
        let dir = tempdir().unwrap();
        let cas = dir.path().join("cas");
        let hash = "fe".repeat(32);
        let first2 = &hash[..2];
        std::fs::create_dir_all(cas.join(first2)).unwrap();
        std::fs::write(cas.join(first2).join(&hash[2..]), b"registered").unwrap();

        db.register_cas(&hash).unwrap();

        let classified = db.orphan_candidates(&cas).unwrap();
        assert!(classified.safe_to_delete.is_empty());
        assert!(classified.legacy_untracked.is_empty());
    }

    #[test]
    fn once_known_but_unreferenced_is_safe_to_delete() {
        let db = test_db();
        let dir = tempdir().unwrap();
        let cas = dir.path().join("cas");
        let hash = "de".repeat(32);
        let first2 = &hash[..2];
        std::fs::create_dir_all(cas.join(first2)).unwrap();
        std::fs::write(cas.join(first2).join(&hash[2..]), b"orphan").unwrap();

        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: "fp".repeat(32),
            workspace_fingerprint: "w".repeat(64),
            response_hash: hash.clone(),
            model: "m".into(),
            status: 200,
            tokens_input: 0,
            tokens_output: 0,
        })
        .unwrap();
        // clear() leaves the safe-cache lifecycle marker, so this is safe.
        db.clear(None).unwrap();

        let classified = db.orphan_candidates(&cas).unwrap();
        assert!(
            !classified.safe_to_delete.is_empty(),
            "known-but-unreferenced hash should be safe_to_delete"
        );
        assert!(
            classified.legacy_untracked.is_empty(),
            "no legacy-untracked expected"
        );
    }

    #[test]
    fn storage_stats_returns_cache_and_blob_counts() {
        let db = test_db();
        db.insert(&NewCacheEntry {
            project_path: "/p".into(),
            fingerprint: "fp".repeat(32),
            workspace_fingerprint: "w".repeat(64),
            response_hash: "st".repeat(32),
            model: "m".into(),
            status: 200,
            tokens_input: 100,
            tokens_output: 50,
        })
        .unwrap();

        let dir = tempdir().unwrap();
        let cas = dir.path().join("cas");
        // Create a CAS blob so on-disk count is non-zero
        let cr_hash = "ef".repeat(32);
        let first2 = &cr_hash[..2];
        std::fs::create_dir_all(cas.join(first2)).unwrap();
        std::fs::write(cas.join(first2).join(&cr_hash[2..]), b"content").unwrap();
        db.register_cas(&cr_hash).unwrap();

        let stats = db.storage_stats(&cas).unwrap();
        assert_eq!(stats.cache_entries, 1);
        assert!(stats.registered_blobs > 0);
        assert!(stats.cas_bytes_on_disk > 0);
    }
}

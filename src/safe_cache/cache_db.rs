use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;

/// A row read back from the safe_cache table.
#[derive(Debug, Clone)]
pub struct CacheEntry {
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

pub struct CacheDb {
    conn: Connection,
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

        const EXPECTED_VERSION: i32 = 9;

        if current_version > EXPECTED_VERSION {
            anyhow::bail!(
                "Database was created by a newer version of Toche (schema version {} > {}). \
                 Please upgrade Toche or use a backup.",
                current_version,
                EXPECTED_VERSION
            );
        }

        // Note: version 1 is the ledger table (managed by meter/db.rs).
        // Safe-cache table starts at version 8.
        if current_version < 8 {
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
            conn.execute("INSERT INTO schema_version (version) VALUES (8)", [])?;
        }

        Ok(Self { conn })
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
        // Insert with 0 TTL means it's immediately expired
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

        // Manually backdate the entry to make it 2 days old
        let old_date = (Utc::now() - chrono::Duration::days(2)).to_rfc3339();
        db.conn
            .execute(
                "UPDATE safe_cache SET created_at = ?1",
                rusqlite::params![old_date],
            )
            .unwrap();

        let removed = db.evict_expired(1).unwrap(); // TTL = 1 day
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
}

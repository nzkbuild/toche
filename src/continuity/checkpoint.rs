use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;

use crate::safe_cache::workspace;

/// A row read back from the checkpoints table.
#[derive(Debug, Clone)]
pub struct CheckpointEntry {
    pub id: i64,
    pub project_path: String,
    pub git_head: String,
    pub workspace_fingerprint: String,
    pub task: String,
    pub completed: String,
    pub changed_files: String,
    pub verification: String,
    pub open_risks: String,
    pub next_action: String,
    pub facts_json: String,
    pub model_assisted: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Data needed to insert a new checkpoint.
#[derive(Debug, Clone)]
pub struct NewCheckpoint {
    pub project_path: String,
    pub task: String,
    pub completed: String,
    pub changed_files: String,
    pub verification: String,
    pub open_risks: String,
    pub next_action: String,
    pub facts_json: String,
    pub model_assisted: bool,
}

pub struct CheckpointDb {
    conn: Connection,
}

impl CheckpointDb {
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

        // Note: versions 1-7 are ledger table versions, version 8 is safe_cache.
        // Checkpoints table starts at version 9.
        if current_version < 9 {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS checkpoints (
                    id INTEGER PRIMARY KEY,
                    project_path TEXT NOT NULL,
                    git_head TEXT NOT NULL DEFAULT '',
                    workspace_fingerprint TEXT NOT NULL DEFAULT '',
                    task TEXT NOT NULL DEFAULT '',
                    completed TEXT NOT NULL DEFAULT '',
                    changed_files TEXT NOT NULL DEFAULT '',
                    verification TEXT NOT NULL DEFAULT '',
                    open_risks TEXT NOT NULL DEFAULT '',
                    next_action TEXT NOT NULL DEFAULT '',
                    facts_json TEXT NOT NULL DEFAULT '{}',
                    model_assisted INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                )",
                [],
            )?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (9)",
                [],
            )?;
        }

        Ok(Self { conn })
    }

    pub fn insert(&self, entry: &NewCheckpoint) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let git_head = current_git_head();
        let ws_fp = workspace::compute_workspace_fingerprint();
        self.conn.execute(
            "INSERT INTO checkpoints
             (project_path, git_head, workspace_fingerprint, task, completed,
              changed_files, verification, open_risks, next_action,
              facts_json, model_assisted, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                entry.project_path,
                git_head,
                ws_fp,
                entry.task,
                entry.completed,
                entry.changed_files,
                entry.verification,
                entry.open_risks,
                entry.next_action,
                entry.facts_json,
                entry.model_assisted as i64,
                now,
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn latest(&self, project_path: &str) -> Result<Option<CheckpointEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_path, git_head, workspace_fingerprint,
                    task, completed, changed_files, verification,
                    open_risks, next_action, facts_json, model_assisted,
                    created_at, updated_at
             FROM checkpoints
             WHERE project_path = ?1
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![project_path], row_to_entry)?;
        Ok(rows.next().transpose()?)
    }

    pub fn list(&self, project_path: &str, limit: u32) -> Result<Vec<CheckpointEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_path, git_head, workspace_fingerprint,
                    task, completed, changed_files, verification,
                    open_risks, next_action, facts_json, model_assisted,
                    created_at, updated_at
             FROM checkpoints
             WHERE project_path = ?1
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![project_path, limit as i64], row_to_entry)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get(&self, id: i64) -> Result<Option<CheckpointEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_path, git_head, workspace_fingerprint,
                    task, completed, changed_files, verification,
                    open_risks, next_action, facts_json, model_assisted,
                    created_at, updated_at
             FROM checkpoints WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], row_to_entry)?;
        Ok(rows.next().transpose()?)
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM checkpoints WHERE id = ?1", rusqlite::params![id])?;
        Ok(n > 0)
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<CheckpointEntry> {
    Ok(CheckpointEntry {
        id: row.get(0)?,
        project_path: row.get(1)?,
        git_head: row.get(2)?,
        workspace_fingerprint: row.get(3)?,
        task: row.get(4)?,
        completed: row.get(5)?,
        changed_files: row.get(6)?,
        verification: row.get(7)?,
        open_risks: row.get(8)?,
        next_action: row.get(9)?,
        facts_json: row.get(10)?,
        model_assisted: row.get::<_, i64>(11)? != 0,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn current_git_head() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> CheckpointDb {
        CheckpointDb::open(Path::new(":memory:")).expect("in-memory checkpoint db")
    }

    fn new_entry(task: &str) -> NewCheckpoint {
        NewCheckpoint {
            project_path: "/test/project".into(),
            task: task.into(),
            completed: String::new(),
            changed_files: String::new(),
            verification: String::new(),
            open_risks: String::new(),
            next_action: String::new(),
            facts_json: "{}".into(),
            model_assisted: false,
        }
    }

    #[test]
    fn insert_and_retrieve() {
        let db = test_db();
        let id = db.insert(&new_entry("task one")).unwrap();
        assert!(id > 0);

        let entry = db.get(id).unwrap().expect("should exist");
        assert_eq!(entry.task, "task one");
        assert!(!entry.created_at.is_empty());
    }

    #[test]
    fn latest_returns_most_recent() {
        let db = test_db();
        db.insert(&new_entry("first")).unwrap();
        db.insert(&new_entry("second")).unwrap();

        let latest = db.latest("/test/project").unwrap().expect("should exist");
        assert_eq!(latest.task, "second");
    }

    #[test]
    fn list_respects_limit() {
        let db = test_db();
        for i in 0..5 {
            db.insert(&new_entry(&format!("task {i}"))).unwrap();
        }
        let entries = db.list("/test/project", 3).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn delete_removes() {
        let db = test_db();
        let id = db.insert(&new_entry("to delete")).unwrap();
        assert!(db.delete(id).unwrap());
        assert!(db.get(id).unwrap().is_none());
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        let db = test_db();
        assert!(!db.delete(999).unwrap());
    }

    #[test]
    fn auto_populates_git_and_workspace() {
        let db = test_db();
        let id = db.insert(&new_entry("test")).unwrap();
        let entry = db.get(id).unwrap().expect("should exist");

        // workspace fingerprint is always 64 hex chars
        assert_eq!(entry.workspace_fingerprint.len(), 64);
        // git_head may be empty in CI, but field exists
        assert!(entry.created_at == entry.updated_at);
    }
}

use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::meter::types::{DayBreakdown, LedgerEntry, ModelBreakdown, StatsSummary, UsageBreakdown};

/// Represents a request ready to be inserted into the ledger.
pub struct NewLedgerRecord {
    pub timestamp: chrono::DateTime<Utc>,
    pub model: String,
    pub profile_name: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub coalesced_count: u64,
    pub latency_ms: u64,
    pub status: String,
    pub cost: Option<f64>,
    pub project_path: String,
}

pub struct LedgerDb {
    conn: Connection,
}

impl LedgerDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let _ = conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;",
        );
        conn.execute(
            "CREATE TABLE IF NOT EXISTS ledger (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                model TEXT NOT NULL,
                profile_name TEXT NOT NULL DEFAULT '',
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
                coalesced_count INTEGER NOT NULL DEFAULT 0,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'success',
                cost REAL,
                project_path TEXT NOT NULL DEFAULT ''
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ledger_timestamp ON ledger(timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ledger_project ON ledger(project_path, timestamp)",
            [],
        )?;

        // Migration: add coalesced_count to existing databases
        let has_column: bool = {
            let mut stmt =
                conn.prepare("PRAGMA table_info(ledger)")?;
            let columns: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            columns.iter().any(|c| c == "coalesced_count")
        };
        if !has_column {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN coalesced_count INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }

        Ok(Self { conn })
    }

    pub fn record(&self, entry: &NewLedgerRecord) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ledger (timestamp, model, profile_name, input_tokens, output_tokens,
             cache_read_input_tokens, cache_creation_input_tokens, coalesced_count, latency_ms, status, cost, project_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                entry.timestamp.to_rfc3339(),
                entry.model,
                entry.profile_name,
                entry.input_tokens as i64,
                entry.output_tokens as i64,
                entry.cache_read_input_tokens as i64,
                entry.cache_creation_input_tokens as i64,
                entry.coalesced_count as i64,
                entry.latency_ms as i64,
                entry.status,
                entry.cost,
                entry.project_path,
            ],
        )?;
        self.cleanup_old()?;
        Ok(self.conn.last_insert_rowid())
    }

    fn cleanup_old(&self) -> Result<()> {
        let cutoff = Utc::now() - chrono::Duration::days(90);
        self.conn.execute(
            "DELETE FROM ledger WHERE timestamp < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
        )?;
        Ok(())
    }

    fn project_filter(project_path: Option<&str>) -> (Option<String>, Option<String>) {
        match project_path {
            Some(p) => (
                Some(p.to_string()),
                Some(format!("{}{}*", p, std::path::MAIN_SEPARATOR)),
            ),
            None => (None, None),
        }
    }

    fn build_breakdown(
        &self,
        model_filter: &str,
        project_exact: &Option<String>,
        project_glob: &Option<String>,
    ) -> Result<UsageBreakdown> {
        if model_filter.is_empty() {
            let sql = "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cache_read_input_tokens), 0), COALESCE(SUM(cache_creation_input_tokens), 0),
                    COALESCE(SUM(coalesced_count), 0),
                    COALESCE(AVG(latency_ms), 0),
                    COALESCE(SUM(CASE WHEN cost IS NOT NULL THEN cost ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN cost IS NULL THEN 1 ELSE 0 END), 0)
             FROM ledger
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)";

            let result = self.conn.query_row(
                sql,
                rusqlite::params![project_exact, project_glob],
                |row| {
                    Ok(UsageBreakdown {
                        total_requests: row.get::<_, i64>(0)? as u64,
                        input_tokens: row.get::<_, i64>(1)? as u64,
                        output_tokens: row.get::<_, i64>(2)? as u64,
                        cache_read_input_tokens: row.get::<_, i64>(3)? as u64,
                        cache_creation_input_tokens: row.get::<_, i64>(4)? as u64,
                        coalesced_count: row.get::<_, i64>(5)? as u64,
                        avg_latency_ms: row.get::<_, f64>(6)?,
                        total_cost_known: row.get::<_, f64>(7)?,
                        total_cost_unknown_requests: row.get::<_, i64>(8)? as u64,
                    })
                },
            )?;
            Ok(result)
        } else {
            let sql = "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cache_read_input_tokens), 0), COALESCE(SUM(cache_creation_input_tokens), 0),
                    COALESCE(SUM(coalesced_count), 0),
                    COALESCE(AVG(latency_ms), 0),
                    COALESCE(SUM(CASE WHEN cost IS NOT NULL THEN cost ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN cost IS NULL THEN 1 ELSE 0 END), 0)
             FROM ledger
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2) AND model = ?3";

            let result = self.conn.query_row(
                sql,
                rusqlite::params![project_exact, project_glob, model_filter],
                |row| {
                    Ok(UsageBreakdown {
                        total_requests: row.get::<_, i64>(0)? as u64,
                        input_tokens: row.get::<_, i64>(1)? as u64,
                        output_tokens: row.get::<_, i64>(2)? as u64,
                        cache_read_input_tokens: row.get::<_, i64>(3)? as u64,
                        cache_creation_input_tokens: row.get::<_, i64>(4)? as u64,
                        coalesced_count: row.get::<_, i64>(5)? as u64,
                        avg_latency_ms: row.get::<_, f64>(6)?,
                        total_cost_known: row.get::<_, f64>(7)?,
                        total_cost_unknown_requests: row.get::<_, i64>(8)? as u64,
                    })
                },
            )?;
            Ok(result)
        }
    }

    pub fn get_summary(&self, project_path: Option<&str>) -> Result<StatsSummary> {
        let (exact, glob) = Self::project_filter(project_path);

        let total = self.build_breakdown("", &exact, &glob)?;

        let by_model: Vec<ModelBreakdown> = {
            let mut stmt = self.conn.prepare(
                "SELECT model FROM ledger
                 WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
                 GROUP BY model ORDER BY COUNT(*) DESC",
            )?;
            let models: Vec<String> = stmt
                .query_map(rusqlite::params![exact, glob], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;

            models
                .into_iter()
                .map(|model| -> Result<ModelBreakdown> {
                    let b = self.build_breakdown(&model, &exact, &glob)?;
                    Ok(ModelBreakdown { model, breakdown: b })
                })
                .collect::<Result<Vec<_>>>()?
        };

        let by_day: Vec<DayBreakdown> = {
            let mut stmt = self.conn.prepare(
                "SELECT DATE(timestamp) as date,
                        COUNT(*) as commands,
                        COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_input_tokens), 0),
                        COALESCE(SUM(cache_creation_input_tokens), 0),
                        COALESCE(SUM(coalesced_count), 0),
                        COALESCE(AVG(latency_ms), 0),
                        COALESCE(SUM(CASE WHEN cost IS NOT NULL THEN cost ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN cost IS NULL THEN 1 ELSE 0 END), 0)
                 FROM ledger
                 WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
                 GROUP BY DATE(timestamp) ORDER BY DATE(timestamp) DESC LIMIT 30",
            )?;
            let rows = stmt.query_map(rusqlite::params![exact, glob], |row| {
                let input = row.get::<_, i64>(2)? as u64;
                let output = row.get::<_, i64>(3)? as u64;
                let commands = row.get::<_, i64>(1)? as u64;
                Ok(DayBreakdown {
                    date: row.get(0)?,
                    breakdown: UsageBreakdown {
                        total_requests: commands,
                        input_tokens: input,
                        output_tokens: output,
                        cache_read_input_tokens: row.get::<_, i64>(4)? as u64,
                        cache_creation_input_tokens: row.get::<_, i64>(5)? as u64,
                        coalesced_count: row.get::<_, i64>(6)? as u64,
                        avg_latency_ms: row.get::<_, f64>(7)?,
                        total_cost_known: row.get::<_, f64>(8)?,
                        total_cost_unknown_requests: row.get::<_, i64>(9)? as u64,
                    },
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        Ok(StatsSummary {
            total,
            by_model,
            by_day,
        })
    }

    pub fn get_entries(&self, limit: u32, project_path: Option<&str>) -> Result<Vec<LedgerEntry>> {
        let (exact, glob) = Self::project_filter(project_path);
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, model, profile_name, input_tokens, output_tokens,
                    cache_read_input_tokens, cache_creation_input_tokens, coalesced_count, latency_ms, status, cost, project_path
             FROM ledger
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             ORDER BY timestamp DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![exact, glob, limit as i64],
            |row| {
                let ts: String = row.get(1)?;
                let parsed = chrono::DateTime::parse_from_rfc3339(&ts)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(LedgerEntry {
                    id: row.get(0)?,
                    timestamp: parsed,
                    model: row.get(2)?,
                    profile_name: row.get(3)?,
                    input_tokens: row.get::<_, i64>(4)? as u64,
                    output_tokens: row.get::<_, i64>(5)? as u64,
                    cache_read_input_tokens: row.get::<_, i64>(6)? as u64,
                    cache_creation_input_tokens: row.get::<_, i64>(7)? as u64,
                    coalesced_count: row.get::<_, i64>(8)? as u64,
                    latency_ms: row.get::<_, i64>(9)? as u64,
                    status: row.get(10)?,
                    cost: row.get(11)?,
                    project_path: row.get(12)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> LedgerDb {
        LedgerDb::open(Path::new(":memory:")).expect("in-memory db")
    }

    #[test]
    fn test_record_and_retrieve() {
        let db = test_db();
        let record = NewLedgerRecord {
            timestamp: Utc::now(),
            model: "claude-sonnet-5".into(),
            profile_name: "default".into(),
            input_tokens: 1000,
            output_tokens: 200,
            cache_read_input_tokens: 500,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 350,
            status: "success".into(),
            cost: Some(0.0042),
            project_path: "/home/user/project".into(),
        };
        let id = db.record(&record).unwrap();
        assert!(id > 0);

        let entries = db.get_entries(10, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].model, "claude-sonnet-5");
        assert_eq!(entries[0].input_tokens, 1000);
        assert_eq!(entries[0].cost, Some(0.0042));
    }

    #[test]
    fn test_summary_aggregation() {
        let db = test_db();
        db.record(&NewLedgerRecord {
            timestamp: Utc::now(),
            model: "claude-sonnet-5".into(),
            profile_name: "default".into(),
            input_tokens: 1000,
            output_tokens: 200,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 300,
            status: "success".into(),
            cost: Some(0.003),
            project_path: "/tmp/test".into(),
        })
        .unwrap();
        db.record(&NewLedgerRecord {
            timestamp: Utc::now(),
            model: "unknown-model".into(),
            profile_name: "default".into(),
            input_tokens: 500,
            output_tokens: 100,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 200,
            status: "error".into(),
            cost: None,
            project_path: "/tmp/test".into(),
        })
        .unwrap();

        let summary = db.get_summary(None).unwrap();
        assert_eq!(summary.total.total_requests, 2);
        assert_eq!(summary.total.input_tokens, 1500);
        assert_eq!(summary.total.output_tokens, 300);
        assert_eq!(summary.total.total_cost_known, 0.003);
        assert_eq!(summary.total.total_cost_unknown_requests, 1);
        assert_eq!(summary.by_model.len(), 2);
    }

    #[test]
    fn test_unknown_cost_counts_as_unknown() {
        let db = test_db();
        db.record(&NewLedgerRecord {
            timestamp: Utc::now(),
            model: "no-pricing-model".into(),
            profile_name: "default".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 100,
            status: "success".into(),
            cost: None,
            project_path: "/tmp".into(),
        })
        .unwrap();

        let summary = db.get_summary(None).unwrap();
        assert_eq!(summary.total.total_cost_known, 0.0);
        assert_eq!(summary.total.total_cost_unknown_requests, 1);
    }
}

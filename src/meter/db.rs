use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

use crate::meter::types::{
    DayBreakdown, LedgerEntry, ModelBreakdown, StatsSummary, UsageBreakdown,
};

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
    pub reduction_input_tokens: u64,
    pub reduction_output_tokens: u64,
    pub reduction_count: u64,
    pub efficiency_mode: String,
    pub local_cache_hit: bool,
    pub runtime_id: String,
    pub request_id: String,
    pub integration_id: String,
    pub upstream_id: String,
    pub trust_domain_id: String,
    pub config_snapshot_hash: String,
    pub attribution: String,
    pub protocol: String,
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

        // Integrity check before any operations
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap_or_else(|_| "failed".into());
        if integrity != "ok" {
            anyhow::bail!("Database integrity check failed: {}", integrity);
        }

        // Schema version tracking
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

        // Version 1: base ledger table (original columns only)
        if current_version < 1 {
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
                    latency_ms INTEGER NOT NULL DEFAULT 0,
                    status TEXT NOT NULL DEFAULT 'success',
                    cost REAL,
                    project_path TEXT NOT NULL DEFAULT ''
                )",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
        }

        // Versions 2–7: column additions over time (applied only for DBs that
        // predate each column).
        if current_version < 2 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN coalesced_count INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])?;
        }
        if current_version < 3 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN reduction_input_tokens INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (3)", [])?;
        }
        if current_version < 4 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN reduction_output_tokens INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (4)", [])?;
        }
        if current_version < 5 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN reduction_count INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (5)", [])?;
        }
        if current_version < 6 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN efficiency_mode TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (6)", [])?;
        }
        if current_version < 7 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN local_cache_hit INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (7)", [])?;
        }

        // Version 10: identity columns (runtime_id, request_id, integration_id,
        // upstream_id, trust_domain_id, config_snapshot_hash, attribution)
        if current_version < 10 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN runtime_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN request_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN integration_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN upstream_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN trust_domain_id TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN config_snapshot_hash TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN attribution TEXT NOT NULL DEFAULT 'unknown'",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (10)", [])?;
        }

        // Version 11: protocol column for multi-protocol reporting (M11)
        if current_version < 11 {
            conn.execute(
                "ALTER TABLE ledger ADD COLUMN protocol TEXT NOT NULL DEFAULT ''",
                [],
            )?;
            conn.execute("INSERT INTO schema_version (version) VALUES (11)", [])?;
        }

        // Indexes (safe to recreate)
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ledger_timestamp ON ledger(timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ledger_project ON ledger(project_path, timestamp)",
            [],
        )?;

        // cache_rejects may also be created by CacheDb; ensure it exists here
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

        Ok(Self { conn })
    }

    pub fn record(&self, entry: &NewLedgerRecord) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO ledger (timestamp, model, profile_name, input_tokens, output_tokens,
             cache_read_input_tokens, cache_creation_input_tokens, coalesced_count, latency_ms, status, cost, project_path,
             reduction_input_tokens, reduction_output_tokens, reduction_count, efficiency_mode, local_cache_hit,
             runtime_id, request_id, integration_id, upstream_id, trust_domain_id, config_snapshot_hash, attribution, protocol)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
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
                entry.reduction_input_tokens as i64,
                entry.reduction_output_tokens as i64,
                entry.reduction_count as i64,
                entry.efficiency_mode,
                entry.local_cache_hit as i64,
                entry.runtime_id,
                entry.request_id,
                entry.integration_id,
                entry.upstream_id,
                entry.trust_domain_id,
                entry.config_snapshot_hash,
                entry.attribution,
                entry.protocol,
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
        let rows_sql = "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_input_tokens), 0), COALESCE(SUM(cache_creation_input_tokens), 0),
                COALESCE(SUM(coalesced_count), 0),
                COALESCE(AVG(latency_ms), 0),
                COALESCE(SUM(CASE WHEN cost IS NOT NULL THEN cost ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN cost IS NULL THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(reduction_input_tokens), 0),
                COALESCE(SUM(reduction_output_tokens), 0),
                COALESCE(SUM(reduction_count), 0),
                COALESCE(SUM(local_cache_hit), 0),
                COALESCE(SUM(CASE WHEN local_cache_hit = 0 AND coalesced_count = 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN local_cache_hit = 1 THEN input_tokens + output_tokens ELSE 0 END), 0),
                COALESCE(AVG(CASE WHEN local_cache_hit = 1 THEN latency_ms ELSE NULL END), 0),
                COALESCE(AVG(CASE WHEN local_cache_hit = 0 THEN latency_ms ELSE NULL END), 0)
         FROM ledger
         WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)";

        let row_to_breakdown = |row: &rusqlite::Row| -> rusqlite::Result<UsageBreakdown> {
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
                reduction_input_tokens: row.get::<_, i64>(9)? as u64,
                reduction_output_tokens: row.get::<_, i64>(10)? as u64,
                reduction_count: row.get::<_, i64>(11)? as u64,
                efficiency_mode: String::new(),
                local_cache_hit_count: row.get::<_, i64>(12)? as u64,
                upstream_requests: row.get::<_, i64>(13)? as u64,
                local_hit_tokens_saved: row.get::<_, i64>(14)? as u64,
                invalidated_cache_candidates: 0,
                local_hit_avg_latency_ms: row.get::<_, f64>(15)?,
                upstream_avg_latency_ms: row.get::<_, f64>(16)?,
            })
        };

        if model_filter.is_empty() {
            let result = self.conn.query_row(
                rows_sql,
                rusqlite::params![project_exact, project_glob],
                row_to_breakdown,
            )?;
            Ok(result)
        } else {
            let sql = format!("{rows_sql} AND model = ?3");
            let result = self.conn.query_row(
                &sql,
                rusqlite::params![project_exact, project_glob, model_filter],
                row_to_breakdown,
            )?;
            Ok(result)
        }
    }

    pub fn get_summary(&self, project_path: Option<&str>) -> Result<StatsSummary> {
        let (exact, glob) = Self::project_filter(project_path);

        let mut total = self.build_breakdown("", &exact, &glob)?;

        // Query cache_rejects for invalidated count
        let rejected_sql = match &exact {
            Some(_) => {
                "SELECT COUNT(*) FROM cache_rejects WHERE project_path = ?1 OR project_path GLOB ?2"
            }
            None => "SELECT COUNT(*) FROM cache_rejects",
        };
        let rejected: i64 = match &exact {
            Some(_) => self
                .conn
                .query_row(rejected_sql, rusqlite::params![exact, glob], |row| {
                    row.get(0)
                })
                .map_err(|e| tracing::warn!("Failed to query cache_rejects: {e}"))
                .unwrap_or(0),
            None => self
                .conn
                .query_row(rejected_sql, [], |row| row.get(0))
                .map_err(|e| tracing::warn!("Failed to query cache_rejects: {e}"))
                .unwrap_or(0),
        };
        total.invalidated_cache_candidates = rejected as u64;

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
                    Ok(ModelBreakdown {
                        model,
                        breakdown: b,
                    })
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
                        COALESCE(SUM(CASE WHEN cost IS NULL THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(reduction_input_tokens), 0),
                        COALESCE(SUM(reduction_output_tokens), 0),
                        COALESCE(SUM(reduction_count), 0),
                        COALESCE(SUM(local_cache_hit), 0),
                        COALESCE(SUM(CASE WHEN local_cache_hit = 0 AND coalesced_count = 0 THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN local_cache_hit = 1 THEN input_tokens + output_tokens ELSE 0 END), 0),
                        COALESCE(AVG(CASE WHEN local_cache_hit = 1 THEN latency_ms ELSE NULL END), 0),
                        COALESCE(AVG(CASE WHEN local_cache_hit = 0 THEN latency_ms ELSE NULL END), 0)
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
                        reduction_input_tokens: row.get::<_, i64>(10)? as u64,
                        reduction_output_tokens: row.get::<_, i64>(11)? as u64,
                        reduction_count: row.get::<_, i64>(12)? as u64,
                        efficiency_mode: String::new(),
                        local_cache_hit_count: row.get::<_, i64>(13)? as u64,
                        upstream_requests: row.get::<_, i64>(14)? as u64,
                        local_hit_tokens_saved: row.get::<_, i64>(15)? as u64,
                        invalidated_cache_candidates: 0,
                        local_hit_avg_latency_ms: row.get::<_, f64>(16)?,
                        upstream_avg_latency_ms: row.get::<_, f64>(17)?,
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
                    cache_read_input_tokens, cache_creation_input_tokens, coalesced_count, latency_ms, status, cost, project_path,
                    reduction_input_tokens, reduction_output_tokens, reduction_count, efficiency_mode, local_cache_hit,
                    runtime_id, request_id, integration_id, upstream_id, trust_domain_id, config_snapshot_hash, attribution, protocol
             FROM ledger
             WHERE (?1 IS NULL OR project_path = ?1 OR project_path GLOB ?2)
             ORDER BY timestamp DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![exact, glob, limit as i64], |row| {
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
                reduction_input_tokens: row.get::<_, i64>(13)? as u64,
                reduction_output_tokens: row.get::<_, i64>(14)? as u64,
                reduction_count: row.get::<_, i64>(15)? as u64,
                efficiency_mode: row.get(16)?,
                local_cache_hit: row.get::<_, i64>(17)? != 0,
                runtime_id: row.get(18)?,
                request_id: row.get(19)?,
                integration_id: row.get(20)?,
                upstream_id: row.get(21)?,
                trust_domain_id: row.get(22)?,
                config_snapshot_hash: row.get(23)?,
                attribution: row.get(24)?,
                protocol: row.get(25)?,
            })
        })?;
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
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit: false,
            runtime_id: "test-runtime".into(),
            request_id: "test-request-001".into(),
            integration_id: "abc123".into(),
            upstream_id: "xyz789".into(),
            trust_domain_id: "deadbeef1234abcd".into(),
            config_snapshot_hash: "abcdef01".into(),
            attribution: "unknown".into(),
            protocol: "anthropic".into(),
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
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit: false,
            runtime_id: "test-runtime".into(),
            request_id: "test-req-1".into(),
            integration_id: "abc123".into(),
            upstream_id: "xyz789".into(),
            trust_domain_id: "deadbeef1234abcd".into(),
            config_snapshot_hash: "abcdef01".into(),
            attribution: "unknown".into(),
            protocol: "anthropic".into(),
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
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit: false,
            runtime_id: "test-runtime".into(),
            request_id: "test-req-2".into(),
            integration_id: "abc123".into(),
            upstream_id: "xyz789".into(),
            trust_domain_id: "deadbeef1234abcd".into(),
            config_snapshot_hash: "abcdef01".into(),
            attribution: "unknown".into(),
            protocol: "anthropic".into(),
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
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit: false,
            runtime_id: "test-runtime".into(),
            request_id: "test-req-3".into(),
            integration_id: "abc123".into(),
            upstream_id: "xyz789".into(),
            trust_domain_id: "deadbeef1234abcd".into(),
            config_snapshot_hash: "abcdef01".into(),
            attribution: "unknown".into(),
            protocol: "anthropic".into(),
        })
        .unwrap();

        let summary = db.get_summary(None).unwrap();
        assert_eq!(summary.total.total_cost_known, 0.0);
        assert_eq!(summary.total.total_cost_unknown_requests, 1);
    }
}

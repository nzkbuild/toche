use std::time::Instant;

use anyhow::Result;

use crate::meter::db::{LedgerDb, NewLedgerRecord};
use crate::meter::pricing::PricingMap;

/// Lightweight timer for measuring request latency.
pub struct RequestTimer {
    start: Instant,
}

impl RequestTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// Get the canonical project path for the current working directory.
pub fn current_project_path() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Estimate token count from text using ~4 chars = 1 token heuristic.
/// Adapted from RTK tracking.rs `estimate_tokens`.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as f64 / 4.0).ceil() as u64
}

/// Record a request to the ledger. Resolves cost from the pricing map
/// and inserts into the database. Returns the row ID.
///
/// If pricing is unknown for the model, `cost` is set to NULL in the ledger
/// (the `total_cost_unknown_requests` counter will reflect this).
pub fn record_request(db: &LedgerDb, pricing: &PricingMap, record: NewLedgerRecord) -> Result<i64> {
    let cost = pricing.find(&record.model).map(|p| {
        let input_cost = (record.input_tokens as f64) * p.input;
        let output_cost = (record.output_tokens as f64) * p.output;
        let cache_read_cost = (record.cache_read_input_tokens as f64) * p.cache_read;
        let cache_create_cost = (record.cache_creation_input_tokens as f64) * p.cache_create;
        input_cost + output_cost + cache_read_cost + cache_create_cost
    });

    let mut entry = NewLedgerRecord { cost, ..record };

    // Truncate cost to reasonable precision
    if let Some(ref mut c) = entry.cost {
        *c = (*c * 10_000_000.0).round() / 10_000_000.0;
    }

    db.record(&entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::db::LedgerDb;
    use chrono::Utc;
    use std::path::Path;

    fn test_db() -> LedgerDb {
        LedgerDb::open(Path::new(":memory:")).expect("in-memory db")
    }
    fn test_pricing() -> PricingMap {
        PricingMap::load_embedded()
    }

    #[test]
    fn test_request_timer() {
        let timer = RequestTimer::start();
        let elapsed = timer.elapsed_ms();
        assert!(elapsed < 10, "elapsed {elapsed}ms but expected <10ms");
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("hello world"), 3);
    }

    #[test]
    fn test_record_with_known_pricing() {
        let db = test_db();
        let pricing = test_pricing();

        let record = NewLedgerRecord {
            timestamp: Utc::now(),
            model: "claude-sonnet-5".into(),
            profile_name: "default".into(),
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_input_tokens: 500_000,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 500,
            status: "success".into(),
            cost: None,
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
        };

        let _id = record_request(&db, &pricing, record).unwrap();
        let entries = db.get_entries(10, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].cost.is_some());
        let cost = entries[0].cost.unwrap();
        assert!(cost > 0.0, "cost should be positive, got {cost}");
    }

    #[test]
    fn test_record_with_unknown_pricing_has_null_cost() {
        let db = test_db();
        let pricing = test_pricing();

        let record = NewLedgerRecord {
            timestamp: Utc::now(),
            model: "nonexistent-model".into(),
            profile_name: "default".into(),
            input_tokens: 1_000,
            output_tokens: 100,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            latency_ms: 100,
            status: "success".into(),
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
        };

        let _id = record_request(&db, &pricing, record).unwrap();
        let entries = db.get_entries(10, None).unwrap();
        assert_eq!(entries[0].cost, None);
    }

    #[test]
    fn test_current_project_path_is_string() {
        let path = current_project_path();
        assert!(!path.is_empty());
    }
}

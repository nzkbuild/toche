use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LedgerEntry {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
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

#[derive(Debug, Clone, Serialize)]
pub struct UsageBreakdown {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub coalesced_count: u64,
    pub total_requests: u64,
    pub total_cost_known: f64,
    pub total_cost_unknown_requests: u64,
    pub avg_latency_ms: f64,
    pub reduction_input_tokens: u64,
    pub reduction_output_tokens: u64,
    pub reduction_count: u64,
    pub efficiency_mode: String,
    pub local_cache_hit_count: u64,
    pub upstream_requests: u64,
    pub local_hit_tokens_saved: u64,
    pub invalidated_cache_candidates: u64,
    pub local_hit_avg_latency_ms: f64,
    pub upstream_avg_latency_ms: f64,
}
impl Default for UsageBreakdown {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            coalesced_count: 0,
            total_requests: 0,
            total_cost_known: 0.0,
            total_cost_unknown_requests: 0,
            avg_latency_ms: 0.0,
            reduction_input_tokens: 0,
            reduction_output_tokens: 0,
            reduction_count: 0,
            efficiency_mode: String::new(),
            local_cache_hit_count: 0,
            upstream_requests: 0,
            local_hit_tokens_saved: 0,
            invalidated_cache_candidates: 0,
            local_hit_avg_latency_ms: 0.0,
            upstream_avg_latency_ms: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSummary {
    pub total: UsageBreakdown,
    pub by_model: Vec<ModelBreakdown>,
    pub by_day: Vec<DayBreakdown>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelBreakdown {
    pub model: String,
    pub breakdown: UsageBreakdown,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayBreakdown {
    pub date: String,
    pub breakdown: UsageBreakdown,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsOutput {
    pub summary: StatsSummary,
    pub entries: Vec<LedgerEntry>,
}

/// Schema-versioned JSON stats output (M11).
#[derive(Debug, Clone, Serialize)]
pub struct StatsOutputV1 {
    pub schema_version: String,
    pub summary: StatsSummary,
    pub entries: Vec<LedgerEntry>,
    pub by_protocol: Vec<ProtocolBreakdown>,
    pub by_integration: Vec<IntegrationBreakdown>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProtocolBreakdown {
    pub protocol: String,
    pub breakdown: UsageBreakdown,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationBreakdown {
    pub integration: String,
    pub breakdown: UsageBreakdown,
}

/// Measurement confidence per M11 requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum MeasurementConfidence {
    /// Token counts derived from actual HTTP body measurement.
    Measured,
    /// Values reported by the provider in response headers.
    ProviderReported,
    /// Values estimated from byte/character counts.
    Estimated,
    /// Values from configuration (pricing, expected model).
    Configured,
    /// Value could not be determined.
    Unknown,
}

impl std::fmt::Display for MeasurementConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeasurementConfidence::Measured => f.write_str("measured"),
            MeasurementConfidence::ProviderReported => f.write_str("provider-reported"),
            MeasurementConfidence::Estimated => f.write_str("estimated"),
            MeasurementConfidence::Configured => f.write_str("configured"),
            MeasurementConfidence::Unknown => f.write_str("unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_breakdown_defaults() {
        let b = UsageBreakdown::default();
        assert_eq!(b.input_tokens, 0);
        assert_eq!(b.output_tokens, 0);
        assert_eq!(b.total_requests, 0);
        assert_eq!(b.total_cost_known, 0.0);
        assert_eq!(b.total_cost_unknown_requests, 0);
    }

    #[test]
    fn test_stats_output_serialization() {
        let output = StatsOutput {
            summary: StatsSummary {
                total: UsageBreakdown::default(),
                by_model: vec![],
                by_day: vec![],
            },
            entries: vec![],
        };
        let json = serde_json::to_string_pretty(&output).unwrap();
        assert!(json.contains("summary"));
        assert!(json.contains("entries"));
    }
}

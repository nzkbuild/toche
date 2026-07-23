use anyhow::Context;

use crate::config::loader::load_config;
use crate::meter::db::LedgerDb;
use crate::meter::types::{MeasurementConfidence, StatsOutput, StatsOutputV1};
use crate::profiles::loader::config_dir;

/// Schema version embedded in all JSON output.
pub const STATS_JSON_SCHEMA_VERSION: &str = "1.0.0";

fn classify_confidence(entry: &crate::meter::types::LedgerEntry) -> MeasurementConfidence {
    if entry.input_tokens > 0 || entry.output_tokens > 0 {
        if entry.model != "unknown" && entry.cost.is_some() {
            MeasurementConfidence::Measured
        } else {
            MeasurementConfidence::ProviderReported
        }
    } else if entry.cost.is_some() {
        MeasurementConfidence::Configured
    } else {
        MeasurementConfidence::Unknown
    }
}

pub async fn run(
    json: bool,
    entries: u32,
    protocol_filter: Option<&str>,
    integration_filter: Option<&str>,
    trust_domain_filter: Option<&str>,
) -> anyhow::Result<()> {
    let config = load_config().context("Failed to load configuration")?;
    let (db_path, _) = config.storage.resolve_paths(&config_dir());
    let db = LedgerDb::open(&db_path)
        .with_context(|| format!("Failed to open ledger at {}", db_path.display()))?;

    let summary = db
        .get_summary(None)
        .context("Failed to query ledger summary")?;
    let recent = db
        .get_entries(entries, None)
        .context("Failed to query ledger entries")?;

    // Filter entries by protocol, integration, trust domain
    let filtered: Vec<_> = recent
        .into_iter()
        .filter(|e| {
            if let Some(p) = protocol_filter {
                if e.protocol != p {
                    return false;
                }
            }
            if let Some(i) = integration_filter {
                if e.profile_name != i {
                    return false;
                }
            }
            if let Some(td) = trust_domain_filter {
                if e.trust_domain_id != td {
                    return false;
                }
            }
            true
        })
        .collect();

    // Aggregate by protocol
    let mut by_protocol: Vec<crate::meter::types::ProtocolBreakdown> = Vec::new();
    {
        let mut groups: std::collections::HashMap<String, crate::meter::types::UsageBreakdown> =
            std::collections::HashMap::new();
        for e in &filtered {
            let b = groups.entry(e.protocol.clone()).or_default();
            b.total_requests += 1;
            b.input_tokens += e.input_tokens;
            b.output_tokens += e.output_tokens;
            b.cache_read_input_tokens += e.cache_read_input_tokens;
            b.cache_creation_input_tokens += e.cache_creation_input_tokens;
            b.coalesced_count += e.coalesced_count;
            b.upstream_requests += if e.local_cache_hit { 0 } else { 1 };
            if e.cost.is_some() {
                b.total_cost_known += e.cost.unwrap_or(0.0);
            } else {
                b.total_cost_unknown_requests += 1;
            }
            b.reduction_input_tokens += e.reduction_input_tokens;
            b.reduction_output_tokens += e.reduction_output_tokens;
            b.reduction_count += e.reduction_count;
            if e.local_cache_hit {
                b.local_cache_hit_count += 1;
                b.local_hit_tokens_saved += e.input_tokens + e.output_tokens;
            }
        }
        for (proto, breakdown) in groups.into_iter() {
            by_protocol.push(crate::meter::types::ProtocolBreakdown {
                protocol: proto,
                breakdown,
            });
        }
    }

    // Aggregate by integration
    let mut by_integration: Vec<crate::meter::types::IntegrationBreakdown> = Vec::new();
    {
        let mut groups: std::collections::HashMap<String, crate::meter::types::UsageBreakdown> =
            std::collections::HashMap::new();
        for e in &filtered {
            let b = groups.entry(e.profile_name.clone()).or_default();
            b.total_requests += 1;
            b.input_tokens += e.input_tokens;
            b.output_tokens += e.output_tokens;
            b.cache_read_input_tokens += e.cache_read_input_tokens;
            b.cache_creation_input_tokens += e.cache_creation_input_tokens;
            b.coalesced_count += e.coalesced_count;
            b.upstream_requests += if e.local_cache_hit { 0 } else { 1 };
            if e.cost.is_some() {
                b.total_cost_known += e.cost.unwrap_or(0.0);
            } else {
                b.total_cost_unknown_requests += 1;
            }
            b.reduction_input_tokens += e.reduction_input_tokens;
            b.reduction_output_tokens += e.reduction_output_tokens;
            b.reduction_count += e.reduction_count;
            if e.local_cache_hit {
                b.local_cache_hit_count += 1;
                b.local_hit_tokens_saved += e.input_tokens + e.output_tokens;
            }
        }
        for (integration, breakdown) in groups.into_iter() {
            by_integration.push(crate::meter::types::IntegrationBreakdown {
                integration,
                breakdown,
            });
        }
    }

    let output = StatsOutput {
        summary,
        entries: filtered.clone(),
    };

    if json {
        let v1 = StatsOutputV1 {
            schema_version: STATS_JSON_SCHEMA_VERSION.to_string(),
            summary: output.summary,
            entries: filtered.clone(),
            by_protocol,
            by_integration,
        };

        let json_str =
            serde_json::to_string_pretty(&v1).context("Failed to serialize stats to JSON")?;
        println!("{json_str}");
    } else {
        println!("Toche Usage Stats");
        println!("=================");
        println!();
        let t = &output.summary.total;
        println!("Total requests:      {}", t.total_requests);
        println!("Upstream requests:   {}", t.upstream_requests);
        println!("Coalesced requests:  {}", t.coalesced_count);
        println!("Local cache hits:    {}", t.local_cache_hit_count);
        if t.invalidated_cache_candidates > 0 {
            println!(
                "Cache candidates rejected: {}",
                t.invalidated_cache_candidates
            );
        }
        if t.local_hit_tokens_saved > 0 {
            println!("Tokens saved (local hits): {}", t.local_hit_tokens_saved);
        }
        println!("Input tokens:        {}", t.input_tokens);
        println!("Output tokens:       {}", t.output_tokens);
        println!("Cache read tokens:   {}", t.cache_read_input_tokens);
        println!("Cache create tokens: {}", t.cache_creation_input_tokens);
        if t.reduction_count > 0 {
            let saved = if t.reduction_input_tokens > 0 {
                let pct = (t.reduction_input_tokens as f64 - t.reduction_output_tokens as f64)
                    / t.reduction_input_tokens as f64
                    * 100.0;
                format!(
                    "{} ({:.1}%)",
                    t.reduction_input_tokens - t.reduction_output_tokens,
                    pct
                )
            } else {
                "0".to_string()
            };
            println!("Reduction:");
            println!("  Tool outputs reduced: {}", t.reduction_count);
            println!("  Raw tokens:           {}", t.reduction_input_tokens);
            println!("  Reduced tokens:       {}", t.reduction_output_tokens);
            println!("  Tokens saved:         {}", saved);
        }
        if t.local_hit_avg_latency_ms > 0.0 {
            println!("Avg latency (local):   {:.0}ms", t.local_hit_avg_latency_ms);
        }
        if t.upstream_avg_latency_ms > 0.0 {
            println!("Avg latency (upstream):{:.0}ms", t.upstream_avg_latency_ms);
        }
        println!("Avg latency:           {:.0}ms", t.avg_latency_ms);
        println!("Known cost:          ${:.6}", t.total_cost_known);
        if t.total_cost_unknown_requests > 0 {
            println!(
                "Unknown-cost reqs:   {} (no pricing for model)",
                t.total_cost_unknown_requests
            );
        }
        println!();

        if !by_protocol.is_empty() {
            println!("By Protocol:");
            for p in &by_protocol {
                println!(
                    "  {:25} {:>6} reqs  {:>10} in  {:>10} out  ${:.6}",
                    p.protocol,
                    p.breakdown.total_requests,
                    p.breakdown.input_tokens,
                    p.breakdown.output_tokens,
                    p.breakdown.total_cost_known
                );
            }
            println!();
        }

        if !by_integration.is_empty() {
            println!("By Integration:");
            for i in &by_integration {
                println!(
                    "  {:25} {:>6} reqs  {:>10} in  {:>10} out  ${:.6}",
                    i.integration,
                    i.breakdown.total_requests,
                    i.breakdown.input_tokens,
                    i.breakdown.output_tokens,
                    i.breakdown.total_cost_known
                );
            }
            println!();
        }

        if !output.summary.by_model.is_empty() {
            println!("By Model:");
            for m in &output.summary.by_model {
                println!(
                    "  {:30} {:>6} reqs  {:>10} in  {:>10} out  ${:.6}",
                    m.model,
                    m.breakdown.total_requests,
                    m.breakdown.input_tokens,
                    m.breakdown.output_tokens,
                    m.breakdown.total_cost_known
                );
            }
            println!();
        }

        if !output.entries.is_empty() {
            println!("Recent Requests:");
            for e in &output.entries {
                let cost_str = e
                    .cost
                    .map(|c| format!("${c:.6}"))
                    .unwrap_or_else(|| "unknown".to_string());
                let proto = if e.protocol.is_empty() {
                    "?"
                } else {
                    &e.protocol
                };
                let conf = classify_confidence(e);
                println!(
                    "  {}  {:12}  {:25}  {:>6} in {:>6} out  {:>4}ms  {}  {}  {}",
                    e.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    proto,
                    e.model,
                    e.input_tokens,
                    e.output_tokens,
                    e.latency_ms,
                    e.status,
                    cost_str,
                    conf,
                );
            }
        }
    }

    Ok(())
}

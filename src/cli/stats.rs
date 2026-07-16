use anyhow::Context;

use crate::meter::db::LedgerDb;
use crate::meter::types::StatsOutput;
use crate::profiles::loader::config_dir;

pub async fn run(json: bool, entries: u32) -> anyhow::Result<()> {
    let db_path = config_dir().join("ledger.db");
    let db = LedgerDb::open(&db_path)
        .with_context(|| format!("Failed to open ledger at {}", db_path.display()))?;

    let summary = db
        .get_summary(None)
        .context("Failed to query ledger summary")?;
    let recent = db
        .get_entries(entries, None)
        .context("Failed to query ledger entries")?;

    let output = StatsOutput {
        summary,
        entries: recent,
    };

    if json {
        let json_str =
            serde_json::to_string_pretty(&output).context("Failed to serialize stats to JSON")?;
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
                let cache_mark = if e.local_cache_hit { " [CACHE]" } else { "" };
                println!(
                    "  {}  {:30}  {:>6} in {:>6} out  {:>4}ms  {}  {}{}",
                    e.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    e.model,
                    e.input_tokens,
                    e.output_tokens,
                    e.latency_ms,
                    e.status,
                    cost_str,
                    cache_mark,
                );
            }
        }
    }

    Ok(())
}

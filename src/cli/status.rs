use crate::config::loader::load_config;

pub async fn run(json: bool) -> anyhow::Result<()> {
    // Try to reach the live /status endpoint
    let live_status = reqwest::get("http://127.0.0.1:8743/status").await;

    if let Ok(resp) = live_status {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
                    );
                    return Ok(());
                }

                println!("Toche Status (live)");
                println!("===================");
                println!();
                println!(
                    "Runtime ID:   {}",
                    body.get("runtime_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                );
                println!(
                    "Config hash:  {}",
                    body.get("config_snapshot_hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                );
                println!(
                    "Port:         {}",
                    body.get("port").and_then(|v| v.as_u64()).unwrap_or(0)
                );
                println!(
                    "Active flights: {}",
                    body.get("active_flights")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                );

                if let Some(flights) = body.get("flight_details").and_then(|v| v.as_array()) {
                    if !flights.is_empty() {
                        println!();
                        println!("Flight details:");
                        for f in flights {
                            let url = f
                                .get("upstream_url")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?");
                            let domain = f
                                .get("trust_domain_hash")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?");
                            let waiters =
                                f.get("waiter_count").and_then(|v| v.as_u64()).unwrap_or(0);
                            println!("  {url} (domain={domain}, waiters={waiters})");
                        }
                    }
                }

                if let Some(proto) = body.get("protocol_counts").and_then(|v| v.as_object()) {
                    if !proto.is_empty() {
                        println!();
                        println!("Protocol counts (from ledger):");
                        for (k, v) in proto {
                            println!("  {k}: {v}");
                        }
                    }
                }

                if let Some(integrations) =
                    body.get("integration_counts").and_then(|v| v.as_object())
                {
                    if !integrations.is_empty() {
                        println!();
                        println!("Integration counts (from ledger):");
                        for (k, v) in integrations {
                            println!("  {k}: {v}");
                        }
                    }
                }

                if let Some(degraded) = body.get("degraded_systems").and_then(|v| v.as_array()) {
                    if !degraded.is_empty() {
                        println!();
                        println!("Degraded systems: {}", degraded.len());
                    }
                }

                return Ok(());
            }
        }
    }

    // Fallback: gateway not running, show config summary
    if json {
        let config = load_config()?;
        let summary = serde_json::json!({
            "status": "offline",
            "integrations": config.integrations.iter().map(|i| serde_json::json!({
                "name": i.name,
                "id": i.id,
                "upstream_id": i.upstream,
            })).collect::<Vec<_>>(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_else(|_| summary.to_string())
        );
        return Ok(());
    }

    println!("Toche Status (offline)");
    println!("======================");
    println!("Gateway is not running.");
    println!();
    println!("Start with: toche");
    println!();

    match load_config() {
        Ok(config) => {
            let default_name = config
                .defaults
                .integration
                .and_then(|id| config.integrations.iter().find(|i| i.id == id))
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "none".into());
            println!("Default integration: {default_name}");
            for i in &config.integrations {
                let upstream = config
                    .upstreams
                    .iter()
                    .find(|u| u.id == i.upstream)
                    .map(|u| u.url.as_str())
                    .unwrap_or("unknown");
                println!("  {} -> {}", i.name, upstream);
            }
        }
        Err(e) => {
            println!("No configuration loaded: {e}");
        }
    }

    Ok(())
}

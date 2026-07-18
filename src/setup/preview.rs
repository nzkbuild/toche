use crate::config::toche_config::TocheConfig;
use std::path::Path;

/// Render a human-readable preview of the proposed configuration changes.
pub fn render(config: &TocheConfig, config_dir: &Path) -> String {
    let mut lines = Vec::new();
    lines.push("Setup preview:".to_string());
    lines.push(format!(
        "  Config path: {}",
        config_dir.join("config.toml").display()
    ));
    lines.push(format!("  Schema version: {}", config.schema_version));

    if let Some(default_id) = config.defaults.integration.as_deref() {
        let name = config
            .integrations
            .iter()
            .find(|i| i.id == *default_id)
            .map(|i| i.name.as_str())
            .unwrap_or("unknown");
        lines.push(format!("  Default integration: {name}"));
    }

    for upstream in &config.upstreams {
        lines.push(format!(
            "  Upstream '{}' -> {}",
            upstream.name, upstream.url
        ));
        lines.push(format!("    Auth header: {}", upstream.auth.header_name));
        lines.push(format!("    Secret ref: {}", upstream.auth.secret_ref));
    }

    for integration in &config.integrations {
        lines.push(format!(
            "  Integration '{}' ({})",
            integration.name, integration.id
        ));
    }

    for policy in &config.policies {
        lines.push(format!("  Policy '{}' ({})", policy.name, policy.id));
    }

    lines.join("\n")
}

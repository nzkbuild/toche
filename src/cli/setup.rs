use anyhow::Context;
use tracing::info;

use crate::config::loader::config_dir;
use crate::config::migration::migrate_v1_to_v2;
use crate::config::utils::atomic_write_secure;
use crate::profiles::types::{AuthMethod, Profile, Profiles};

pub async fn run(force: bool) -> anyhow::Result<()> {
    info!("Starting Toche setup...");

    let dir = config_dir();
    std::fs::create_dir_all(&dir).context("Failed to create config directory")?;

    let config_path = dir.join("config.toml");

    // Guard against silently destroying existing user configuration.
    if config_path.exists() && !force {
        println!("config.toml already exists at {}", config_path.display());
        println!();
        println!("Running setup again would overwrite your custom configuration.");
        println!(
            "If you want to regenerate the default config, use --force to overwrite (a backup will be created)."
        );
        return Ok(());
    }

    if config_path.exists() && force {
        let bak_path = dir.join("config.toml.bak");
        std::fs::copy(&config_path, &bak_path).context("Failed to backup existing config.toml")?;
        println!("Existing config.toml backed up to {}", bak_path.display());
    }

    // Detect existing Claude Code gateway configuration
    let claude_settings = detect_claude_config()?;

    let config = if let Some(settings) = claude_settings {
        let profile = import_from_claude_settings(&settings)?;
        println!("Detected Claude Code upstream: {}", profile.upstream_url);
        let profiles = Profiles {
            default: Some(profile.name.clone()),
            profiles: vec![profile],
        };
        migrate_v1_to_v2(&profiles)
    } else {
        println!("No existing Claude Code gateway found.");
        println!(
            "Create {}/config.toml to configure your upstream.",
            dir.display()
        );
        println!("Example:");
        println!("  [runtime]");
        println!("  port = 8743");
        println!("  listen_address = \"127.0.0.1\"");
        println!("  request_timeout_ms = 300000");
        println!();
        println!("  [[upstreams]]");
        println!("  id = \"7f8a3b2c\"");
        println!("  name = \"Anthropic\"");
        println!("  url = \"https://api.anthropic.com\"");
        println!("  auth.type = \"environment\"");
        println!("  auth.key = \"ANTHROPIC_API_KEY\"");
        println!("  auth.header_name = \"x-api-key\"");
        return Ok(());
    };

    let toml_str = toml::to_string_pretty(&config).context("Failed to serialize config")?;
    atomic_write_secure(&config_path, &toml_str)?;

    let default_name = config
        .defaults
        .integration
        .and_then(|id| config.integrations.iter().find(|i| i.id == id))
        .map(|i| i.name.clone())
        .unwrap_or_else(|| "default".into());
    println!(
        "Integration '{}' saved to {}",
        default_name,
        config_path.display()
    );
    println!("Run `toche connect` to point Claude Code to Toche.");

    detect_graphify();

    Ok(())
}

fn detect_graphify() {
    println!();
    let on_path = which::which("graphify").is_ok();
    let uv_path = crate::config::utils::home_dir()
        .join(".local")
        .join("bin")
        .join("graphify");

    if on_path || uv_path.exists() {
        println!(
            "Detected Graphify at: {}",
            which::which("graphify")
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| uv_path.display().to_string())
        );
        println!("Add [graphify] section to the integration in config.toml to configure:");
        println!("  [integrations.graphify]");
        println!("  enabled = true");
        println!("  # graph_path = \"path/to/graph.json\"  # if non-default");
        println!("  # auto_extract = false");
    } else {
        println!("Graphify not found.");
        println!("To enable knowledge graph queries, install Graphify:");
        println!("  uv tool install graphifyy");
        println!("  # or: pipx install graphifyy");
        println!("Then add a [graphify] section to the integration in config.toml.");
    }
}

fn detect_claude_config() -> anyhow::Result<Option<serde_json::Value>> {
    let path = crate::config::utils::home_dir()
        .join(".claude")
        .join("settings.json");
    if !path.exists() {
        return Ok(None);
    }
    let settings = crate::config::utils::read_jsonc(&path)
        .context("Failed to read Claude Code settings.json")?;
    Ok(Some(settings))
}

fn import_from_claude_settings(settings: &serde_json::Value) -> anyhow::Result<Profile> {
    let base_url = settings
        .get("baseURL")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.anthropic.com");

    let api_key = settings
        .get("apiKeyHelper")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches("env:").to_string());

    let auth_method = if let Some(key) = api_key {
        AuthMethod::ApiKey {
            header_name: "x-api-key".to_string(),
            key,
        }
    } else {
        AuthMethod::None
    };

    Ok(Profile {
        name: "default".to_string(),
        upstream_url: base_url.to_string(),
        auth_method,
        headers: Default::default(),
        models: Default::default(),
        cache: None,
        reduce: None,
        efficiency: None,
        safe_cache: None,
        graphify: None,
    })
}

use anyhow::Context;
use tracing::info;

use crate::config::utils::atomic_write;
use crate::profiles::loader::config_dir;
use crate::profiles::types::{AuthMethod, Profile, Profiles};

pub async fn run() -> anyhow::Result<()> {
    info!("Starting Toche setup...");

    let dir = config_dir();
    std::fs::create_dir_all(&dir).context("Failed to create config directory")?;

    // Detect existing Claude Code gateway configuration
    let claude_settings = detect_claude_config()?;

    let profiles = if let Some(settings) = claude_settings {
        let profile = import_from_claude_settings(&settings)?;
        println!(
            "Detected Claude Code upstream: {}",
            profile.upstream_url
        );
        Profiles {
            default: Some(profile.name.clone()),
            profiles: vec![profile],
        }
    } else {
        println!("No existing Claude Code gateway found.");
        println!("Create {}/profiles.toml to configure your upstream.", dir.display());
        println!("Example:");
        println!("  [[profiles]]");
        println!("  name = \"default\"");
        println!("  upstream_url = \"https://api.anthropic.com\"");
        println!("  auth_method.type = \"api_key\"");
        println!("  auth_method.header_name = \"x-api-key\"");
        println!("  auth_method.key = \"sk-ant-...\"");
        return Ok(());
    };

    let toml_str = toml::to_string_pretty(&profiles).context("Failed to serialize profiles")?;
    let path = dir.join("profiles.toml");
    atomic_write(&path, &toml_str)?;

    println!(
        "Profile '{}' saved to {}",
        profiles.default.as_deref().unwrap_or("default"),
        path.display()
    );
    println!("Run `toche connect` to point Claude Code to Toche.");
    Ok(())
}

fn detect_claude_config() -> anyhow::Result<Option<serde_json::Value>> {
    let path = dirs::home_dir()
        .unwrap()
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
    })
}

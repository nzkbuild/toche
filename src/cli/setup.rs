use anyhow::Context;
use tracing::info;

use crate::config::utils::atomic_write_secure;
use crate::profiles::loader::config_dir;
use crate::profiles::types::{AuthMethod, Profile, Profiles};

pub async fn run(force: bool) -> anyhow::Result<()> {
    info!("Starting Toche setup...");

    let dir = config_dir();
    std::fs::create_dir_all(&dir).context("Failed to create config directory")?;

    let profiles_path = dir.join("profiles.toml");

    // Guard against silently destroying existing user configuration.
    if profiles_path.exists() && !force {
        println!(
            "profiles.toml already exists at {}",
            profiles_path.display()
        );
        println!();
        println!("Running setup again would overwrite your custom profiles, API keys,");
        println!("and model configuration. If you want to regenerate the default config,");
        println!("use --force to overwrite (a backup will be created).");
        return Ok(());
    }

    if profiles_path.exists() && force {
        let bak_path = dir.join("profiles.toml.bak");
        std::fs::copy(&profiles_path, &bak_path)
            .context("Failed to backup existing profiles.toml")?;
        println!("Existing profiles.toml backed up to {}", bak_path.display());
    }

    // Detect existing Claude Code gateway configuration
    let claude_settings = detect_claude_config()?;

    let profiles = if let Some(settings) = claude_settings {
        let profile = import_from_claude_settings(&settings)?;
        println!("Detected Claude Code upstream: {}", profile.upstream_url);
        Profiles {
            default: Some(profile.name.clone()),
            profiles: vec![profile],
        }
    } else {
        println!("No existing Claude Code gateway found.");
        println!(
            "Create {}/profiles.toml to configure your upstream.",
            dir.display()
        );
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
    atomic_write_secure(&profiles_path, &toml_str)?;

    println!(
        "Profile '{}' saved to {}",
        profiles.default.as_deref().unwrap_or("default"),
        profiles_path.display()
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
        println!("Add [graphify] section to profiles.toml to configure:");
        println!("  [graphify]");
        println!("  enabled = true");
        println!("  # graph_path = \"path/to/graph.json\"  # if non-default");
        println!("  # auto_extract = false");
    } else {
        println!("Graphify not found.");
        println!("To enable knowledge graph queries, install Graphify:");
        println!("  uv tool install graphifyy");
        println!("  # or: pipx install graphifyy");
        println!("Then add a [graphify] section to your profile in profiles.toml.");
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

use crate::config::loader::config_dir;
use crate::integrations::points_to_toche;

pub async fn run() -> anyhow::Result<()> {
    println!("Toche Doctor");
    println!("============");
    println!();

    // Config directory
    let dir = config_dir();
    println!("Config directory: {}", dir.display());
    println!("  exists: {}", dir.exists());

    // Config files
    let config_path = dir.join("config.toml");
    let legacy_path = dir.join("profiles.toml");
    println!("Config file: {}", config_path.display());
    println!("  exists: {}", config_path.exists());
    println!("Legacy profiles file: {}", legacy_path.display());
    println!("  exists: {}", legacy_path.exists());

    match crate::config::loader::load_config() {
        Ok(config) => {
            let default_name = config
                .defaults
                .integration
                .and_then(|id| config.integrations.iter().find(|i| i.id == id))
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "none".into());
            println!("  default integration: {default_name}");
            for i in &config.integrations {
                let upstream = config
                    .upstreams
                    .iter()
                    .find(|u| u.id == i.upstream)
                    .map(|u| u.url.as_str())
                    .unwrap_or("unknown");
                println!("    {} -> {}", i.name, upstream);
            }
        }
        Err(e) => {
            println!("  error: {e}");
        }
    }

    // Claude Code integration
    let claude_dir = crate::config::utils::home_dir().join(".claude");
    println!("Claude Code directory: {}", claude_dir.display());
    println!("  exists: {}", claude_dir.exists());

    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists() {
        match crate::config::utils::read_jsonc(&settings_path) {
            Ok(settings) => {
                let base_url = settings
                    .get("baseURL")
                    .and_then(|v| v.as_str())
                    .unwrap_or("not set");
                let env_url = settings
                    .pointer("/env/ANTHROPIC_BASE_URL")
                    .and_then(|v| v.as_str())
                    .unwrap_or("not set");
                let pointing_to_toche = points_to_toche(&settings);
                println!("  baseURL: {base_url}");
                println!("  env.ANTHROPIC_BASE_URL: {env_url}");
                println!("  points to Toche: {pointing_to_toche}");
            }
            Err(e) => {
                println!("  error reading settings.json: {e}");
            }
        }
    } else {
        println!("  settings.json: not found");
    }

    // Backup exists?
    let backup_path = settings_path.with_extension("json.toche-backup");
    println!("Backup file: {}", backup_path.display());
    println!("  exists: {}", backup_path.exists());

    // Graphify
    println!();
    let graphify = which::which("graphify")
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "not installed".into());
    println!("Graphify: {}", graphify);

    Ok(())
}

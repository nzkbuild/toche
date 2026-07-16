use crate::profiles::loader::config_dir;

pub async fn run() -> anyhow::Result<()> {
    println!("Toche Doctor");
    println!("============");
    println!();

    // Config directory
    let dir = config_dir();
    println!("Config directory: {}", dir.display());
    println!("  exists: {}", dir.exists());

    // Profiles
    let profiles_path = dir.join("profiles.toml");
    println!("Profiles file: {}", profiles_path.display());
    println!("  exists: {}", profiles_path.exists());

    match crate::profiles::loader::load_profiles() {
        Ok(profiles) => {
            println!(
                "  default profile: {}",
                profiles.default.as_deref().unwrap_or("none")
            );
            for p in &profiles.profiles {
                println!("    {} -> {}", p.name, p.upstream_url);
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
                let points_to_toche = base_url.contains("127.0.0.1:8743");
                println!(
                    "  baseURL: {base_url} {}",
                    if points_to_toche {
                        "(points to Toche)"
                    } else {
                        ""
                    }
                );
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

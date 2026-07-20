use crate::config::utils::home_dir;

/// Detect Codex CLI installation and current configuration.
#[allow(dead_code)]
pub struct CodexDiscovery {
    /// Path to Codex config.toml.
    pub config_path: std::path::PathBuf,
    /// Whether the Codex home directory exists.
    pub codex_dir_exists: bool,
    /// Whether config.toml exists.
    pub config_exists: bool,
    /// Current upstream URL for the default provider, if detectable.
    pub current_upstream_url: Option<String>,
    /// Whether the current config already points to Toche.
    pub points_to_toche: bool,
    /// Whether the codex binary is on PATH.
    pub binary_installed: bool,
}

impl CodexDiscovery {
    #[allow(dead_code)]
    pub fn detect() -> Self {
        let codex_dir = codex_home();
        let codex_dir_exists = codex_dir.exists();
        let config_path = codex_dir.join("config.toml");
        let config_exists = config_path.exists();
        let binary_installed = which::which("codex").is_ok();

        let (current_upstream_url, points_to_toche) = if config_exists {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => {
                    let points_to_toche = content.contains("127.0.0.1:8743");
                    // Try to extract the current OpenAI base URL from config
                    let upstream = codex_upstream_url_from_toml(&content);
                    (upstream, points_to_toche)
                }
                Err(_) => (None, false),
            }
        } else {
            (None, false)
        };

        Self {
            config_path,
            codex_dir_exists,
            config_exists,
            current_upstream_url,
            points_to_toche,
            binary_installed,
        }
    }
}

/// Resolve the Codex home directory: $CODEX_HOME or ~/.codex.
pub fn codex_home() -> std::path::PathBuf {
    if let Ok(env_paths) = std::env::var("CODEX_HOME") {
        let first = env_paths.split(',').map(str::trim).next();
        if let Some(path) = first {
            if !path.is_empty() {
                return std::path::PathBuf::from(path);
            }
        }
    }
    home_dir().join(".codex")
}

/// Resolve the Codex config.toml path.
pub fn codex_config_path() -> std::path::PathBuf {
    codex_home().join("config.toml")
}

/// Public re-export for use by the config module.
pub fn codex_upstream_url_from_toml_public(content: &str) -> Option<String> {
    codex_upstream_url_from_toml(content)
}

/// Try to extract an OpenAI-compatible base URL from Codex TOML config.
/// Looks for `openai_base_url` or `base_url` keys.
fn codex_upstream_url_from_toml(content: &str) -> Option<String> {
    for line in content.lines() {
        let setting = line.split('#').next().unwrap_or_default().trim();
        if let Some((key, value)) = setting.split_once('=') {
            let key = key.trim();
            if key == "openai_base_url" || key == "base_url" {
                let value = value.trim().trim_matches(['"', '\''].as_ref());
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codex_upstream_url_from_toml_extracts_openai_base_url() {
        let content = r#"openai_base_url = "https://api.openai.com/v1""#;
        assert_eq!(
            codex_upstream_url_from_toml(content).as_deref(),
            Some("https://api.openai.com/v1")
        );
    }

    #[test]
    fn test_codex_upstream_url_from_toml_extracts_base_url() {
        let content = r#"base_url = "https://custom.api.com/v1""#;
        assert_eq!(
            codex_upstream_url_from_toml(content).as_deref(),
            Some("https://custom.api.com/v1")
        );
    }

    #[test]
    fn test_codex_upstream_url_from_toml_handles_comments() {
        let content = r#"openai_base_url = "https://api.openai.com/v1" # API endpoint"#;
        assert_eq!(
            codex_upstream_url_from_toml(content).as_deref(),
            Some("https://api.openai.com/v1")
        );
    }

    #[test]
    fn test_codex_upstream_url_from_toml_returns_none_when_missing() {
        let content = r#"service_tier = "fast""#;
        assert_eq!(codex_upstream_url_from_toml(content), None);
    }

    #[test]
    fn test_codex_upstream_url_from_toml_handles_single_quotes() {
        let content = "openai_base_url = 'https://api.openai.com/v1'";
        assert_eq!(
            codex_upstream_url_from_toml(content).as_deref(),
            Some("https://api.openai.com/v1")
        );
    }
}

use crate::config::utils::{home_dir, read_jsonc};

/// Detect Claude Code installation and current configuration.
#[allow(dead_code)]
pub struct ClaudeDiscovery {
    pub settings_path: std::path::PathBuf,
    pub settings_exist: bool,
    pub current_base_url: Option<String>,
    pub api_key_helper: Option<String>,
    pub points_to_toche: bool,
}

impl ClaudeDiscovery {
    #[allow(dead_code)]
    pub fn detect() -> Self {
        let settings_path = home_dir().join(".claude").join("settings.json");
        let settings_exist = settings_path.exists();

        let (current_base_url, api_key_helper, points_to_toche) = if settings_exist {
            match read_jsonc(&settings_path) {
                Ok(settings) => {
                    let base_url = settings
                        .get("baseURL")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let helper = settings
                        .get("apiKeyHelper")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let toche = super::super::points_to_toche(&settings);
                    (base_url, helper, toche)
                }
                Err(_) => (None, None, false),
            }
        } else {
            (None, None, false)
        };

        Self {
            settings_path,
            settings_exist,
            current_base_url,
            api_key_helper,
            points_to_toche,
        }
    }
}

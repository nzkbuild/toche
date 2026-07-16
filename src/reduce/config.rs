//! Per-profile configuration for the Safe Context Reduction pipeline.

use serde::{Deserialize, Serialize};

/// Controls the reduction pipeline per profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReduceConfig {
    /// Master switch: when false, reduction is disabled for this profile.
    #[serde(default = "default_reduce_enabled")]
    pub enabled: bool,
    /// Commands whose tool output should never be reduced (exact name match).
    #[serde(default)]
    pub command_bypass: Vec<String>,
}

fn default_reduce_enabled() -> bool {
    true
}

impl Default for ReduceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            command_bypass: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enabled() {
        let cfg = ReduceConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.command_bypass.is_empty());
    }

    #[test]
    fn deserialize_minimal() {
        let cfg: ReduceConfig = toml::from_str("").unwrap_or_default();
        assert!(cfg.enabled);
    }

    #[test]
    fn deserialize_bypass_list() {
        let cfg: ReduceConfig = toml::from_str(
            r#"
enabled = true
command_bypass = ["kubectl", "docker"]
"#,
        )
        .unwrap();
        assert_eq!(cfg.command_bypass, vec!["kubectl", "docker"]);
    }

    #[test]
    fn deserialize_disabled() {
        let cfg: ReduceConfig = toml::from_str("enabled = false").unwrap();
        assert!(!cfg.enabled);
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EfficiencyMode {
    Normal,
    Concise,
    Careful,
}

impl Default for EfficiencyMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EfficiencyConfig {
    #[serde(default)]
    pub mode: EfficiencyMode,
}

impl Default for EfficiencyConfig {
    fn default() -> Self {
        Self {
            mode: EfficiencyMode::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_normal_mode() {
        let cfg = EfficiencyConfig::default();
        assert!(matches!(cfg.mode, EfficiencyMode::Normal));
    }

    #[test]
    fn concision_mode_deserializes() {
        let cfg: EfficiencyConfig =
            toml::from_str("mode = \"concise\"").expect("should deserialize");
        assert!(matches!(cfg.mode, EfficiencyMode::Concise));
    }

    #[test]
    fn careful_mode_deserializes() {
        let cfg: EfficiencyConfig =
            toml::from_str("mode = \"careful\"").expect("should deserialize");
        assert!(matches!(cfg.mode, EfficiencyMode::Careful));
    }
}

use serde::{Deserialize, Serialize};

/// Per-profile Graphify knowledge graph configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphifyConfig {
    /// Whether Graphify integration is enabled for this profile.
    #[serde(default = "default_graphify_enabled")]
    pub enabled: bool,
    /// Custom path to graph.json (overrides default `graphify-out/graph.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_path: Option<String>,
    /// Whether to auto-run extract when no graph exists.
    #[serde(default)]
    pub auto_extract: bool,
}

fn default_graphify_enabled() -> bool {
    true
}

impl Default for GraphifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            graph_path: None,
            auto_extract: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_enabled() {
        let cfg = GraphifyConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.graph_path.is_none());
        assert!(!cfg.auto_extract);
    }

    #[test]
    fn deserialize_minimal() {
        let cfg: GraphifyConfig = toml::from_str("enabled = false").unwrap();
        assert!(!cfg.enabled);
        assert!(cfg.graph_path.is_none());
    }

    #[test]
    fn deserialize_with_path() {
        let cfg: GraphifyConfig =
            toml::from_str("enabled = true\ngraph_path = \"/custom/graph.json\"").unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.graph_path, Some("/custom/graph.json".into()));
    }
}

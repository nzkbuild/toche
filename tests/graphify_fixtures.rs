use std::path::PathBuf;

use toche::graphify::adapter::GraphifyAdapter;
use toche::graphify::config::GraphifyConfig;

#[test]
fn config_defaults() {
    let cfg = GraphifyConfig::default();
    assert!(cfg.enabled);
    assert!(cfg.graph_path.is_none());
    assert!(!cfg.auto_extract);
}

#[test]
fn config_deserialize_disabled() {
    let cfg: GraphifyConfig = toml::from_str("enabled = false").unwrap();
    assert!(!cfg.enabled);
}

#[test]
fn config_deserialize_with_path() {
    let cfg: GraphifyConfig = toml::from_str(
        "enabled = true\ngraph_path = \"/custom/graph.json\"",
    )
    .unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.graph_path, Some("/custom/graph.json".into()));
}

#[test]
fn config_deserialize_auto_extract() {
    let cfg: GraphifyConfig = toml::from_str(
        "enabled = true\nauto_extract = true",
    )
    .unwrap();
    assert!(cfg.auto_extract);
}

#[test]
fn adapter_new_default_path() {
    let adapter = GraphifyAdapter::new(None);
    assert!(adapter.graph_path.is_none());
}

#[test]
fn adapter_new_custom_path() {
    let custom = PathBuf::from("/tmp/graph.json");
    let adapter = GraphifyAdapter::new(Some(custom.clone()));
    assert_eq!(adapter.graph_path, Some(custom));
}

#[test]
fn is_installed_does_not_crash() {
    let _ = GraphifyAdapter::is_installed();
}

use std::path::PathBuf;

/// Resolves Toche config directory: TOCHE_CONFIG_DIR env -> ~/.toche
pub fn config_dir() -> PathBuf {
    std::env::var("TOCHE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".toche"))
}

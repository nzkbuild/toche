use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeCacheConfig {
    /// Enable persistent safe caching for this profile.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum age of a cache entry in days before eviction.
    #[serde(default = "default_ttl_days")]
    pub ttl_days: u32,
    /// Maximum response body size in bytes to cache.
    #[serde(default = "default_max_entry_bytes")]
    pub max_entry_bytes: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_ttl_days() -> u32 {
    30
}

fn default_max_entry_bytes() -> u64 {
    1_048_576 // 1 MiB
}

impl Default for SafeCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_days: 30,
            max_entry_bytes: 1_048_576,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_values() {
        let cfg = SafeCacheConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.ttl_days, 30);
        assert_eq!(cfg.max_entry_bytes, 1_048_576);
    }

    #[test]
    fn deserialize_minimal() {
        let toml_str = "[safe_cache]\nenabled = false";
        #[derive(Deserialize)]
        struct Wrapper {
            safe_cache: Option<SafeCacheConfig>,
        }
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        let c = w.safe_cache.unwrap();
        assert!(!c.enabled);
        assert_eq!(c.ttl_days, 30); // default
        assert_eq!(c.max_entry_bytes, 1_048_576); // default
    }

    #[test]
    fn deserialize_full() {
        let toml_str = "[safe_cache]\nenabled = true\nttl_days = 7\nmax_entry_bytes = 512000";
        #[derive(Deserialize)]
        struct Wrapper {
            safe_cache: Option<SafeCacheConfig>,
        }
        let w: Wrapper = toml::from_str(toml_str).unwrap();
        let c = w.safe_cache.unwrap();
        assert!(c.enabled);
        assert_eq!(c.ttl_days, 7);
        assert_eq!(c.max_entry_bytes, 512_000);
    }
}

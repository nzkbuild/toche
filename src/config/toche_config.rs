use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::efficiency::config::EfficiencyConfig;
use crate::graphify::config::GraphifyConfig;
use crate::reduce::config::ReduceConfig;
use crate::safe_cache::config::SafeCacheConfig;

// --- Top-level config ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TocheConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub integrations: Vec<Integration>,
    #[serde(default)]
    pub upstreams: Vec<Upstream>,
    #[serde(default)]
    pub policies: Vec<Policy>,
}

// --- RuntimeConfig ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    /// Maximum request body size in bytes (default 16 MiB).
    #[serde(default = "default_max_request_body_bytes")]
    pub max_request_body_bytes: u64,
    /// Maximum upstream response body size in bytes (default 64 MiB).
    #[serde(default = "default_max_response_body_bytes")]
    pub max_response_body_bytes: u64,
    /// Maximum concurrent upstream requests (default 8).
    #[serde(default = "default_max_concurrent_upstream")]
    pub max_concurrent_upstream: usize,
    /// Max milliseconds to wait for a concurrency permit (default 60 s).
    #[serde(default = "default_upstream_permit_timeout_ms")]
    pub upstream_permit_timeout_ms: u64,
}

fn default_port() -> u16 {
    8743
}
fn default_listen_address() -> String {
    "127.0.0.1".into()
}
fn default_request_timeout_ms() -> u64 {
    300_000
}
fn default_max_request_body_bytes() -> u64 {
    16 * 1024 * 1024 // 16 MiB
}
fn default_max_response_body_bytes() -> u64 {
    64 * 1024 * 1024 // 64 MiB
}
fn default_max_concurrent_upstream() -> usize {
    8
}
fn default_upstream_permit_timeout_ms() -> u64 {
    60_000
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            listen_address: default_listen_address(),
            request_timeout_ms: default_request_timeout_ms(),
            max_request_body_bytes: default_max_request_body_bytes(),
            max_response_body_bytes: default_max_response_body_bytes(),
            max_concurrent_upstream: default_max_concurrent_upstream(),
            upstream_permit_timeout_ms: default_upstream_permit_timeout_ms(),
        }
    }
}

impl RuntimeConfig {
    /// Validate runtime configuration values. Returns a list of human-readable
    /// validation error messages. An empty vec means valid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors: Vec<String> = Vec::new();

        if self.max_request_body_bytes == 0 {
            errors.push("runtime.max_request_body_bytes must not be zero".into());
        }
        if self.max_request_body_bytes > 256 * 1024 * 1024 {
            errors.push(format!(
                "runtime.max_request_body_bytes is unreasonably large ({} > 256 MiB)",
                self.max_request_body_bytes
            ));
        }

        if self.max_response_body_bytes == 0 {
            errors.push("runtime.max_response_body_bytes must not be zero".into());
        }
        if self.max_response_body_bytes > 1024 * 1024 * 1024 {
            errors.push(format!(
                "runtime.max_response_body_bytes is unreasonably large ({} > 1 GiB)",
                self.max_response_body_bytes
            ));
        }

        if self.max_concurrent_upstream == 0 {
            errors.push("runtime.max_concurrent_upstream must not be zero".into());
        }
        if self.max_concurrent_upstream > 1024 {
            errors.push(format!(
                "runtime.max_concurrent_upstream is unreasonably large ({} > 1024)",
                self.max_concurrent_upstream
            ));
        }

        if self.upstream_permit_timeout_ms == 0 {
            errors.push("runtime.upstream_permit_timeout_ms must not be zero".into());
        }

        errors
    }
}

// --- DefaultsConfig ---

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default)]
    pub integration: Option<String>,
}

// --- StorageConfig ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_ledger_db")]
    pub ledger_db: String,
    #[serde(default = "default_cas_dir")]
    pub cas_dir: String,
}

fn default_ledger_db() -> String {
    "ledger.db".into()
}
fn default_cas_dir() -> String {
    "cas".into()
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            ledger_db: default_ledger_db(),
            cas_dir: default_cas_dir(),
        }
    }
}

// --- Integration ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub upstream: String,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphify: Option<GraphifyConfig>,
    /// Preserved from legacy profile — model name rewriting rules.
    /// Not used by the current gateway but carried forward for
    /// future multi-client routing.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub models: HashMap<String, String>,
}

// --- Upstream ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub auth: UpstreamAuth,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamAuth {
    #[serde(default)]
    pub secret_ref: SecretRef,
    #[serde(default = "default_auth_header_name")]
    pub header_name: String,
}

fn default_auth_header_name() -> String {
    "x-api-key".into()
}

impl Default for UpstreamAuth {
    fn default() -> Self {
        Self {
            secret_ref: SecretRef::None,
            header_name: default_auth_header_name(),
        }
    }
}

// --- SecretRef ---

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SecretRef {
    #[serde(rename = "environment")]
    Environment { key: String },
    #[serde(rename = "command")]
    Command { program: String },
    #[serde(rename = "legacy_inline")]
    LegacyInline { value: String },
    #[serde(rename = "none")]
    #[default]
    None,
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretRef::Environment { key } => {
                f.debug_struct("Environment").field("key", key).finish()
            }
            SecretRef::Command { program } => {
                f.debug_struct("Command").field("program", program).finish()
            }
            SecretRef::LegacyInline { .. } => f.write_str("LegacyInline(***)"),
            SecretRef::None => f.write_str("None"),
        }
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretRef::Environment { key } => write!(f, "env:{}", key),
            SecretRef::Command { program } => write!(f, "cmd:{}", program),
            SecretRef::LegacyInline { .. } => f.write_str("inline(***)"),
            SecretRef::None => f.write_str("none"),
        }
    }
}

// --- Policy ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CachePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduce: Option<ReduceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efficiency: Option<EfficiencyConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_cache: Option<SafeCacheConfig>,
}

/// Which parts of the conversation get cache breakpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheBreakpoint {
    #[default]
    Standard,
    SystemOnly,
}

/// Controls how Toche manages provider prompt caching for a profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    #[default]
    Observe,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachePolicy {
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: CacheMode,
    #[serde(default)]
    pub breakpoint: CacheBreakpoint,
}

fn default_cache_enabled() -> bool {
    true
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: CacheMode::Observe,
            breakpoint: CacheBreakpoint::Standard,
        }
    }
}

// --- Identifier generation ---

/// Deterministic 8-hex-char ID from a prefix and normalized name.
/// Uses SHA-256 → first 4 bytes → hex. Same input always produces same ID.
pub fn derive_id(prefix: &str, name: &str) -> String {
    use sha2::Digest;
    let normalized = name.trim().to_lowercase();
    let input = format!("{prefix}:{normalized}");
    let hash = sha2::Sha256::digest(input.as_bytes());
    hex::encode(&hash[..4])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_id_deterministic() {
        let a = derive_id("integration", "default");
        let b = derive_id("integration", "default");
        assert_eq!(a, b);
    }

    #[test]
    fn derive_id_different_names() {
        let a = derive_id("integration", "default");
        let b = derive_id("integration", "other");
        assert_ne!(a, b);
    }

    #[test]
    fn derive_id_whitespace_insensitive() {
        let a = derive_id("integration", "  Default  ");
        let b = derive_id("integration", "default");
        assert_eq!(a, b);
    }

    #[test]
    fn derive_id_case_insensitive() {
        let a = derive_id("integration", "DEFAULT");
        let b = derive_id("integration", "default");
        assert_eq!(a, b);
    }

    #[test]
    fn derive_id_different_prefixes() {
        let a = derive_id("integration", "default");
        let b = derive_id("upstream", "default");
        let c = derive_id("policy", "default");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn derive_id_is_8_hex_chars() {
        let id = derive_id("integration", "default");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn secret_ref_debug_hides_legacy_inline() {
        let sr = SecretRef::LegacyInline {
            value: "sk-ant-secret".into(),
        };
        let debug = format!("{:?}", sr);
        assert!(!debug.contains("sk-ant"));
        assert!(debug.contains("***"));
    }

    #[test]
    fn secret_ref_display_hides_legacy_inline() {
        let sr = SecretRef::LegacyInline {
            value: "sk-ant-secret".into(),
        };
        let display = format!("{}", sr);
        assert!(!display.contains("sk-ant"));
        assert!(display.contains("***"));
    }

    #[test]
    fn secret_ref_environment_debug_shows_key() {
        let sr = SecretRef::Environment {
            key: "ANTHROPIC_API_KEY".into(),
        };
        let debug = format!("{:?}", sr);
        assert!(debug.contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn runtime_config_defaults() {
        let cfg = RuntimeConfig::default();
        assert_eq!(cfg.port, 8743);
        assert_eq!(cfg.listen_address, "127.0.0.1");
        assert_eq!(cfg.request_timeout_ms, 300_000);
        assert_eq!(cfg.max_request_body_bytes, 16 * 1024 * 1024);
        assert_eq!(cfg.max_response_body_bytes, 64 * 1024 * 1024);
        assert_eq!(cfg.max_concurrent_upstream, 8);
        assert_eq!(cfg.upstream_permit_timeout_ms, 60_000);
    }

    #[test]
    fn runtime_config_validate_zero_values() {
        let cfg = RuntimeConfig {
            port: 8743,
            max_request_body_bytes: 0,
            max_response_body_bytes: 0,
            max_concurrent_upstream: 0,
            upstream_permit_timeout_ms: 0,
            ..RuntimeConfig::default()
        };
        let errors = cfg.validate();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.contains("max_request_body_bytes")));
        assert!(errors.iter().any(|e| e.contains("max_response_body_bytes")));
        assert!(errors.iter().any(|e| e.contains("max_concurrent_upstream")));
        assert!(
            errors
                .iter()
                .any(|e| e.contains("upstream_permit_timeout_ms"))
        );
    }

    #[test]
    fn runtime_config_validate_overflow_values() {
        let cfg = RuntimeConfig {
            port: 8743,
            max_request_body_bytes: 300 * 1024 * 1024,
            max_response_body_bytes: 2 * 1024 * 1024 * 1024,
            max_concurrent_upstream: 2048,
            ..RuntimeConfig::default()
        };
        let errors = cfg.validate();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.contains("max_request_body_bytes")));
        assert!(errors.iter().any(|e| e.contains("max_response_body_bytes")));
        assert!(errors.iter().any(|e| e.contains("max_concurrent_upstream")));
    }

    #[test]
    fn runtime_config_valid_passes() {
        let cfg = RuntimeConfig::default();
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn runtime_config_deserialize_missing_uses_defaults() {
        let toml_str = r#"
port = 9999
listen_address = "0.0.0.0"
request_timeout_ms = 10000
"#;
        let cfg: RuntimeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.port, 9999);
        assert_eq!(cfg.max_request_body_bytes, 16 * 1024 * 1024);
        assert_eq!(cfg.max_response_body_bytes, 64 * 1024 * 1024);
        assert_eq!(cfg.max_concurrent_upstream, 8);
        assert_eq!(cfg.upstream_permit_timeout_ms, 60_000);
    }

    #[test]
    fn storage_config_defaults() {
        let cfg = StorageConfig::default();
        assert_eq!(cfg.ledger_db, "ledger.db");
        assert_eq!(cfg.cas_dir, "cas");
    }

    #[test]
    fn toche_config_roundtrip_minimal() {
        let cfg = TocheConfig {
            schema_version: 2,
            runtime: RuntimeConfig::default(),
            defaults: DefaultsConfig::default(),
            storage: StorageConfig::default(),
            integrations: vec![],
            upstreams: vec![],
            policies: vec![],
        };
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: TocheConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.schema_version, 2);
    }

    #[test]
    fn toche_config_roundtrip_full() {
        let id = derive_id("integration", "default");
        let upstream_id = derive_id("upstream", "default");
        let policy_id = derive_id("policy", "default");

        let cfg = TocheConfig {
            schema_version: 2,
            runtime: RuntimeConfig::default(),
            defaults: DefaultsConfig {
                integration: Some(id.clone()),
            },
            storage: StorageConfig::default(),
            integrations: vec![Integration {
                id: id.clone(),
                name: "default".into(),
                upstream: upstream_id.clone(),
                policy: Some(policy_id.clone()),
                graphify: None,
                models: HashMap::new(),
            }],
            upstreams: vec![Upstream {
                id: upstream_id,
                name: "Anthropic".into(),
                url: "https://api.anthropic.com".into(),
                auth: UpstreamAuth {
                    secret_ref: SecretRef::Environment {
                        key: "ANTHROPIC_API_KEY".into(),
                    },
                    header_name: "x-api-key".into(),
                },
                headers: HashMap::new(),
            }],
            policies: vec![Policy {
                id: policy_id,
                name: "default".into(),
                cache: None,
                reduce: None,
                efficiency: None,
                safe_cache: None,
            }],
        };
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: TocheConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.schema_version, 2);
        assert_eq!(parsed.integrations.len(), 1);
        assert_eq!(parsed.upstreams.len(), 1);
        assert_eq!(parsed.policies.len(), 1);
        assert_eq!(parsed.defaults.integration.as_deref(), Some(id.as_str()));
    }
}

use std::collections::HashMap;

use crate::efficiency::config::EfficiencyConfig;
use crate::graphify::config::GraphifyConfig;
use crate::reduce::config::ReduceConfig;
use crate::safe_cache::config::SafeCacheConfig;

use super::toche_config::{CachePolicy, Integration, SecretRef, TocheConfig};

/// Flattened view of an integration after resolving upstream and policy references.
#[derive(Debug, Clone)]
pub struct ResolvedIntegration {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub upstream_url: String,
    pub upstream_headers: HashMap<String, String>,
    pub auth: ResolvedAuth,
    pub cache: Option<CachePolicy>,
    pub reduce: Option<ReduceConfig>,
    pub efficiency: Option<EfficiencyConfig>,
    pub safe_cache: Option<SafeCacheConfig>,
    pub graphify: Option<GraphifyConfig>,
}

#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub header_name: String,
    #[allow(dead_code)]
    pub secret_ref: SecretRef,
    /// The actual credential value, resolved at load time.
    /// None if secret_ref is None or resolution failed.
    pub value: Option<String>,
}

/// Resolve a SecretRef to its actual credential value.
pub fn resolve_secret(sr: &SecretRef) -> Option<String> {
    match sr {
        SecretRef::Environment { key } => std::env::var(key).ok(),
        SecretRef::Command { program } => {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(program)
                .output()
                .ok()?;
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        }
        SecretRef::LegacyInline { value } => Some(value.clone()),
        SecretRef::None => None,
    }
}

/// Resolve the default integration from a TocheConfig.
/// Returns None if no default is configured or the default integration is not found.
pub fn resolve_default(config: &TocheConfig) -> Option<ResolvedIntegration> {
    let default_id = config.defaults.integration.as_deref()?;
    let integration = config.integrations.iter().find(|i| i.id == default_id)?;
    Some(resolve_integration(config, integration))
}

/// Resolve a single integration against the config's upstream and policy maps.
fn resolve_integration(config: &TocheConfig, integration: &Integration) -> ResolvedIntegration {
    let upstream = config
        .upstreams
        .iter()
        .find(|u| u.id == integration.upstream);

    let policy = integration
        .policy
        .as_deref()
        .and_then(|pid| config.policies.iter().find(|p| p.id == pid));

    let (upstream_url, upstream_headers, auth) = match upstream {
        Some(u) => (
            u.url.clone(),
            u.headers.clone(),
            ResolvedAuth {
                header_name: u.auth.header_name.clone(),
                secret_ref: u.auth.secret_ref.clone(),
                value: resolve_secret(&u.auth.secret_ref),
            },
        ),
        None => (
            String::new(),
            HashMap::new(),
            ResolvedAuth {
                header_name: String::new(),
                secret_ref: SecretRef::None,
                value: None,
            },
        ),
    };

    ResolvedIntegration {
        id: integration.id.clone(),
        name: integration.name.clone(),
        upstream_url,
        upstream_headers,
        auth,
        cache: policy.and_then(|p| p.cache.clone()),
        reduce: policy.and_then(|p| p.reduce.clone()),
        efficiency: policy.and_then(|p| p.efficiency.clone()),
        safe_cache: policy.and_then(|p| p.safe_cache.clone()),
        graphify: integration.graphify.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::toche_config::{
        CacheBreakpoint, CacheMode, DefaultsConfig, Policy, RuntimeConfig, StorageConfig, Upstream,
        UpstreamAuth,
    };

    fn make_config() -> TocheConfig {
        let i_id = crate::config::toche_config::derive_id("integration", "default");
        let u_id = crate::config::toche_config::derive_id("upstream", "default");
        let p_id = crate::config::toche_config::derive_id("policy", "default");

        TocheConfig {
            schema_version: 2,
            runtime: RuntimeConfig::default(),
            defaults: DefaultsConfig {
                integration: Some(i_id.clone()),
            },
            storage: StorageConfig::default(),
            integrations: vec![Integration {
                id: i_id,
                name: "default".into(),
                upstream: u_id.clone(),
                policy: Some(p_id.clone()),
                graphify: None,
                models: HashMap::new(),
            }],
            upstreams: vec![Upstream {
                id: u_id,
                name: "Anthropic".into(),
                url: "https://api.anthropic.com".into(),
                auth: UpstreamAuth {
                    secret_ref: SecretRef::LegacyInline {
                        value: "sk-ant-test".into(),
                    },
                    header_name: "x-api-key".into(),
                },
                headers: {
                    let mut h = HashMap::new();
                    h.insert("anthropic-version".into(), "2023-06-01".into());
                    h
                },
            }],
            policies: vec![Policy {
                id: p_id,
                name: "default".into(),
                cache: Some(CachePolicy {
                    enabled: true,
                    mode: CacheMode::Observe,
                    breakpoint: CacheBreakpoint::Standard,
                }),
                reduce: Some(ReduceConfig::default()),
                efficiency: Some(EfficiencyConfig::default()),
                safe_cache: Some(SafeCacheConfig::default()),
            }],
        }
    }

    #[test]
    fn resolve_default_returns_integration() {
        let config = make_config();
        let resolved = resolve_default(&config).unwrap();
        assert_eq!(resolved.name, "default");
        assert_eq!(resolved.upstream_url, "https://api.anthropic.com");
    }

    #[test]
    fn resolve_default_returns_none_when_no_default() {
        let mut config = make_config();
        config.defaults.integration = None;
        assert!(resolve_default(&config).is_none());
    }

    #[test]
    fn resolve_secret_legacy_inline() {
        let sr = SecretRef::LegacyInline {
            value: "my-key".into(),
        };
        assert_eq!(resolve_secret(&sr), Some("my-key".into()));
    }

    #[test]
    fn resolve_secret_none() {
        assert_eq!(resolve_secret(&SecretRef::None), None);
    }

    #[test]
    fn resolve_secret_environment() {
        unsafe {
            std::env::set_var("TOCHE_TEST_SECRET", "test-value");
        }
        let sr = SecretRef::Environment {
            key: "TOCHE_TEST_SECRET".into(),
        };
        assert_eq!(resolve_secret(&sr), Some("test-value".into()));
    }

    #[test]
    fn resolve_secret_environment_missing() {
        let sr = SecretRef::Environment {
            key: "TOCHE_NONEXISTENT_VAR".into(),
        };
        assert_eq!(resolve_secret(&sr), None);
    }

    #[test]
    fn resolved_integration_has_cache_policy() {
        let config = make_config();
        let resolved = resolve_default(&config).unwrap();
        let cache = resolved.cache.unwrap();
        assert!(cache.enabled);
        assert!(matches!(cache.mode, CacheMode::Observe));
    }

    #[test]
    fn resolved_integration_has_upstream_headers() {
        let config = make_config();
        let resolved = resolve_default(&config).unwrap();
        assert_eq!(
            resolved.upstream_headers.get("anthropic-version"),
            Some(&"2023-06-01".to_string())
        );
    }
}

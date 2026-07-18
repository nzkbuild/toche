use std::path::Path;

use super::toche_config::{
    DefaultsConfig, Integration, Policy, RuntimeConfig, SecretRef, StorageConfig, TocheConfig,
    Upstream, UpstreamAuth, derive_id,
};
use crate::config::utils::atomic_write_secure;
use crate::profiles::types::{AuthMethod, Profiles};

/// Result of attempting to detect and load configuration.
pub enum ConfigSource {
    /// config.toml (schema v2) found and loaded.
    V2(TocheConfig),
    /// profiles.toml (schema v1) found, migrated, and the result.
    V1Migrated(TocheConfig),
    /// Neither config.toml nor profiles.toml found.
    Missing,
}

/// Detect which config version exists and load accordingly.
/// If only profiles.toml exists, migrate it to config.toml.
pub fn detect_and_load(config_dir: &Path) -> anyhow::Result<ConfigSource> {
    let v2_path = config_dir.join("config.toml");
    let v1_path = config_dir.join("profiles.toml");

    if v2_path.exists() {
        let content = std::fs::read_to_string(&v2_path)?;
        let config: TocheConfig = toml::from_str(&content)?;
        if config.schema_version == 2 {
            return Ok(ConfigSource::V2(config));
        }
        anyhow::bail!(
            "Unknown schema_version {} in config.toml",
            config.schema_version
        );
    }

    if v1_path.exists() {
        // Idempotency: if a previous migration already produced config.toml,
        // do not overwrite it; treat the existing v2 config as authoritative.
        if v2_path.exists() {
            let content = std::fs::read_to_string(&v2_path)?;
            let config: TocheConfig = toml::from_str(&content)?;
            return Ok(ConfigSource::V2(config));
        }

        let content = std::fs::read_to_string(&v1_path)?;
        let profiles: Profiles = toml::from_str(&content)?;
        let config = migrate_v1_to_v2(&profiles);
        // Persist the migrated config atomically
        let toml_str = toml::to_string_pretty(&config)?;
        atomic_write_secure(&v2_path, &toml_str)?;
        // Backup the old file only once to avoid clobbering an existing backup.
        let bak_path = config_dir.join("profiles.toml.v1.bak");
        if !bak_path.exists() {
            std::fs::rename(&v1_path, &bak_path)?;
        } else {
            std::fs::remove_file(&v1_path)?;
        }
        return Ok(ConfigSource::V1Migrated(config));
    }

    Ok(ConfigSource::Missing)
}

/// Convert a v2 TocheConfig back to the legacy Profiles shape.
/// Used by the deprecated `load_profiles()` compatibility wrapper.
#[allow(dead_code)]
pub fn config_to_legacy_profiles(config: &TocheConfig) -> anyhow::Result<Profiles> {
    use crate::profiles::types::{CacheConfig, Profile};

    let mut profiles = Vec::new();
    for integration in &config.integrations {
        let upstream = config
            .upstreams
            .iter()
            .find(|u| u.id == integration.upstream)
            .ok_or_else(|| anyhow::anyhow!("upstream {} not found", integration.upstream))?;

        let policy = integration
            .policy
            .as_deref()
            .and_then(|pid| config.policies.iter().find(|p| p.id == pid));

        let auth_method = match &upstream.auth.secret_ref {
            SecretRef::LegacyInline { value } => {
                if upstream
                    .auth
                    .header_name
                    .eq_ignore_ascii_case("authorization")
                {
                    AuthMethod::BearerToken {
                        token: value.clone(),
                    }
                } else {
                    AuthMethod::ApiKey {
                        header_name: upstream.auth.header_name.clone(),
                        key: value.clone(),
                    }
                }
            }
            _ => AuthMethod::None,
        };

        let cache = policy.map(|p| {
            use crate::config::toche_config::{
                CacheBreakpoint as V2Breakpoint, CacheMode as V2Mode,
            };
            CacheConfig {
                enabled: p.cache.as_ref().map(|c| c.enabled).unwrap_or(true),
                mode: match p.cache.as_ref().map(|c| &c.mode) {
                    Some(V2Mode::Auto) => crate::profiles::types::CacheMode::Auto,
                    _ => crate::profiles::types::CacheMode::Observe,
                },
                breakpoint: match p.cache.as_ref().map(|c| &c.breakpoint) {
                    Some(V2Breakpoint::SystemOnly) => {
                        crate::profiles::types::CacheBreakpoint::SystemOnly
                    }
                    _ => crate::profiles::types::CacheBreakpoint::Standard,
                },
            }
        });

        profiles.push(Profile {
            name: integration.name.clone(),
            upstream_url: upstream.url.clone(),
            auth_method,
            headers: upstream.headers.clone(),
            models: integration.models.clone(),
            cache,
            reduce: policy.and_then(|p| p.reduce.clone()),
            efficiency: policy.and_then(|p| p.efficiency.clone()),
            safe_cache: policy.and_then(|p| p.safe_cache.clone()),
            graphify: integration.graphify.clone(),
        });
    }

    let default = config.defaults.integration.as_ref().and_then(|id| {
        config
            .integrations
            .iter()
            .find(|i| i.id == *id)
            .map(|i| i.name.clone())
    });

    Ok(Profiles { default, profiles })
}

/// Migrate legacy Profiles to TocheConfig v2.
/// Each Profile becomes one Integration + one Upstream + one Policy.
pub fn migrate_v1_to_v2(profiles: &Profiles) -> TocheConfig {
    let mut integrations = Vec::new();
    let mut upstreams = Vec::new();
    let mut policies = Vec::new();

    for profile in &profiles.profiles {
        let i_id = derive_id("integration", &profile.name);
        let u_id = derive_id("upstream", &profile.name);
        let p_id = derive_id("policy", &profile.name);

        // Build upstream
        let (secret_ref, header_name) = migrate_auth(&profile.auth_method);
        upstreams.push(Upstream {
            id: u_id.clone(),
            name: profile.name.clone(),
            url: profile.upstream_url.clone(),
            auth: UpstreamAuth {
                secret_ref,
                header_name,
            },
            headers: profile.headers.clone(),
        });

        // Build policy from per-profile feature configs
        let policy = Policy {
            id: p_id.clone(),
            name: profile.name.clone(),
            cache: profile.cache.as_ref().map(|c| {
                use crate::config::toche_config::{CacheBreakpoint, CacheMode, CachePolicy};
                CachePolicy {
                    enabled: c.enabled,
                    mode: match c.mode {
                        crate::profiles::types::CacheMode::Observe => CacheMode::Observe,
                        crate::profiles::types::CacheMode::Auto => CacheMode::Auto,
                    },
                    breakpoint: match c.breakpoint {
                        crate::profiles::types::CacheBreakpoint::Standard => {
                            CacheBreakpoint::Standard
                        }
                        crate::profiles::types::CacheBreakpoint::SystemOnly => {
                            CacheBreakpoint::SystemOnly
                        }
                    },
                }
            }),
            reduce: profile.reduce.clone(),
            efficiency: profile.efficiency.clone(),
            safe_cache: profile.safe_cache.clone(),
        };
        policies.push(policy);

        // Build integration
        integrations.push(Integration {
            id: i_id.clone(),
            name: profile.name.clone(),
            upstream: u_id,
            policy: Some(p_id),
            graphify: profile.graphify.clone(),
            models: profile.models.clone(),
        });
    }

    let default_integration = profiles.default.as_ref().and_then(|default_name| {
        let normalized = default_name.trim().to_lowercase();
        integrations
            .iter()
            .find(|i| i.name.to_lowercase() == normalized)
            .map(|i| i.id.clone())
    });

    TocheConfig {
        schema_version: 2,
        runtime: RuntimeConfig::default(),
        defaults: DefaultsConfig {
            integration: default_integration,
        },
        storage: StorageConfig::default(),
        integrations,
        upstreams,
        policies,
    }
}

fn migrate_auth(auth: &AuthMethod) -> (SecretRef, String) {
    match auth {
        AuthMethod::ApiKey { header_name, key } => (
            SecretRef::LegacyInline { value: key.clone() },
            header_name.clone(),
        ),
        AuthMethod::BearerToken { token } => (
            SecretRef::LegacyInline {
                value: token.clone(),
            },
            "authorization".into(),
        ),
        AuthMethod::None => (SecretRef::None, "x-api-key".into()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::efficiency::config::{EfficiencyConfig, EfficiencyMode};
    use crate::graphify::config::GraphifyConfig;
    use crate::profiles::types::{
        AuthMethod, CacheBreakpoint as V1CacheBreakpoint, CacheConfig as V1CacheConfig,
        CacheMode as V1CacheMode, Profile, Profiles,
    };
    use crate::reduce::config::ReduceConfig;
    use crate::safe_cache::config::SafeCacheConfig;

    #[test]
    fn migrate_single_profile_api_key() {
        let profiles = Profiles {
            default: Some("default".into()),
            profiles: vec![Profile {
                name: "default".into(),
                upstream_url: "https://api.anthropic.com".into(),
                auth_method: AuthMethod::ApiKey {
                    header_name: "x-api-key".into(),
                    key: "sk-ant-secret".into(),
                },
                headers: {
                    let mut h = HashMap::new();
                    h.insert("anthropic-version".into(), "2023-06-01".into());
                    h
                },
                models: HashMap::new(),
                cache: None,
                reduce: None,
                efficiency: None,
                safe_cache: None,
                graphify: None,
            }],
        };

        let config = migrate_v1_to_v2(&profiles);
        assert_eq!(config.schema_version, 2);
        assert_eq!(config.integrations.len(), 1);
        assert_eq!(config.upstreams.len(), 1);
        assert_eq!(config.policies.len(), 1);

        let upstream = &config.upstreams[0];
        assert_eq!(upstream.url, "https://api.anthropic.com");
        assert_eq!(
            upstream.headers.get("anthropic-version"),
            Some(&"2023-06-01".to_string())
        );

        let integration = &config.integrations[0];
        assert_eq!(integration.upstream, upstream.id);
        assert_eq!(
            integration.policy.as_deref().unwrap(),
            config.policies[0].id
        );

        // Default is set by ID reference
        assert_eq!(
            config.defaults.integration.as_deref().unwrap(),
            integration.id
        );
    }

    #[test]
    fn migrate_all_feature_configs() {
        let profiles = Profiles {
            default: None,
            profiles: vec![Profile {
                name: "full".into(),
                upstream_url: "https://api.example.com".into(),
                auth_method: AuthMethod::None,
                headers: HashMap::new(),
                models: HashMap::new(),
                cache: Some(V1CacheConfig {
                    enabled: false,
                    mode: V1CacheMode::Auto,
                    breakpoint: V1CacheBreakpoint::SystemOnly,
                }),
                reduce: Some(ReduceConfig {
                    enabled: true,
                    command_bypass: vec!["kubectl".into()],
                }),
                efficiency: Some(EfficiencyConfig {
                    mode: EfficiencyMode::Concise,
                }),
                safe_cache: Some(SafeCacheConfig {
                    enabled: false,
                    ttl_days: 7,
                    max_entry_bytes: 500_000,
                }),
                graphify: Some(GraphifyConfig {
                    enabled: true,
                    graph_path: Some("/custom/graph.json".into()),
                    auto_extract: true,
                }),
            }],
        };

        let config = migrate_v1_to_v2(&profiles);

        let policy = &config.policies[0];
        let cache = policy.cache.as_ref().unwrap();
        assert!(!cache.enabled);
        assert!(matches!(
            cache.mode,
            crate::config::toche_config::CacheMode::Auto
        ));
        assert!(matches!(
            cache.breakpoint,
            crate::config::toche_config::CacheBreakpoint::SystemOnly
        ));

        let reduce = policy.reduce.as_ref().unwrap();
        assert!(reduce.enabled);
        assert_eq!(reduce.command_bypass, vec!["kubectl"]);

        let efficiency = policy.efficiency.as_ref().unwrap();
        assert!(matches!(efficiency.mode, EfficiencyMode::Concise));

        let safe_cache = policy.safe_cache.as_ref().unwrap();
        assert!(!safe_cache.enabled);
        assert_eq!(safe_cache.ttl_days, 7);

        let integration = &config.integrations[0];
        let graphify = integration.graphify.as_ref().unwrap();
        assert!(graphify.enabled);
        assert_eq!(graphify.graph_path.as_deref(), Some("/custom/graph.json"));
    }

    #[test]
    fn migrate_bearer_token_to_legacy_inline() {
        let profiles = Profiles {
            default: None,
            profiles: vec![Profile {
                name: "bearer".into(),
                upstream_url: "https://api.example.com".into(),
                auth_method: AuthMethod::BearerToken {
                    token: "my-token".into(),
                },
                headers: HashMap::new(),
                models: HashMap::new(),
                cache: None,
                reduce: None,
                efficiency: None,
                safe_cache: None,
                graphify: None,
            }],
        };

        let config = migrate_v1_to_v2(&profiles);
        let upstream = &config.upstreams[0];
        assert_eq!(upstream.auth.header_name, "authorization");
        if let SecretRef::LegacyInline { value } = &upstream.auth.secret_ref {
            assert_eq!(value, "my-token");
        } else {
            panic!("expected LegacyInline");
        }
    }

    #[test]
    fn migrate_two_profiles() {
        let profiles = Profiles {
            default: Some("default".into()),
            profiles: vec![
                Profile {
                    name: "default".into(),
                    upstream_url: "https://api.anthropic.com".into(),
                    auth_method: AuthMethod::None,
                    headers: HashMap::new(),
                    models: HashMap::new(),
                    cache: None,
                    reduce: None,
                    efficiency: None,
                    safe_cache: None,
                    graphify: None,
                },
                Profile {
                    name: "openai".into(),
                    upstream_url: "https://api.openai.com".into(),
                    auth_method: AuthMethod::None,
                    headers: HashMap::new(),
                    models: HashMap::new(),
                    cache: None,
                    reduce: None,
                    efficiency: None,
                    safe_cache: None,
                    graphify: None,
                },
            ],
        };

        let config = migrate_v1_to_v2(&profiles);
        assert_eq!(config.integrations.len(), 2);
        assert_eq!(config.upstreams.len(), 2);
        assert_eq!(config.policies.len(), 2);

        // Default should point to the "default" integration
        let default_id = config.defaults.integration.as_deref().unwrap();
        let default_integration = config
            .integrations
            .iter()
            .find(|i| i.id == default_id)
            .unwrap();
        assert_eq!(default_integration.name, "default");
    }

    #[test]
    fn deterministic_ids_across_runs() {
        let make_profiles = || Profiles {
            default: None,
            profiles: vec![Profile {
                name: "default".into(),
                upstream_url: "https://api.anthropic.com".into(),
                auth_method: AuthMethod::None,
                headers: HashMap::new(),
                models: HashMap::new(),
                cache: None,
                reduce: None,
                efficiency: None,
                safe_cache: None,
                graphify: None,
            }],
        };

        let a = migrate_v1_to_v2(&make_profiles());
        let b = migrate_v1_to_v2(&make_profiles());

        assert_eq!(a.integrations[0].id, b.integrations[0].id);
        assert_eq!(a.upstreams[0].id, b.upstreams[0].id);
        assert_eq!(a.policies[0].id, b.policies[0].id);
    }

    #[test]
    fn detect_and_load_migrates_v1_to_v2() {
        let dir = tempfile::tempdir().unwrap();
        let profiles_path = dir.path().join("profiles.toml");
        std::fs::write(
            &profiles_path,
            r#"
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "none" }
"#,
        )
        .unwrap();

        let source = detect_and_load(dir.path()).unwrap();
        let config = match source {
            ConfigSource::V1Migrated(c) => c,
            _ => panic!("expected V1Migrated"),
        };
        assert_eq!(config.schema_version, 2);
        assert_eq!(config.integrations.len(), 1);

        // config.toml should exist and be valid
        let config_path = dir.path().join("config.toml");
        assert!(config_path.exists());
        let raw = std::fs::read_to_string(&config_path).unwrap();
        let parsed: TocheConfig = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.schema_version, 2);

        // Legacy file should be backed up
        let bak_path = dir.path().join("profiles.toml.v1.bak");
        assert!(bak_path.exists());
        assert!(!profiles_path.exists());
    }

    #[test]
    fn detect_and_load_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let profiles_path = dir.path().join("profiles.toml");
        std::fs::write(
            &profiles_path,
            r#"
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "none" }
"#,
        )
        .unwrap();

        let first = detect_and_load(dir.path()).unwrap();
        let config1 = match first {
            ConfigSource::V1Migrated(c) => c,
            _ => panic!("expected V1Migrated on first call"),
        };

        // Second call should load existing config.toml, not re-migrate
        let second = detect_and_load(dir.path()).unwrap();
        let config2 = match second {
            ConfigSource::V2(c) => c,
            _ => panic!("expected V2 on second call"),
        };

        assert_eq!(config1.integrations[0].id, config2.integrations[0].id);
        assert!(dir.path().join("config.toml").exists());
    }

    #[test]
    fn detect_and_load_prefers_existing_v2() {
        let dir = tempfile::tempdir().unwrap();

        // Write a v2 config
        std::fs::write(
            dir.path().join("config.toml"),
            r#"schema_version = 2

[runtime]
port = 8743
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[defaults]
integration = "abc12345"

[[integrations]]
id = "abc12345"
name = "custom"
upstream = "def67890"

[[upstreams]]
id = "def67890"
name = "Custom"
url = "https://api.custom.com"

[[policies]]
id = "pol11111"
name = "custom"
"#,
        )
        .unwrap();

        // Also write a legacy profiles.toml
        std::fs::write(
            dir.path().join("profiles.toml"),
            r#"
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "none" }
"#,
        )
        .unwrap();

        let source = detect_and_load(dir.path()).unwrap();
        let config = match source {
            ConfigSource::V2(c) => c,
            _ => panic!("expected V2 when both files exist"),
        };
        assert_eq!(config.integrations[0].name, "custom");
        assert_eq!(config.upstreams[0].url, "https://api.custom.com");
    }

    #[test]
    fn detect_and_load_rejects_malformed_v2() {
        let dir = tempfile::tempdir().unwrap();

        // Malformed config.toml
        std::fs::write(
            dir.path().join("config.toml"),
            "this is not valid toml ::::",
        )
        .unwrap();

        // Valid legacy profiles.toml
        std::fs::write(
            dir.path().join("profiles.toml"),
            r#"
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "none" }
"#,
        )
        .unwrap();

        // Should fail because config.toml exists but is malformed, not fall back
        let result = detect_and_load(dir.path());
        assert!(result.is_err());
        // Legacy file should remain untouched
        assert!(dir.path().join("profiles.toml").exists());
    }

    #[test]
    fn detect_and_load_rejects_unsupported_schema() {
        let dir = tempfile::tempdir().unwrap();

        // config.toml with unsupported schema version
        std::fs::write(
            dir.path().join("config.toml"),
            r#"schema_version = 99

[runtime]
port = 8743
listen_address = "127.0.0.1"
request_timeout_ms = 300000
"#,
        )
        .unwrap();

        let result = detect_and_load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn detect_and_load_backup_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();

        // Pre-existing backup
        std::fs::write(
            dir.path().join("profiles.toml.v1.bak"),
            "original backup content",
        )
        .unwrap();

        // Legacy profiles.toml
        std::fs::write(
            dir.path().join("profiles.toml"),
            r#"
default = "default"

[[profiles]]
name = "default"
upstream_url = "https://api.anthropic.com"
auth_method = { type = "none" }
"#,
        )
        .unwrap();

        detect_and_load(dir.path()).unwrap();

        // Backup should still contain the original content
        let backup_content =
            std::fs::read_to_string(dir.path().join("profiles.toml.v1.bak")).unwrap();
        assert_eq!(backup_content, "original backup content");
        // Legacy file should be removed after migration
        assert!(!dir.path().join("profiles.toml").exists());
    }
}

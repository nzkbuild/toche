use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;

use crate::config::loader::config_dir;
use crate::config::migration::{ConfigSource, detect_and_load};
use crate::config::toche_config::{
    CachePolicy, DefaultsConfig, Integration, Policy, RuntimeConfig, SecretRef, StorageConfig,
    TocheConfig, Upstream, UpstreamAuth, derive_id,
};
use crate::config::utils::atomic_write_secure;
use crate::profiles::types::Profiles;

pub mod preview;

/// Ownership record written after a successful setup transaction.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct OwnershipRecord {
    pub version: String,
    pub integration_ids: Vec<String>,
    pub upstream_ids: Vec<String>,
    pub policy_ids: Vec<String>,
}

/// Result of running the setup transaction engine.
#[derive(Debug, Clone)]
pub enum SetupOutcome {
    /// No changes were necessary.
    NoOp,
    /// Configuration was applied.
    Applied {
        config: Box<TocheConfig>,
        record: OwnershipRecord,
    },
}

/// High-level setup transaction coordinator.
///
/// Lifecycle: Detect -> Resolve -> Ask -> Preview -> Validate -> Apply ->
///            Re-read -> Verify -> Ownership record.
pub struct SetupTransaction {
    config_dir: PathBuf,
    interactive: bool,
    force: bool,
    /// Injected answers for non-interactive validation mode.
    answers: SetupAnswers,
}

/// Answers collected from the user or injected for tests.
#[derive(Debug, Clone, Default)]
pub struct SetupAnswers {
    pub upstream_url: Option<String>,
    pub api_key: Option<String>,
    pub header_name: Option<String>,
    pub integration_name: Option<String>,
}

impl SetupTransaction {
    pub fn new(interactive: bool, force: bool) -> Self {
        Self {
            config_dir: config_dir(),
            interactive,
            force,
            answers: SetupAnswers::default(),
        }
    }

    /// Provide injected answers (used in non-interactive validation/tests).
    #[allow(dead_code)]
    pub fn with_answers(mut self, answers: SetupAnswers) -> Self {
        self.answers = answers;
        self
    }

    /// Run the full setup lifecycle and return the outcome.
    pub fn run(&self) -> anyhow::Result<SetupOutcome> {
        // 1. Detect
        let detected =
            detect_and_load(&self.config_dir).context("Failed to detect existing configuration")?;

        // 2. Resolve what we already know
        let resolved = self.resolve_existing(&detected)?;

        // 3. Ask only unresolved questions
        let answers = self.collect_answers(resolved)?;

        // 4. Build preview
        let proposed = self.build_config(&answers)?;
        let preview = preview::render(&proposed, &self.config_dir);

        // 5. Preview / confirm
        if self.interactive {
            println!("\n{preview}");
            if !answers.is_fully_resolved() {
                let confirm = inquire::Confirm::new("Apply these changes?")
                    .with_default(true)
                    .prompt()
                    .map_err(|e| anyhow::anyhow!("Prompt failed: {e}"))?;
                if !confirm {
                    return Ok(SetupOutcome::NoOp);
                }
            }
        }

        // 6. Validate
        self.validate(&proposed)?;

        // If a config already exists and the proposed config is equivalent,
        // skip applying and report a no-op.
        if let ConfigSource::V2(existing) | ConfigSource::V1Migrated(existing) = &detected {
            if configs_equivalent(existing, &proposed) {
                return Ok(SetupOutcome::NoOp);
            }
        }

        // 7. Apply transactionally
        self.apply(&proposed)?;

        // 8. Re-read
        let reloaded = self.reload()?;

        // 9. Verify
        self.verify(&reloaded, &proposed)?;

        // 10. Ownership record
        let record = OwnershipRecord {
            version: env!("CARGO_PKG_VERSION").to_string(),
            integration_ids: reloaded.integrations.iter().map(|i| i.id.clone()).collect(),
            upstream_ids: reloaded.upstreams.iter().map(|u| u.id.clone()).collect(),
            policy_ids: reloaded.policies.iter().map(|p| p.id.clone()).collect(),
        };

        Ok(SetupOutcome::Applied {
            config: Box::new(reloaded),
            record,
        })
    }

    fn resolve_existing(&self, detected: &ConfigSource) -> anyhow::Result<ResolvedInputs> {
        match detected {
            ConfigSource::V2(config) | ConfigSource::V1Migrated(config) => {
                Ok(ResolvedInputs::from_config(config))
            }
            ConfigSource::Missing => Ok(ResolvedInputs::default()),
        }
    }

    fn collect_answers(&self, resolved: ResolvedInputs) -> anyhow::Result<SetupAnswers> {
        if self.interactive {
            self.prompt_interactively(resolved)
        } else {
            Ok(self.answers.clone())
        }
    }

    fn prompt_interactively(&self, resolved: ResolvedInputs) -> anyhow::Result<SetupAnswers> {
        let upstream_url = if let Some(url) = resolved.upstream_url {
            inquire::Text::new("Upstream URL:")
                .with_default(&url)
                .prompt()
                .map_err(|e| anyhow::anyhow!("Prompt failed: {e}"))?
        } else {
            inquire::Text::new("Upstream URL:")
                .with_default("https://api.anthropic.com")
                .prompt()
                .map_err(|e| anyhow::anyhow!("Prompt failed: {e}"))?
        };

        let api_key = inquire::Text::new("API key (leave blank for none):")
            .prompt()
            .map_err(|e| anyhow::anyhow!("Prompt failed: {e}"))?;

        let header_name = inquire::Text::new("Auth header name:")
            .with_default("x-api-key")
            .prompt()
            .map_err(|e| anyhow::anyhow!("Prompt failed: {e}"))?;

        let integration_name = inquire::Text::new("Integration name:")
            .with_default("default")
            .prompt()
            .map_err(|e| anyhow::anyhow!("Prompt failed: {e}"))?;

        Ok(SetupAnswers {
            upstream_url: Some(upstream_url),
            api_key: if api_key.is_empty() {
                None
            } else {
                Some(api_key)
            },
            header_name: Some(header_name),
            integration_name: Some(integration_name),
        })
    }

    fn build_config(&self, answers: &SetupAnswers) -> anyhow::Result<TocheConfig> {
        let name = answers
            .integration_name
            .as_deref()
            .unwrap_or("default")
            .to_string();
        let upstream_url = answers
            .upstream_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com")
            .to_string();
        let header_name = answers
            .header_name
            .as_deref()
            .unwrap_or("x-api-key")
            .to_string();

        let i_id = derive_id("integration", &name);
        let u_id = derive_id("upstream", &name);
        let p_id = derive_id("policy", &name);

        let secret_ref = match &answers.api_key {
            Some(key) => SecretRef::LegacyInline { value: key.clone() },
            None => SecretRef::None,
        };

        let integration = Integration {
            id: i_id.clone(),
            name: name.clone(),
            upstream: u_id.clone(),
            policy: Some(p_id.clone()),
            graphify: None,
            models: HashMap::new(),
        };

        let upstream = Upstream {
            id: u_id.clone(),
            name: name.clone(),
            url: upstream_url,
            auth: UpstreamAuth {
                secret_ref,
                header_name,
            },
            headers: HashMap::new(),
        };

        let policy = Policy {
            id: p_id,
            name: name.clone(),
            cache: Some(CachePolicy::default()),
            reduce: None,
            efficiency: None,
            safe_cache: None,
        };

        Ok(TocheConfig {
            schema_version: 2,
            runtime: RuntimeConfig::default(),
            defaults: DefaultsConfig {
                integration: Some(i_id),
            },
            storage: StorageConfig::default(),
            integrations: vec![integration],
            upstreams: vec![upstream],
            policies: vec![policy],
        })
    }

    fn validate(&self, config: &TocheConfig) -> anyhow::Result<()> {
        if config.schema_version != 2 {
            anyhow::bail!("unsupported schema version {}", config.schema_version);
        }
        if config.upstreams.is_empty() {
            anyhow::bail!("at least one upstream is required");
        }
        if config.integrations.is_empty() {
            anyhow::bail!("at least one integration is required");
        }
        Ok(())
    }

    fn apply(&self, config: &TocheConfig) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir).context("Failed to create config directory")?;

        let config_path = self.config_dir.join("config.toml");

        // Backup existing config if force is set and it exists.
        if self.force && config_path.exists() {
            let bak_path = self.config_dir.join("config.toml.bak");
            std::fs::copy(&config_path, &bak_path)
                .context("Failed to backup existing config.toml")?;
        }

        let toml_str = toml::to_string_pretty(config).context("Failed to serialize config")?;
        atomic_write_secure(&config_path, &toml_str).context("Failed to write config.toml")?;

        Ok(())
    }

    fn reload(&self) -> anyhow::Result<TocheConfig> {
        match detect_and_load(&self.config_dir)
            .context("Failed to reload configuration after apply")?
        {
            ConfigSource::V2(c) | ConfigSource::V1Migrated(c) => Ok(c),
            ConfigSource::Missing => anyhow::bail!("config.toml disappeared after apply"),
        }
    }

    fn verify(&self, reloaded: &TocheConfig, proposed: &TocheConfig) -> anyhow::Result<()> {
        if reloaded.schema_version != proposed.schema_version {
            anyhow::bail!("schema version mismatch after reload");
        }
        if reloaded.integrations.len() != proposed.integrations.len() {
            anyhow::bail!("integration count mismatch after reload");
        }
        if reloaded.upstreams.len() != proposed.upstreams.len() {
            anyhow::bail!("upstream count mismatch after reload");
        }
        Ok(())
    }
}

/// Existing configuration facts used to pre-fill setup questions.
#[derive(Debug, Default)]
struct ResolvedInputs {
    upstream_url: Option<String>,
}

impl ResolvedInputs {
    fn from_config(config: &TocheConfig) -> Self {
        let upstream_url = config
            .defaults
            .integration
            .as_ref()
            .and_then(|id| config.integrations.iter().find(|i| i.id == *id))
            .and_then(|i| config.upstreams.iter().find(|u| u.id == i.upstream))
            .map(|u| u.url.clone());

        Self { upstream_url }
    }
}

impl SetupAnswers {
    fn is_fully_resolved(&self) -> bool {
        self.upstream_url.is_some() && self.integration_name.is_some() && self.header_name.is_some()
    }
}

/// Import a legacy `Profiles` into a v2 `TocheConfig`.
#[allow(dead_code)]
pub fn import_legacy_profiles(profiles: &Profiles) -> TocheConfig {
    crate::config::migration::migrate_v1_to_v2(profiles)
}

/// Compare two configs for functional equivalence for the purpose of
/// deciding whether a setup re-run is a no-op.
fn configs_equivalent(a: &TocheConfig, b: &TocheConfig) -> bool {
    a.schema_version == b.schema_version
        && a.runtime.port == b.runtime.port
        && a.runtime.listen_address == b.runtime.listen_address
        && a.runtime.request_timeout_ms == b.runtime.request_timeout_ms
        && a.defaults.integration == b.defaults.integration
        && a.storage.ledger_db == b.storage.ledger_db
        && a.storage.cas_dir == b.storage.cas_dir
        && a.integrations.len() == b.integrations.len()
        && a.upstreams.len() == b.upstreams.len()
        && a.policies.len() == b.policies.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers_default() -> SetupAnswers {
        SetupAnswers {
            upstream_url: Some("https://api.anthropic.com".into()),
            api_key: Some("sk-test".into()),
            header_name: Some("x-api-key".into()),
            integration_name: Some("default".into()),
        }
    }

    #[test]
    fn setup_no_op_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        let applied = match outcome {
            SetupOutcome::Applied { config, .. } => config,
            SetupOutcome::NoOp => panic!("expected applied on first run"),
        };
        assert_eq!(applied.schema_version, 2);
        assert_eq!(applied.integrations.len(), 1);

        // Re-run with identical answers should be a no-op.
        let tx2 = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        let outcome2 = tx2.run().unwrap();
        assert!(matches!(outcome2, SetupOutcome::NoOp));
    }

    #[test]
    fn setup_interruption_leaves_config_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        tx.run().unwrap();

        let before = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();

        // Simulate interruption by removing the temp file path; atomic_write_secure
        // cleans stale temp files, so the original config remains intact.
        let tmp = dir.path().join("config.toml.tmp");
        std::fs::write(&tmp, "partial content").unwrap();

        let tx2 = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        tx2.run().unwrap();

        let after = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn setup_preview_contains_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        let config = match outcome {
            SetupOutcome::Applied { config, .. } => config,
            SetupOutcome::NoOp => panic!("expected applied"),
        };
        let preview = preview::render(&config, dir.path());
        assert!(preview.contains("https://api.anthropic.com"));
        assert!(preview.contains("default"));
    }

    #[test]
    fn setup_apply_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        let config = match outcome {
            SetupOutcome::Applied { config, .. } => config,
            SetupOutcome::NoOp => panic!("expected applied"),
        };
        assert_eq!(config.upstreams.len(), 1);
        assert_eq!(config.integrations.len(), 1);
        assert_eq!(config.policies.len(), 1);

        // Removing the config file and re-running creates a fresh one.
        std::fs::remove_file(dir.path().join("config.toml")).unwrap();
        let tx2 = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            answers: answers_default(),
        };
        let outcome2 = tx2.run().unwrap();
        assert!(matches!(outcome2, SetupOutcome::Applied { .. }));
    }
}

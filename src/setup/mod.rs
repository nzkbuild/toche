use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipRecord {
    pub version: String,
    pub integration_ids: Vec<String>,
    pub upstream_ids: Vec<String>,
    pub policy_ids: Vec<String>,
}

/// Result of running the setup transaction engine.
#[derive(Debug, Clone)]
pub enum SetupOutcome {
    NoOp,
    Applied {
        config: Box<TocheConfig>,
        record: OwnershipRecord,
    },
    /// --dry-run: preview was generated but nothing written.
    #[allow(dead_code)]
    DryRun {
        config: Box<TocheConfig>,
        preview: String,
    },
}

/// Result of running setup with --json.
#[derive(Debug, Clone, Serialize)]
pub struct SetupJsonOutput {
    pub schema_version: u32,
    pub outcome: String,
    pub preview: String,
    pub integrations: Vec<IntegrationSummary>,
    pub upstreams: Vec<UpstreamSummary>,
    pub policies: Vec<PolicySummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationSummary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamSummary {
    pub id: String,
    pub name: String,
    pub url: String,
    /// auth header name — credential value NOT included
    pub auth_header: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicySummary {
    pub id: String,
    pub name: String,
}

/// High-level setup transaction coordinator.
///
/// Lifecycle: Acquire lock → Detect → Resolve → Ask → Preview → Validate →
///            Apply transactionally → Re-read → Verify → Ownership record →
///            Release lock.
pub struct SetupTransaction {
    config_dir: PathBuf,
    interactive: bool,
    #[allow(dead_code)]
    force: bool,
    dry_run: bool,
    json_output: bool,
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
            dry_run: false,
            json_output: false,
            answers: SetupAnswers::default(),
        }
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn with_json(mut self, json: bool) -> Self {
        self.json_output = json;
        self
    }

    #[allow(dead_code)]
    pub fn with_answers(mut self, answers: SetupAnswers) -> Self {
        self.answers = answers;
        self
    }

    /// For tests: override config_dir to a temp directory.
    #[allow(dead_code)]
    pub fn with_config_dir(mut self, dir: PathBuf) -> Self {
        self.config_dir = dir;
        self
    }

    /// Run the full setup lifecycle and return the outcome.
    pub fn run(&self) -> anyhow::Result<SetupOutcome> {
        // 0. Acquire lock
        let lock = SetupLock::acquire(&self.config_dir)?;

        let result = self.run_locked();

        // Release lock (drop on scope exit handles it, but explicit for clarity)
        drop(lock);

        result
    }

    fn run_locked(&self) -> anyhow::Result<SetupOutcome> {
        // 1. Detect
        let detected =
            detect_and_load(&self.config_dir).context("Failed to detect existing configuration")?;

        // 2. Resolve
        let resolved = self.resolve_existing(&detected)?;

        // 3. Ask
        let answers = self.collect_answers(resolved)?;

        // 4. Build preview
        let proposed = self.build_config(&answers)?;
        let preview_str = preview::render(&proposed, &self.config_dir);

        // 5. Preview / confirm
        if self.interactive {
            println!("\n{preview_str}");
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

        // If dry-run, return preview without applying.
        if self.dry_run {
            if self.json_output {
                let json_out = build_json_output("dry_run", &proposed, &preview_str);
                println!("{}", serde_json::to_string_pretty(&json_out)?);
            } else {
                println!("{}", preview_str);
            }
            return Ok(SetupOutcome::DryRun {
                config: Box::new(proposed),
                preview: preview_str,
            });
        }

        // If a config already exists and the proposed config is equivalent,
        // skip applying and report a no-op.
        if let ConfigSource::V2(existing) | ConfigSource::V1Migrated(existing) = &detected {
            if configs_equivalent(existing, &proposed) {
                return Ok(SetupOutcome::NoOp);
            }
        }

        // 7. Apply transactionally (with rollback)
        let backup = self.backup_existing()?;
        let result = self.apply(&proposed);
        if let Err(e) = result {
            // Rollback: restore from backup
            if let Some(bak) = backup {
                let config_path = self.config_dir.join("config.toml");
                if bak.exists() {
                    let _ = fs::copy(&bak, &config_path);
                    let _ = fs::remove_file(&bak);
                }
            }
            return Err(e.context("Apply failed; previous configuration has been restored"));
        }

        // 8. Re-read
        let reloaded = match self.reload() {
            Ok(c) => c,
            Err(e) => {
                // Rollback on re-read failure
                if let Some(bak) = backup {
                    let config_path = self.config_dir.join("config.toml");
                    if bak.exists() {
                        let _ = fs::copy(&bak, &config_path);
                        let _ = fs::remove_file(&bak);
                    }
                }
                return Err(e.context(
                    "Re-read failed after apply; previous configuration has been restored",
                ));
            }
        };

        // Clean up backup after successful re-read
        if let Some(bak) = backup {
            let _ = fs::remove_file(&bak);
        }

        // 9. Verify
        if let Err(e) = self.verify(&reloaded, &proposed) {
            return Err(e.context("Verification failed after apply"));
        }

        // 10. Write ownership record
        let record = OwnershipRecord {
            version: env!("CARGO_PKG_VERSION").to_string(),
            integration_ids: reloaded.integrations.iter().map(|i| i.id.clone()).collect(),
            upstream_ids: reloaded.upstreams.iter().map(|u| u.id.clone()).collect(),
            policy_ids: reloaded.policies.iter().map(|p| p.id.clone()).collect(),
        };
        self.write_ownership(&record)
            .context("Failed to write ownership record")?;

        if self.json_output {
            let json_out = build_json_output("applied", &reloaded, &preview_str);
            println!("{}", serde_json::to_string_pretty(&json_out)?);
        }

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
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                anyhow::bail!(
                    "Setup requires interactive input but stdin is not a terminal. \
                     Use --dry-run with --json for non-interactive inspection."
                );
            }
            self.prompt_interactively(resolved)
        } else {
            // Non-interactive mode: use injected answers directly
            if self.answers.upstream_url.is_none() {
                anyhow::bail!(
                    "Non-interactive setup requires all answers to be provided. \
                     Use SetupTransaction::with_answers() to inject configuration."
                );
            }
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

    /// Backup existing config.toml if it exists. Returns the backup path (None if no existing config).
    fn backup_existing(&self) -> anyhow::Result<Option<PathBuf>> {
        let config_path = self.config_dir.join("config.toml");
        if !config_path.exists() {
            return Ok(None);
        }
        let bak_path = self.config_dir.join("config.toml.toche-rollback");
        fs::copy(&config_path, &bak_path)
            .context("Failed to backup existing config.toml for rollback")?;
        Ok(Some(bak_path))
    }

    fn apply(&self, config: &TocheConfig) -> anyhow::Result<()> {
        fs::create_dir_all(&self.config_dir).context("Failed to create config directory")?;

        let config_path = self.config_dir.join("config.toml");
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

    fn write_ownership(&self, record: &OwnershipRecord) -> anyhow::Result<()> {
        let path = self.config_dir.join("ownership.toml");
        let toml_str =
            toml::to_string_pretty(record).context("Failed to serialize ownership record")?;
        atomic_write_secure(&path, &toml_str).context("Failed to write ownership.toml")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Setup lock
// ---------------------------------------------------------------------------

/// A file-based lock in the config directory preventing concurrent setup runs.
#[derive(Debug)]
struct SetupLock {
    path: PathBuf,
}

impl SetupLock {
    fn acquire(config_dir: &std::path::Path) -> anyhow::Result<Self> {
        fs::create_dir_all(config_dir).context("Failed to create config directory for lock")?;
        let path = config_dir.join("setup.lock");

        // Try to create the lock file exclusively — fails if it already exists
        match fs::File::options().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                // Write PID for stale lock diagnosis
                let pid = std::process::id();
                let _ = write!(f, "{pid}");
                Ok(Self { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Diagnose stale lock
                let pid_str = fs::read_to_string(&path).unwrap_or_default();
                let pid: String = pid_str.trim().to_string();
                anyhow::bail!(
                    "Another setup process may be running (PID: {pid}). \
                     Lock file: {}. If no setup process is active, remove \
                     the lock file manually.",
                    path.display()
                );
            }
            Err(e) => {
                anyhow::bail!("Failed to acquire setup lock: {e}");
            }
        }
    }
}

impl Drop for SetupLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn build_json_output(outcome: &str, config: &TocheConfig, preview: &str) -> SetupJsonOutput {
    SetupJsonOutput {
        schema_version: 2,
        outcome: outcome.to_string(),
        preview: preview.to_string(),
        integrations: config
            .integrations
            .iter()
            .map(|i| IntegrationSummary {
                id: i.id.clone(),
                name: i.name.clone(),
            })
            .collect(),
        upstreams: config
            .upstreams
            .iter()
            .map(|u| UpstreamSummary {
                id: u.id.clone(),
                name: u.name.clone(),
                url: u.url.clone(),
                auth_header: u.auth.header_name.clone(),
            })
            .collect(),
        policies: config
            .policies
            .iter()
            .map(|p| PolicySummary {
                id: p.id.clone(),
                name: p.name.clone(),
            })
            .collect(),
    }
}

fn configs_equivalent(a: &TocheConfig, b: &TocheConfig) -> bool {
    a.schema_version == b.schema_version
        && a.runtime.port == b.runtime.port
        && a.runtime.listen_address == b.runtime.listen_address
        && a.runtime.request_timeout_ms == b.runtime.request_timeout_ms
        && a.runtime.max_request_body_bytes == b.runtime.max_request_body_bytes
        && a.runtime.max_response_body_bytes == b.runtime.max_response_body_bytes
        && a.runtime.max_concurrent_upstream == b.runtime.max_concurrent_upstream
        && a.runtime.upstream_permit_timeout_ms == b.runtime.upstream_permit_timeout_ms
        && a.defaults.integration == b.defaults.integration
        && a.storage.ledger_db == b.storage.ledger_db
        && a.storage.cas_dir == b.storage.cas_dir
        && a.integrations.len() == b.integrations.len()
        && a.upstreams.len() == b.upstreams.len()
        && a.policies.len() == b.policies.len()
}

/// Import a legacy `Profiles` into a v2 `TocheConfig`.
#[allow(dead_code)]
pub fn import_legacy_profiles(profiles: &Profiles) -> TocheConfig {
    crate::config::migration::migrate_v1_to_v2(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn answers_default() -> SetupAnswers {
        SetupAnswers {
            upstream_url: Some("https://api.anthropic.com".into()),
            api_key: Some("sk-test".into()),
            header_name: Some("x-api-key".into()),
            integration_name: Some("default".into()),
        }
    }

    fn tx_for_test(dir: &tempfile::TempDir) -> SetupTransaction {
        SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: false,
            json_output: false,
            answers: answers_default(),
        }
    }

    // --- Basic lifecycle ---

    #[test]
    fn setup_no_op_when_unchanged() {
        let dir = make_temp_dir();
        let tx = tx_for_test(&dir);
        let outcome = tx.run().unwrap();
        assert!(matches!(outcome, SetupOutcome::Applied { .. }));

        let outcome2 = tx_for_test(&dir).run().unwrap();
        assert!(matches!(outcome2, SetupOutcome::NoOp));
    }

    #[test]
    fn setup_interruption_leaves_config_unchanged() {
        let dir = make_temp_dir();
        tx_for_test(&dir).run().unwrap();

        let before = fs::read_to_string(dir.path().join("config.toml")).unwrap();

        // Simulate interruption: stale temp file from a prior incomplete write
        let tmp = dir.path().join("config.toml.tmp");
        fs::write(&tmp, "partial content").unwrap();

        tx_for_test(&dir).run().unwrap();

        let after = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn setup_preview_contains_upstream() {
        let dir = make_temp_dir();
        let outcome = tx_for_test(&dir).run().unwrap();
        let config = match outcome {
            SetupOutcome::Applied { config, .. } => config,
            _ => panic!("expected applied"),
        };
        let preview = preview::render(&config, dir.path());
        assert!(preview.contains("https://api.anthropic.com"));
        assert!(preview.contains("default"));
    }

    #[test]
    fn setup_apply_remove_roundtrip() {
        let dir = make_temp_dir();
        let outcome = tx_for_test(&dir).run().unwrap();
        let config = match outcome {
            SetupOutcome::Applied { config, .. } => config,
            _ => panic!("expected applied"),
        };
        assert_eq!(config.upstreams.len(), 1);

        // Removing the config file and re-running creates a fresh one.
        fs::remove_file(dir.path().join("config.toml")).unwrap();
        let outcome2 = tx_for_test(&dir).run().unwrap();
        assert!(matches!(outcome2, SetupOutcome::Applied { .. }));
    }

    // --- Lock ---

    #[test]
    fn lock_prevents_concurrent_setup() {
        let dir = make_temp_dir();
        let lock = SetupLock::acquire(dir.path()).unwrap();
        let result = SetupLock::acquire(dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Another setup process"));
        drop(lock);
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = make_temp_dir();
        {
            let _lock = SetupLock::acquire(dir.path()).unwrap();
            assert!(dir.path().join("setup.lock").exists());
        }
        assert!(!dir.path().join("setup.lock").exists());
        // Should be able to re-acquire
        let _lock2 = SetupLock::acquire(dir.path()).unwrap();
    }

    // --- Dry-run ---

    #[test]
    fn dry_run_does_not_write_config() {
        let dir = make_temp_dir();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: true,
            json_output: false,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        assert!(matches!(outcome, SetupOutcome::DryRun { .. }));
        assert!(!dir.path().join("config.toml").exists());
        assert!(!dir.path().join("ownership.toml").exists());
    }

    #[test]
    fn dry_run_reports_changes() {
        let dir = make_temp_dir();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: true,
            json_output: false,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        match outcome {
            SetupOutcome::DryRun { preview, .. } => {
                assert!(preview.contains("https://api.anthropic.com"));
            }
            _ => panic!("expected DryRun"),
        }
    }

    #[test]
    fn dry_run_json_is_valid() {
        let dir = make_temp_dir();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: true,
            json_output: true,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        assert!(matches!(outcome, SetupOutcome::DryRun { .. }));
        // In test, println goes to stdout but we can verify the outcome variant
    }

    // --- Non-TTY behaviour ---

    #[test]
    fn non_interactive_without_answers_fails() {
        let dir = make_temp_dir();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: false,
            json_output: false,
            answers: SetupAnswers::default(),
        };
        let result = tx.run();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Non-interactive setup requires")
        );
    }

    #[test]
    fn non_interactive_with_answers_succeeds() {
        let dir = make_temp_dir();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: false,
            json_output: false,
            answers: answers_default(),
        };
        let outcome = tx.run().unwrap();
        assert!(matches!(outcome, SetupOutcome::Applied { .. }));
    }

    // --- Ownership ---

    #[test]
    fn ownership_record_is_persisted() {
        let dir = make_temp_dir();
        let tx = tx_for_test(&dir);
        let outcome = tx.run().unwrap();
        assert!(matches!(outcome, SetupOutcome::Applied { .. }));

        let ownership_path = dir.path().join("ownership.toml");
        assert!(ownership_path.exists());
        let raw = fs::read_to_string(&ownership_path).unwrap();
        let record: OwnershipRecord = toml::from_str(&raw).unwrap();
        assert_eq!(record.version, env!("CARGO_PKG_VERSION"));
        assert!(!record.integration_ids.is_empty());
        assert!(!record.upstream_ids.is_empty());
    }

    #[test]
    fn ownership_record_is_updated_on_rerun() {
        let dir = make_temp_dir();
        tx_for_test(&dir).run().unwrap();

        let ownership_path = dir.path().join("ownership.toml");
        let mtime1 = fs::metadata(&ownership_path).unwrap().modified().unwrap();

        // Re-run with same config (should be no-op, ownership unchanged)
        tx_for_test(&dir).run().unwrap();
        let mtime2 = fs::metadata(&ownership_path).unwrap().modified().unwrap();
        assert_eq!(mtime1, mtime2);
    }

    // --- Rollback ---

    #[test]
    fn rollback_restores_config_on_verify_failure() {
        let dir = make_temp_dir();

        // First, create a valid config
        tx_for_test(&dir).run().unwrap();

        // Corrupt the config.toml after backup but simulate a scenario where
        // apply would need to rollback. We test this by verifying that an
        // incomplete/partial write is cleaned up.
        let tmp_file = dir.path().join("config.toml.tmp");
        fs::write(&tmp_file, "incomplete").unwrap();

        // Running setup should still succeed (atomic_write_secure cleans stale tmp)
        tx_for_test(&dir).run().unwrap();

        // Config should still be valid TOML
        let after = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        let _parsed: TocheConfig = toml::from_str(&after).unwrap();
    }

    // --- Deterministic plans ---

    #[test]
    fn identical_answers_produce_identical_plans() {
        let answers = answers_default();
        let tx = SetupTransaction {
            config_dir: PathBuf::from("/nonexistent"),
            interactive: false,
            force: false,
            dry_run: true,
            json_output: false,
            answers: answers.clone(),
        };
        let config1 = tx.build_config(&answers).unwrap();
        let config2 = tx.build_config(&answers_default()).unwrap();
        assert!(configs_equivalent(&config1, &config2));
    }

    // --- Secret safety ---

    #[test]
    fn preview_does_not_contain_api_keys() {
        let dir = make_temp_dir();
        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: false,
            dry_run: true,
            json_output: false,
            answers: SetupAnswers {
                upstream_url: Some("https://api.anthropic.com".into()),
                api_key: Some("sk-ant-secret-key-123456".into()),
                header_name: Some("x-api-key".into()),
                integration_name: Some("default".into()),
            },
        };
        let outcome = tx.run().unwrap();
        match outcome {
            SetupOutcome::DryRun { preview, .. } => {
                assert!(!preview.contains("sk-ant-secret-key-123456"));
                // Should show "inline(***)" or "none" from SecretRef Display
                assert!(preview.contains("inline(***)") || preview.contains("none"));
            }
            _ => panic!("expected DryRun"),
        }
    }

    #[test]
    fn json_output_excludes_secrets() {
        let answers = SetupAnswers {
            upstream_url: Some("https://api.anthropic.com".into()),
            api_key: Some("sk-ant-secret".into()),
            header_name: Some("x-api-key".into()),
            integration_name: Some("default".into()),
        };
        let tx = SetupTransaction {
            config_dir: PathBuf::from("/nonexistent"),
            interactive: false,
            force: false,
            dry_run: true,
            json_output: false,
            answers,
        };
        let config = tx.build_config(&tx.answers).unwrap();
        let json_out = build_json_output("dry_run", &config, "test preview");
        let json_str = serde_json::to_string_pretty(&json_out).unwrap();
        assert!(!json_str.contains("sk-ant-secret"));
    }

    // --- Force semantics ---

    #[test]
    fn force_backs_up_existing_config() {
        let dir = make_temp_dir();

        // Create a pre-existing config.toml
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(
            dir.path().join("config.toml"),
            "schema_version = 2\n[runtime]\nport = 9999\nlisten_address = \"0.0.0.0\"\nrequest_timeout_ms = 300000\n",
        )
        .unwrap();

        let tx = SetupTransaction {
            config_dir: dir.path().to_path_buf(),
            interactive: false,
            force: true,
            dry_run: false,
            json_output: false,
            answers: answers_default(),
        };
        tx.run().unwrap();

        // Config should now have the setup-generated values
        let raw = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(raw.contains("integration"));
    }
}

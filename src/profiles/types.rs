use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Controls how Toche manages provider prompt caching for a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    /// Log what breakpoints would be set, but don't modify requests.
    Observe,
    /// Inject cache_control breakpoints into outgoing requests.
    Auto,
}

impl Default for CacheMode {
    fn default() -> Self {
        Self::Observe
    }
}

/// Which parts of the conversation get cache breakpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheBreakpoint {
    /// Cache the system prompt and consecutive non-tool user+assistant message runs.
    Standard,
    /// Cache only the system prompt.
    SystemOnly,
}

impl Default for CacheBreakpoint {
    fn default() -> Self {
        Self::Standard
    }
}

/// Per-profile cache coordination configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Whether cache coordination is enabled for this profile.
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    /// Observe (log only) or Auto (inject breakpoints).
    #[serde(default)]
    pub mode: CacheMode,
    /// Which parts of the conversation to cache.
    #[serde(default)]
    pub breakpoint: CacheBreakpoint,
}

fn default_cache_enabled() -> bool {
    true
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: CacheMode::Observe,
            breakpoint: CacheBreakpoint::Standard,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub upstream_url: String,
    pub auth_method: AuthMethod,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub models: HashMap<String, String>,
    /// Optional per-profile cache coordination configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthMethod {
    #[serde(rename = "api_key")]
    ApiKey {
        header_name: String,
        key: String,
    },
    #[serde(rename = "bearer")]
    BearerToken {
        token: String,
    },
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profiles {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

impl Profiles {
    pub fn default_profile(&self) -> Option<&Profile> {
        if let Some(ref name) = self.default {
            self.profiles.iter().find(|p| &p.name == name)
        } else {
            self.profiles.first()
        }
    }
}

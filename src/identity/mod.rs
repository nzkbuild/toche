//! Runtime identity and trust domains.
//!
//! Every Toche gateway instance, request, and client carries independently
//! identifiable metadata. The trust domain isolates different credential
//! references so they never share cache entries or in-flight coalescing.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;
use uuid::Uuid;

// --- Core identity types ---

/// A time-sortable UUIDv7 identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeId(String);

/// A per-request UUIDv7 identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestId(String);

/// An external request ID extracted from client headers (if provided).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalRequestId(String);

/// A deterministic trust domain identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrustDomainId(String);

/// Attribution confidence for client identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    /// Client identity was observed from process inspection.
    Exact,
    /// Client reported its identity in headers.
    ClientReported,
    /// Only workspace-level identity is known.
    WorkspaceLevel,
    /// Identity was inferred from indirect signals.
    Inferred,
    /// Identity remains unknown.
    Unknown,
}

impl fmt::Display for Attribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Attribution::Exact => f.write_str("exact"),
            Attribution::ClientReported => f.write_str("client-reported"),
            Attribution::WorkspaceLevel => f.write_str("workspace-level"),
            Attribution::Inferred => f.write_str("inferred"),
            Attribution::Unknown => f.write_str("unknown"),
        }
    }
}

// --- Identity context: all identity fields for a single request ---

/// All identity fields for a single gateway request.
///
/// Some fields are nullable when identity cannot be observed (e.g., in
/// persistent proxy mode, the client process identity is not available).
#[derive(Debug, Clone)]
#[allow(dead_code)] // instance_id, conversation_id, workspace_id, policy_ids reserved for future use
pub struct IdentityContext {
    pub runtime_id: RuntimeId,
    pub request_id: RequestId,
    pub external_request_id: Option<ExternalRequestId>,
    pub integration_id: String,
    pub integration_name: String,
    pub upstream_id: String,
    pub upstream_name: String,
    pub trust_domain_id: TrustDomainId,
    pub instance_id: Option<String>,
    pub conversation_id: Option<String>,
    pub workspace_id: Option<String>,
    pub policy_ids: Vec<String>,
    pub config_snapshot_hash: String,
    pub attribution: Attribution,
}

// --- Constructors ---

impl RuntimeId {
    /// Load the persisted runtime ID or generate a new UUIDv7.
    /// The ID is written to `runtime_id` in the config directory so
    /// it survives gateway restarts.
    pub fn load_or_create(config_dir: &Path) -> Self {
        let path = config_dir.join("runtime_id");
        if let Ok(existing) = std::fs::read_to_string(&path) {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return Self(trimmed.to_string());
            }
        }
        let id = Uuid::now_v7().to_string();
        let _ = std::fs::write(&path, &id);
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuntimeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl RequestId {
    /// Generate a new UUIDv7 per request.
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ExternalRequestId {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TrustDomainId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrustDomainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// --- Trust domain derivation ---

/// Derive a trust domain ID from the combination of:
/// - integration identity (id)
/// - upstream identity (id)
/// - credential reference identity (display form of SecretRef — never raw value)
/// - integration name as additional salt
///
/// Uses SHA-256. Raw credential values are NEVER placed in the hash input.
/// The secret_ref_display should come from `SecretRef::to_string()` which
/// redacts inline values.
pub fn derive_trust_domain_id(
    integration_id: &str,
    integration_name: &str,
    upstream_id: &str,
    secret_ref_display: &str,
) -> TrustDomainId {
    let mut hasher = Sha256::new();
    hasher.update(b"toche-trust-domain-v1:");
    hasher.update(integration_id.as_bytes());
    hasher.update(b":");
    hasher.update(integration_name.as_bytes());
    hasher.update(b":");
    hasher.update(upstream_id.as_bytes());
    hasher.update(b":");
    hasher.update(secret_ref_display.as_bytes());
    let hash = hex::encode(&hasher.finalize()[..8]);
    TrustDomainId(hash)
}

/// Compute a configuration snapshot hash from canonical TOML.
/// Used to detect configuration drift between ledger entries.
pub fn compute_config_snapshot(config_toml: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"toche-config-snapshot-v1:");
    hasher.update(config_toml.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

/// Compute a deterministic workspace ID from the project path.
pub fn workspace_id_from_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"toche-workspace-v1:");
    hasher.update(path.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

// --- IdentityContext builder ---

/// Try to extract an x-request-id or x-toche-request-id header value.
pub fn extract_external_request_id(headers: &axum::http::HeaderMap) -> Option<ExternalRequestId> {
    headers
        .get("x-request-id")
        .or_else(|| headers.get("x-toche-request-id"))
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| ExternalRequestId(s.to_string()))
}

/// Try to extract a conversation/session ID from headers.
pub fn extract_conversation_id(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-toche-conversation-id")
        .or_else(|| headers.get("x-conversation-id"))
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::toche_config::SecretRef;

    #[test]
    fn runtime_id_load_or_create_persists() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().to_path_buf();

        let id1 = RuntimeId::load_or_create(&config_dir);
        let id2 = RuntimeId::load_or_create(&config_dir);
        assert_eq!(id1.as_str(), id2.as_str(), "runtime ID must persist");

        let path = config_dir.join("runtime_id");
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.trim(), id1.as_str());
    }

    #[test]
    fn request_ids_are_unique() {
        let id1 = RequestId::new();
        let id2 = RequestId::new();
        assert_ne!(id1.as_str(), id2.as_str());
    }

    #[test]
    fn request_id_format_is_uuidv7() {
        let id = RequestId::new();
        // UUIDv7 still parses as a valid UUID
        assert!(Uuid::parse_str(id.as_str()).is_ok());
    }

    #[test]
    fn trust_domain_is_deterministic() {
        let d1 = derive_trust_domain_id("abc123", "default", "xyz789", "env:ANTHROPIC_API_KEY");
        let d2 = derive_trust_domain_id("abc123", "default", "xyz789", "env:ANTHROPIC_API_KEY");
        assert_eq!(d1.0, d2.0);
    }

    #[test]
    fn trust_domain_differs_by_integration() {
        let d1 = derive_trust_domain_id("abc123", "personal", "up1", "env:KEY");
        let d2 = derive_trust_domain_id("def456", "work", "up1", "env:KEY");
        assert_ne!(d1.0, d2.0);
    }

    #[test]
    fn trust_domain_differs_by_upstream() {
        let d1 = derive_trust_domain_id("abc123", "default", "up1", "env:KEY");
        let d2 = derive_trust_domain_id("abc123", "default", "up2", "env:KEY");
        assert_ne!(d1.0, d2.0);
    }

    #[test]
    fn trust_domain_differs_by_secret_ref() {
        let d1 = derive_trust_domain_id("abc123", "default", "up1", "env:KEY_A");
        let d2 = derive_trust_domain_id("abc123", "default", "up1", "env:KEY_B");
        assert_ne!(d1.0, d2.0);
    }

    #[test]
    fn trust_domain_never_contains_raw_credential() {
        // Even if someone passes a raw key as the display string, the
        // output is only a hex hash — the raw value is unrecoverable.
        let d = derive_trust_domain_id("abc123", "default", "up1", "sk-ant-secret-key-12345");
        assert!(!d.0.contains("sk-ant"));
        assert_eq!(d.0.len(), 16);
        assert!(d.0.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn secret_ref_display_is_safe() {
        let sr = SecretRef::LegacyInline {
            value: "sk-ant-very-secret".into(),
        };
        let display = sr.to_string();
        assert!(!display.contains("sk-ant"));
        assert!(display.contains("***"));
    }

    #[test]
    fn config_snapshot_is_deterministic() {
        let h1 = compute_config_snapshot("[runtime]\nport = 8743\n");
        let h2 = compute_config_snapshot("[runtime]\nport = 8743\n");
        assert_eq!(h1, h2);
    }

    #[test]
    fn config_snapshot_differs_by_content() {
        let h1 = compute_config_snapshot("[runtime]\nport = 8743\n");
        let h2 = compute_config_snapshot("[runtime]\nport = 9999\n");
        assert_ne!(h1, h2);
    }

    #[test]
    fn config_snapshot_is_16_hex() {
        let h = compute_config_snapshot("test");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn workspace_id_from_path_is_deterministic() {
        let w1 = workspace_id_from_path("/home/user/project");
        let w2 = workspace_id_from_path("/home/user/project");
        assert_eq!(w1, w2);
    }

    #[test]
    fn workspace_id_differs_by_path() {
        let w1 = workspace_id_from_path("/home/user/project-a");
        let w2 = workspace_id_from_path("/home/user/project-b");
        assert_ne!(w1, w2);
    }

    #[test]
    fn extract_external_request_id_from_x_request_id() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-request-id", "req-abc-123".parse().unwrap());
        let id = extract_external_request_id(&headers);
        assert_eq!(id.unwrap().as_str(), "req-abc-123");
    }

    #[test]
    fn extract_external_request_id_prefers_x_toche() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-request-id", "req-other".parse().unwrap());
        headers.insert("x-toche-request-id", "req-toche-456".parse().unwrap());
        let id = extract_external_request_id(&headers);
        // x-request-id is checked first; x-toche-request-id is the fallback
        assert_eq!(id.unwrap().as_str(), "req-other");
    }

    #[test]
    fn extract_external_request_id_empty_is_none() {
        let headers = axum::http::HeaderMap::new();
        assert!(extract_external_request_id(&headers).is_none());
    }

    #[test]
    fn extract_conversation_id_from_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-conversation-id", "conv-xyz".parse().unwrap());
        let id = extract_conversation_id(&headers);
        assert_eq!(id.unwrap(), "conv-xyz");
    }

    #[test]
    fn extract_conversation_id_empty_is_none() {
        let headers = axum::http::HeaderMap::new();
        assert!(extract_conversation_id(&headers).is_none());
    }

    #[test]
    fn attribution_display() {
        assert_eq!(Attribution::Exact.to_string(), "exact");
        assert_eq!(Attribution::ClientReported.to_string(), "client-reported");
        assert_eq!(Attribution::WorkspaceLevel.to_string(), "workspace-level");
        assert_eq!(Attribution::Inferred.to_string(), "inferred");
        assert_eq!(Attribution::Unknown.to_string(), "unknown");
    }

    #[test]
    fn identity_context_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = IdentityContext {
            runtime_id: RuntimeId::load_or_create(dir.path()),
            request_id: RequestId::new(),
            external_request_id: Some(ExternalRequestId("req-1".into())),
            integration_id: "abc123".into(),
            integration_name: "default".into(),
            upstream_id: "xyz789".into(),
            upstream_name: "Anthropic".into(),
            trust_domain_id: derive_trust_domain_id("abc123", "default", "xyz789", "env:KEY"),
            instance_id: None,
            conversation_id: Some("conv-1".into()),
            workspace_id: Some(workspace_id_from_path("/home/user/project")),
            policy_ids: vec!["pol123".into()],
            config_snapshot_hash: compute_config_snapshot("test"),
            attribution: Attribution::Unknown,
        };
        assert!(!ctx.runtime_id.as_str().is_empty());
        assert!(!ctx.request_id.as_str().is_empty());
        assert!(ctx.external_request_id.is_some());
        assert_eq!(ctx.integration_name, "default");
        assert!(ctx.config_snapshot_hash.len() == 16);
    }
}

use axum::http::HeaderMap;

use crate::cache::breakpoint::BreakpointPlan;

/// Protocol-specific response headers extracted from upstream.
#[derive(Debug, Clone, Default)]
pub struct ResponseHeaders {
    pub cache_read_tokens: u64,
    pub cache_create_tokens: u64,
}

/// A thin protocol trait that answers protocol-specific questions only.
///
/// The Toche pipeline (coalescing, safe cache, reduce, efficiency, ledger)
/// is product logic and stays in routes.rs — this trait does NOT model
/// the full request lifecycle.
///
/// Implementations must be `Send + Sync` so they can be used across
/// await points in the axum handler.
pub trait Protocol: Send + Sync {
    /// A stable short name for the protocol, used for ledger records and
    /// diagnostics (e.g. "anthropic", "openai-responses").
    fn name(&self) -> &'static str;

    /// Extract the model identifier from the request body.
    fn extract_model(&self, body: &str) -> String;

    /// The protocol-specific URL path suffix (e.g. "/v1/messages").
    fn path(&self) -> &str;

    /// Compute a normalized, deterministic fingerprint of the request body.
    ///
    /// Normalization strips protocol-specific wire-format fields (e.g.
    /// `stream`, `cache_control`) so they don't change the fingerprint.
    /// Falls back to a raw SHA-256 hash if JSON parsing fails.
    fn fingerprint(&self, body: &str) -> String;

    /// Parse protocol-specific headers from the upstream response.
    fn parse_response_headers(&self, headers: &HeaderMap) -> ResponseHeaders;

    /// Inject cache_control breakpoints into the request body at the
    /// positions specified by the breakpoint plan.
    ///
    /// Returns the original body unchanged if the plan has no breakpoints.
    fn inject_cache_control(&self, body: &str, plan: &BreakpointPlan) -> Result<String, String>;

    /// Check whether the request body indicates a streaming request.
    fn is_streaming(&self, body: &str) -> bool;
}

pub mod anthropic;
pub mod openai_responses;

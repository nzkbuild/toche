use axum::http::HeaderMap;

use crate::cache;
use crate::cache::breakpoint::BreakpointPlan;
use crate::protocol::{Protocol, ResponseHeaders};
use crate::shield;

/// Protocol driver for the Anthropic Messages API.
///
/// Stateless — all methods are pure functions of their inputs.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnthropicProtocol;

impl Protocol for AnthropicProtocol {
    fn extract_model(&self, body: &str) -> String {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("model")?.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn path(&self) -> &str {
        "/v1/messages"
    }

    fn fingerprint(&self, body: &str) -> String {
        shield::fingerprint::compute(body)
    }

    fn parse_response_headers(&self, headers: &HeaderMap) -> ResponseHeaders {
        let cache_read: u64 = headers
            .get("anthropic-cache-read-input-tokens")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let cache_create: u64 = headers
            .get("anthropic-cache-creation-input-tokens")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        ResponseHeaders {
            cache_read_tokens: cache_read,
            cache_create_tokens: cache_create,
        }
    }

    fn inject_cache_control(&self, body: &str, plan: &BreakpointPlan) -> Result<String, String> {
        cache::inject::inject_cache_control(body, plan)
    }

    fn is_streaming(&self, body: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("stream")?.as_bool())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_model ---

    #[test]
    fn extract_model_present() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[]}"#;
        assert_eq!(proto.extract_model(body), "claude-sonnet-5");
    }

    #[test]
    fn extract_model_with_slash() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"cx/gpt-5.6-sol","messages":[]}"#;
        assert_eq!(proto.extract_model(body), "cx/gpt-5.6-sol");
    }

    #[test]
    fn extract_model_missing() {
        let proto = AnthropicProtocol;
        let body = r#"{"max_tokens":1024}"#;
        assert_eq!(proto.extract_model(body), "unknown");
    }

    #[test]
    fn extract_model_empty_body() {
        let proto = AnthropicProtocol;
        assert_eq!(proto.extract_model(""), "unknown");
    }

    #[test]
    fn extract_model_date_suffix_preserved() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5-20251001","messages":[]}"#;
        assert_eq!(proto.extract_model(body), "claude-sonnet-5-20251001");
    }

    #[test]
    fn extract_model_key_in_string_value_not_matched() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"which model to use"}]}"#;
        assert_eq!(proto.extract_model(body), "claude-sonnet-5");
    }

    #[test]
    fn extract_model_with_escaped_quotes() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","messages":[{"role":"user","content":"he said: \"which model?\""}]}"#;
        assert_eq!(proto.extract_model(body), "claude-sonnet-5");
    }

    // --- path ---

    #[test]
    fn path_is_messages() {
        assert_eq!(AnthropicProtocol.path(), "/v1/messages");
    }

    // --- fingerprint ---

    #[test]
    fn fingerprint_delegates_to_shield() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[]}"#;
        let fp = proto.fingerprint(body);
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","stream":true,"messages":[]}"#;
        assert_eq!(proto.fingerprint(body), proto.fingerprint(body));
    }

    // --- parse_response_headers ---

    #[test]
    fn parse_response_headers_no_cache_headers() {
        let proto = AnthropicProtocol;
        let headers = HeaderMap::new();
        let result = proto.parse_response_headers(&headers);
        assert_eq!(result.cache_read_tokens, 0);
        assert_eq!(result.cache_create_tokens, 0);
    }

    #[test]
    fn parse_response_headers_with_cache_tokens() {
        use axum::http::HeaderName;
        use axum::http::HeaderValue;

        let proto = AnthropicProtocol;
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("anthropic-cache-read-input-tokens"),
            HeaderValue::from_static("5000"),
        );
        headers.insert(
            HeaderName::from_static("anthropic-cache-creation-input-tokens"),
            HeaderValue::from_static("1000"),
        );
        let result = proto.parse_response_headers(&headers);
        assert_eq!(result.cache_read_tokens, 5000);
        assert_eq!(result.cache_create_tokens, 1000);
    }

    // --- is_streaming ---

    #[test]
    fn stream_true() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"stream":true,"messages":[]}"#;
        assert!(proto.is_streaming(body));
    }

    #[test]
    fn stream_false() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"stream":false,"messages":[]}"#;
        assert!(!proto.is_streaming(body));
    }

    #[test]
    fn stream_omitted_is_false() {
        let proto = AnthropicProtocol;
        let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[]}"#;
        assert!(!proto.is_streaming(body));
    }

    #[test]
    fn stream_malformed_is_false() {
        let proto = AnthropicProtocol;
        assert!(!proto.is_streaming("{broken"));
    }
}

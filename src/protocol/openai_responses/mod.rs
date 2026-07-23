use axum::http::HeaderMap;

use crate::cache::breakpoint::BreakpointPlan;
use crate::protocol::{Protocol, ResponseHeaders};
use crate::shield;

/// Protocol driver for the OpenAI Responses API.
///
/// Stateless — all methods are pure functions of their inputs.
/// Pass-through only in 1.1.0; no cache injection or Anthropic-specific
/// header parsing.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiResponsesProtocol;

impl Protocol for OpenAiResponsesProtocol {
    fn name(&self) -> &'static str {
        "openai-responses"
    }

    fn extract_model(&self, body: &str) -> String {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("model")?.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn path(&self) -> &str {
        "/v1/responses"
    }

    fn fingerprint(&self, body: &str) -> String {
        shield::fingerprint::compute(body)
    }

    fn parse_response_headers(&self, _headers: &HeaderMap) -> ResponseHeaders {
        ResponseHeaders::default()
    }

    fn inject_cache_control(&self, body: &str, _plan: &BreakpointPlan) -> Result<String, String> {
        Ok(body.to_string())
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
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","input":"Hello"}"#;
        assert_eq!(proto.extract_model(body), "gpt-5.6");
    }

    #[test]
    fn extract_model_custom_prefix() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"cx/gpt-5.6-sol","input":"Hello"}"#;
        assert_eq!(proto.extract_model(body), "cx/gpt-5.6-sol");
    }

    #[test]
    fn extract_model_missing() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"input":"Hello"}"#;
        assert_eq!(proto.extract_model(body), "unknown");
    }

    #[test]
    fn extract_model_empty_body() {
        let proto = OpenAiResponsesProtocol;
        assert_eq!(proto.extract_model(""), "unknown");
    }

    #[test]
    fn extract_model_malformed() {
        let proto = OpenAiResponsesProtocol;
        assert_eq!(proto.extract_model("{broken"), "unknown");
    }

    // --- name ---

    #[test]
    fn name_is_openai_responses() {
        assert_eq!(OpenAiResponsesProtocol.name(), "openai-responses");
    }

    // --- path ---

    #[test]
    fn path_is_responses() {
        assert_eq!(OpenAiResponsesProtocol.path(), "/v1/responses");
    }

    // --- fingerprint ---

    #[test]
    fn fingerprint_delegates_to_shield() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","input":"Hello"}"#;
        let fp = proto.fingerprint(body);
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","stream":true,"input":"Hello"}"#;
        assert_eq!(proto.fingerprint(body), proto.fingerprint(body));
    }

    // --- parse_response_headers ---

    #[test]
    fn parse_response_headers_always_zero() {
        let proto = OpenAiResponsesProtocol;
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_static("x-some-header"),
            axum::http::HeaderValue::from_static("42"),
        );
        let result = proto.parse_response_headers(&headers);
        assert_eq!(result.cache_read_tokens, 0);
        assert_eq!(result.cache_create_tokens, 0);
    }

    #[test]
    fn parse_response_headers_empty() {
        let proto = OpenAiResponsesProtocol;
        let headers = HeaderMap::new();
        let result = proto.parse_response_headers(&headers);
        assert_eq!(result.cache_read_tokens, 0);
        assert_eq!(result.cache_create_tokens, 0);
    }

    // --- inject_cache_control ---

    #[test]
    fn inject_cache_control_returns_body_unchanged() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","input":"Hello"}"#;
        let plan = BreakpointPlan {
            system_block_index: None,
            message_blocks: Vec::new(),
        };
        let result = proto.inject_cache_control(body, &plan).unwrap();
        assert_eq!(result, body);
    }

    // --- is_streaming ---

    #[test]
    fn stream_true() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","stream":true,"input":"Hello"}"#;
        assert!(proto.is_streaming(body));
    }

    #[test]
    fn stream_false() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","stream":false,"input":"Hello"}"#;
        assert!(!proto.is_streaming(body));
    }

    #[test]
    fn stream_omitted_is_false() {
        let proto = OpenAiResponsesProtocol;
        let body = r#"{"model":"gpt-5.6","input":"Hello"}"#;
        assert!(!proto.is_streaming(body));
    }

    #[test]
    fn stream_malformed_is_false() {
        let proto = OpenAiResponsesProtocol;
        assert!(!proto.is_streaming("{broken"));
    }
}

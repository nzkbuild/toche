use toche::shield;

#[test]
fn fingerprint_deterministic() {
    let body = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}"#;
    let a = shield::fingerprint::compute(body);
    let b = shield::fingerprint::compute(body);
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
}

#[test]
fn cache_control_does_not_change_fingerprint() {
    let without =
        r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":[{"type":"text","text":"Hello"}]}]}"#;
    let with = r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":[{"type":"text","text":"Hello","cache_control":{"type":"ephemeral"}}]}]}"#;
    assert_eq!(
        shield::fingerprint::compute(without),
        shield::fingerprint::compute(with)
    );
}

#[test]
fn different_models_different_fingerprints() {
    let body_a =
        r#"{"model":"claude-sonnet-5","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}"#;
    let body_b = r#"{"model":"claude-opus-4.8","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}"#;
    assert_ne!(
        shield::fingerprint::compute(body_a),
        shield::fingerprint::compute(body_b)
    );
}

#[test]
fn stream_field_does_not_change_fingerprint() {
    let with_stream =
        r#"{"model":"claude-sonnet-5","max_tokens":1024,"stream":true,"messages":[]}"#;
    let without_stream =
        r#"{"model":"claude-sonnet-5","max_tokens":1024,"stream":false,"messages":[]}"#;
    assert_eq!(
        shield::fingerprint::compute(with_stream),
        shield::fingerprint::compute(without_stream)
    );
}

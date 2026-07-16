use toche::reduce::config::ReduceConfig;
use toche::reduce::storage;
use toche::reduce::transform;

fn make_config() -> ReduceConfig {
    ReduceConfig {
        enabled: true,
        command_bypass: vec![],
    }
}

fn load_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/reduce/{name}.json");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"))
}

#[test]
fn unsupported_command_passthrough() {
    let body = load_fixture("tool_result_unknown_cmd");
    let r = transform::reduce_body(&body, &make_config(), false).expect("reduce should succeed");
    assert_eq!(r.reductions, 0);
    assert_eq!(r.passthroughs, 1);
    // Content preserved (though JSON serialization may reorder keys)
    let parsed: serde_json::Value = serde_json::from_str(&r.modified_body).unwrap();
    let content = &parsed["messages"][2]["content"][0]["content"];
    let text = content.as_str().unwrap();
    assert!(text.contains("custom script output"));
    assert!(text.contains("pass through unchanged"));
}

#[test]
fn shellcheck_output_reduced() {
    let body = load_fixture("tool_result_cargo_test");
    let r = transform::reduce_body(&body, &make_config(), false).expect("reduce should succeed");
    assert!(
        r.reductions > 0,
        "shellcheck should match a filter and be reduced"
    );
    assert!(
        r.modified_body.contains("toche:reduced"),
        "should contain reduction marker"
    );

    // Reduced output should retain the warning info but be shorter
    assert!(
        r.tokens_reduced < r.tokens_raw,
        "reduced tokens should be less than raw"
    );
    assert_eq!(r.hashes.len(), 1, "should have one stored hash");
    assert!(storage::exists(&r.hashes[0]), "hash should exist in CAS");
}

#[test]
fn deterministic_output() {
    let body = load_fixture("tool_result_cargo_test");
    let r1 = transform::reduce_body(&body, &make_config(), false).expect("first call");
    let r2 = transform::reduce_body(&body, &make_config(), false).expect("second call");
    assert_eq!(
        r1.modified_body, r2.modified_body,
        "reduction must be deterministic"
    );
    assert_eq!(r1.reductions, r2.reductions);
    assert_eq!(r1.hashes, r2.hashes, "same content must produce same hash");
}

#[test]
fn raw_bytes_roundtrip() {
    let data = b"hello world from toche integration test";
    let hash = storage::store(data).expect("store should succeed");
    let retrieved = storage::retrieve(&hash).expect("retrieve should succeed");
    assert_eq!(retrieved, data);
}

#[test]
fn bypass_header_disables_reduction() {
    let body = load_fixture("tool_result_cargo_test");
    let r = transform::reduce_body(&body, &make_config(), true).expect("reduce should succeed");
    assert_eq!(
        r.reductions, 0,
        "bypass header should disable all reductions"
    );
    assert_eq!(r.modified_body, body, "body should be unchanged");
}

#[test]
fn command_bypass_list_honored() {
    let cfg = ReduceConfig {
        enabled: true,
        command_bypass: vec!["shellcheck".to_string()],
    };
    let body = load_fixture("tool_result_cargo_test");
    let r = transform::reduce_body(&body, &cfg, false).expect("reduce should succeed");
    assert_eq!(
        r.reductions, 0,
        "command in bypass list should not be reduced"
    );
    assert_eq!(r.passthroughs, 1, "should count as passthrough");
}

#[test]
fn invalid_json_passthrough() {
    let body = "this is not valid JSON at all!";
    let r = transform::reduce_body(body, &make_config(), false).expect("should not error");
    assert_eq!(
        r.modified_body, body,
        "invalid JSON must pass through unchanged"
    );
    assert_eq!(r.reductions, 0);
    assert_eq!(r.passthroughs, 0);
}

#[test]
fn reduction_marker_present() {
    let body = load_fixture("tool_result_cargo_test");
    let r = transform::reduce_body(&body, &make_config(), false).expect("reduce should succeed");
    assert!(r.reductions > 0, "should have at least one reduction");
    assert!(
        r.modified_body.contains("[toche:reduced"),
        "marker must be present"
    );
    assert!(
        r.modified_body.contains("restored with: toche expand"),
        "marker must contain expand hint"
    );

    // Hash in marker should be valid and exist in CAS
    for hash in &r.hashes {
        assert!(
            storage::exists(hash),
            "hash {hash} from marker should exist in CAS"
        );
        let len = hash.len();
        assert_eq!(len, 64, "SHA-256 hex digest must be 64 chars, got {len}");
    }
}

#[test]
fn biome_output_reduced() {
    let body = load_fixture("tool_result_git_diff");
    let r = transform::reduce_body(&body, &make_config(), false).expect("reduce should succeed");
    assert!(
        r.reductions > 0,
        "biome should match a filter and be reduced"
    );
    assert!(
        r.modified_body.contains("toche:reduced"),
        "should contain reduction marker"
    );
}

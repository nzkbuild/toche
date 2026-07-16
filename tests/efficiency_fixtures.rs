use toche::efficiency::config::EfficiencyMode;
use toche::efficiency::inject;
use toche::efficiency::instructions;

fn load_fixture(name: &str) -> String {
    let path = format!("tests/fixtures/efficiency/{name}.json");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"))
}

#[test]
fn concise_mode_injects_instruction() {
    let body = load_fixture("body_with_system_array");
    let instruction = instructions::instruction_for_mode(&EfficiencyMode::Concise);
    let result = inject::inject_efficiency(&body, instruction).expect("injection should succeed");
    assert!(result.tokens_added > 0);
    assert_ne!(result.modified_body, body);
    let parsed: serde_json::Value = serde_json::from_str(&result.modified_body).unwrap();
    let system = parsed["system"].as_array().unwrap();
    assert_eq!(
        system.len(),
        2,
        "system should have original + instruction block"
    );
    assert_eq!(system[1]["type"], "text");
    assert!(system[1]["text"].as_str().unwrap().contains("concision"));
}

#[test]
fn careful_mode_injects_instruction() {
    let body = load_fixture("body_with_system_array");
    let instruction = instructions::instruction_for_mode(&EfficiencyMode::Careful);
    let result = inject::inject_efficiency(&body, instruction).expect("injection should succeed");
    assert!(result.tokens_added > 0);
    let parsed: serde_json::Value = serde_json::from_str(&result.modified_body).unwrap();
    let system = parsed["system"].as_array().unwrap();
    assert_eq!(system.len(), 2);
    assert!(system[1]["text"].as_str().unwrap().contains("assumptions"));
}

#[test]
fn normal_mode_no_injection() {
    let body = load_fixture("body_with_system_array");
    let instruction = instructions::instruction_for_mode(&EfficiencyMode::Normal);
    let result = inject::inject_efficiency(&body, instruction).expect("injection should succeed");
    assert_eq!(result.modified_body, body);
    assert_eq!(result.tokens_added, 0);
}

#[test]
fn bypass_header_disables_injection() {
    let body = load_fixture("body_with_system_array");
    // Bypass is implemented by passing None as instruction
    let result = inject::inject_efficiency(&body, None).expect("injection should succeed");
    assert_eq!(result.modified_body, body);
    assert_eq!(result.tokens_added, 0);
}

#[test]
fn system_string_converted_to_array() {
    let body = load_fixture("body_with_system_string");
    let instruction = instructions::instruction_for_mode(&EfficiencyMode::Concise);
    let result = inject::inject_efficiency(&body, instruction).expect("injection should succeed");
    let parsed: serde_json::Value = serde_json::from_str(&result.modified_body).unwrap();
    let system = parsed["system"].as_array().unwrap();
    assert_eq!(system.len(), 2);
    assert_eq!(system[0]["text"], "You are a helpful assistant.");
    assert_eq!(system[1]["type"], "text");
    assert!(system[1]["text"].as_str().unwrap().contains("concision"));
}

#[test]
fn no_system_key_unchanged() {
    let body = load_fixture("body_no_system");
    let instruction = instructions::instruction_for_mode(&EfficiencyMode::Concise);
    let result = inject::inject_efficiency(&body, instruction).expect("injection should succeed");
    assert_eq!(result.modified_body, body);
    assert_eq!(result.tokens_added, 0);
}

#[test]
fn deterministic_injection() {
    let body = load_fixture("body_with_system_array");
    let instruction = instructions::instruction_for_mode(&EfficiencyMode::Careful);
    let r1 = inject::inject_efficiency(&body, instruction).unwrap();
    let r2 = inject::inject_efficiency(&body, instruction).unwrap();
    assert_eq!(r1.modified_body, r2.modified_body);
    assert_eq!(r1.tokens_added, r2.tokens_added);
}

#[test]
fn invalid_json_passthrough() {
    let result = inject::inject_efficiency("not json", Some("be concise"));
    assert!(result.is_err());
}

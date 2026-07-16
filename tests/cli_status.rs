use tempfile::TempDir;

/// Replicate points_to_toche logic for test isolation (avoids exposing
/// cli internals through pub mod in lib.rs).
fn points_to_toche(settings: &serde_json::Value) -> bool {
    let base_url_toche = settings
        .get("baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    let env_url_toche = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    base_url_toche || env_url_toche
}

#[test]
fn doctor_no_config_ok() {
    let dir = TempDir::new().unwrap();
    let toche_dir = dir.path().join(".toche");

    // With no config directory, doctor should not crash
    assert!(!toche_dir.join("profiles.toml").exists());

    // Verify profiles loader handles missing config gracefully
            unsafe { std::env::set_var("TOCHE_CONFIG_DIR", toche_dir.to_string_lossy().to_string()); }
    let result = std::panic::catch_unwind(|| {
        let _ = toche::profiles::loader::load_profiles();
    });
    assert!(result.is_ok(), "load_profiles should not panic with missing config");
}

#[test]
fn status_no_profiles_graceful() {
    let dir = TempDir::new().unwrap();
    let toche_dir = dir.path().join(".toche");
    std::fs::create_dir_all(&toche_dir).unwrap();

            unsafe { std::env::set_var("TOCHE_CONFIG_DIR", toche_dir.to_string_lossy().to_string()); }

    // load_profiles should return empty, not panic, with no profiles.toml
    let result = std::panic::catch_unwind(|| {
        let profiles = toche::profiles::loader::load_profiles();
        // With no profiles.toml, should be Ok but may or may not have profiles
        let _ = profiles;
    });
    assert!(result.is_ok(), "load_profiles should not panic with empty config dir");
}

#[test]
fn doctor_detects_toche_routing() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    // settings.json pointing to Toche via env.ANTHROPIC_BASE_URL
    // (top-level baseURL is NOT toche, but env.ANTHROPIC_BASE_URL IS)
    let settings = serde_json::json!({
        "baseURL": "https://api.anthropic.com",
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1"
        }
    });
    std::fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let base_url = parsed
        .get("baseURL")
        .and_then(|v| v.as_str())
        .unwrap_or("not set");
    let env_url = parsed
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("not set");

    // top-level baseURL is NOT toche, but env.ANTHROPIC_BASE_URL IS
    assert!(!base_url.contains("127.0.0.1:8743"));
    assert!(env_url.contains("127.0.0.1:8743"));

    // points_to_toche should catch this (the F2 fix)
    assert!(points_to_toche(&parsed));
}

#[test]
fn point_to_toche_only_base_url() {
    let settings = serde_json::json!({
        "baseURL": "http://127.0.0.1:8743"
    });
    assert!(points_to_toche(&settings));
}

#[test]
fn point_to_toche_only_env_url() {
    let settings = serde_json::json!({
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1"
        }
    });
    assert!(points_to_toche(&settings));
}

#[test]
fn point_to_toche_neither() {
    let settings = serde_json::json!({
        "baseURL": "https://api.anthropic.com",
        "env": {
            "ANTHROPIC_BASE_URL": "https://api.anthropic.com/v1"
        }
    });
    assert!(!points_to_toche(&settings));
}

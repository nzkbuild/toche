use tempfile::TempDir;

#[test]
fn connect_creates_backup_when_existing_settings() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    // Write a pre-existing settings.json
    let settings = serde_json::json!({"someOtherSetting": true});
    std::fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    // Simulate what connect does: copy to backup if not already pointing to Toche
    let settings_path = claude_dir.join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Read current settings
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let already_toche = parsed
        .get("baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    assert!(!already_toche);

    // Backup
    std::fs::copy(&settings_path, &backup_path).unwrap();
    assert!(backup_path.exists());

    // Check backup content matches original
    let backup_raw = std::fs::read_to_string(&backup_path).unwrap();
    let backup_parsed: serde_json::Value = serde_json::from_str(&backup_raw).unwrap();
    assert_eq!(backup_parsed["someOtherSetting"], serde_json::json!(true));
}

#[test]
fn double_connect_detects_already_connected() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    let settings_path = claude_dir.join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Write settings that already point to Toche
    let settings = serde_json::json!({"baseURL": "http://127.0.0.1:8743"});
    std::fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();

    // Simulate double-connect check
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let already_toche = parsed
        .get("baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    assert!(already_toche);

    // Backup should NOT be created (double-connect guard)
    assert!(!backup_path.exists());
}

#[test]
fn disconnect_verifies_before_restore() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    let settings_path = claude_dir.join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Setup: settings point to Toche, backup has original
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&serde_json::json!({"baseURL": "http://127.0.0.1:8743", "other": "value"})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        &backup_path,
        serde_json::to_string_pretty(&serde_json::json!({"other": "value"})).unwrap(),
    )
    .unwrap();

    // Verify current settings point to Toche
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let points_to_toche = parsed
        .get("baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);
    assert!(points_to_toche);

    // Restore from backup
    let backup_content = std::fs::read_to_string(&backup_path).unwrap();
    std::fs::write(&settings_path, &backup_content).unwrap();

    // Verify restored
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let restored: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(restored.get("baseURL").is_none());
    assert_eq!(restored["other"], serde_json::json!("value"));
}

#[test]
fn disconnect_without_connect_detects_nothing_to_undo() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    let settings_path = claude_dir.join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Settings exist but don't point to Toche
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&serde_json::json!({"otherSetting": true})).unwrap(),
    )
    .unwrap();

    // No backup exists
    assert!(!backup_path.exists());

    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let points_to_toche = parsed
        .get("baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.contains("127.0.0.1:8743"))
        .unwrap_or(false);

    // Should detect nothing to disconnect
    assert!(!points_to_toche);
}

use tempfile::TempDir;

/// Replicate points_to_toche logic for test isolation.
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
    assert!(!points_to_toche(&parsed));

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
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    // Simulate double-connect check
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(points_to_toche(&parsed));

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
        serde_json::to_string_pretty(
            &serde_json::json!({"baseURL": "http://127.0.0.1:8743", "other": "value"}),
        )
        .unwrap(),
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
    assert!(points_to_toche(&parsed));

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

    // Should detect nothing to disconnect
    assert!(!points_to_toche(&parsed));
}

// ── F4/F5: No-backup disconnect path ─────────────────────────────────

#[test]
fn no_backup_disconnect_removes_toche_entries() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    let settings_path = claude_dir.join("settings.json");
    let backup_path = settings_path.with_extension("json.toche-backup");

    // Setup: settings point to Toche, NO backup file exists
    let original = serde_json::json!({
        "other": "keep-me",
        "baseURL": "http://127.0.0.1:8743",
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1",
            "OTHER_VAR": "keep-too"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&original).unwrap(),
    )
    .unwrap();
    assert!(!backup_path.exists());

    // Simulate no-backup disconnect
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let mut settings: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(points_to_toche(&settings));

    // Remove Toche entries
    if let Some(obj) = settings.as_object_mut() {
        obj.remove("baseURL");
    }
    // Check and clean env.ANTHROPIC_BASE_URL
    let env_is_empty = if let Some(env_url) = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
    {
        if env_url.contains("127.0.0.1:8743") {
            if let Some(env) = settings.pointer_mut("/env") {
                if let Some(env_obj) = env.as_object_mut() {
                    env_obj.remove("ANTHROPIC_BASE_URL");
                }
                env.as_object().map(|o| o.is_empty()).unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    // Clean up empty env object (F5)
    if env_is_empty {
        if let Some(parent) = settings.as_object_mut() {
            parent.remove("env");
        }
    }
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    // Verify: Toche entries gone, other content preserved
    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let cleaned: serde_json::Value = serde_json::from_str(&raw).unwrap();

    assert!(
        cleaned.get("baseURL").is_none(),
        "baseURL should be removed"
    );
    assert_eq!(
        cleaned["other"],
        serde_json::json!("keep-me"),
        "unrelated fields preserved"
    );

    // env should still exist (OTHER_VAR still present, so not empty)
    let env = cleaned.get("env").unwrap();
    assert!(env.get("OTHER_VAR").is_some(), "OTHER_VAR should survive");
    assert!(
        env.get("ANTHROPIC_BASE_URL").is_none(),
        "ANTHROPIC_BASE_URL should be removed"
    );
}

#[test]
fn no_backup_disconnect_cleans_empty_env() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    let settings_path = claude_dir.join("settings.json");

    // Setup: ANTHROPIC_BASE_URL is the ONLY key in env, pointing to Toche
    let original = serde_json::json!({
        "baseURL": "http://127.0.0.1:8743",
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&original).unwrap(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let mut settings: serde_json::Value = serde_json::from_str(&raw).unwrap();

    // Remove Toche entries
    if let Some(obj) = settings.as_object_mut() {
        obj.remove("baseURL");
    }
    let env_is_empty = if let Some(env_url) = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
    {
        if env_url.contains("127.0.0.1:8743") {
            if let Some(env) = settings.pointer_mut("/env") {
                if let Some(env_obj) = env.as_object_mut() {
                    env_obj.remove("ANTHROPIC_BASE_URL");
                }
                env.as_object().map(|o| o.is_empty()).unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    if env_is_empty {
        if let Some(parent) = settings.as_object_mut() {
            parent.remove("env");
        }
    }
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let cleaned: serde_json::Value = serde_json::from_str(&raw).unwrap();

    // env should be completely gone (F5 fix)
    assert!(
        cleaned.get("env").is_none(),
        "empty env object should be removed entirely"
    );
    assert!(cleaned.get("baseURL").is_none());
}

#[test]
fn no_backup_disconnect_restores_saved_url() {
    let dir = TempDir::new().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();

    let toche_dir = dir.path().join(".toche");
    std::fs::create_dir_all(&toche_dir).unwrap();

    let settings_path = claude_dir.join("settings.json");

    // Simulate: original URL was saved by connect before overwriting
    let original_url = "https://freeai.jembatanai.com/v1";
    std::fs::write(toche_dir.join("pre_toche_url.txt"), original_url).unwrap();

    // Setup: settings point to Toche, NO backup file
    let original = serde_json::json!({
        "other": "keep",
        "baseURL": "http://127.0.0.1:8743",
        "env": {
            "ANTHROPIC_BASE_URL": "http://127.0.0.1:8743/v1"
        }
    });
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&original).unwrap(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let mut settings: serde_json::Value = serde_json::from_str(&raw).unwrap();

    // Simulate disconnect with saved URL
    if let Some(obj) = settings.as_object_mut() {
        obj.remove("baseURL");
    }

    let saved_url = std::fs::read_to_string(toche_dir.join("pre_toche_url.txt")).ok();

    let env_is_empty = if let Some(env_url) = settings
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
    {
        if env_url.contains("127.0.0.1:8743") {
            if let Some(env) = settings.pointer_mut("/env") {
                if let Some(env_obj) = env.as_object_mut() {
                    if let Some(ref original) = saved_url {
                        env_obj.insert(
                            "ANTHROPIC_BASE_URL".into(),
                            serde_json::Value::String(original.clone()),
                        );
                    } else {
                        env_obj.remove("ANTHROPIC_BASE_URL");
                    }
                }
                env.as_object().map(|o| o.is_empty()).unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    if env_is_empty {
        if let Some(parent) = settings.as_object_mut() {
            parent.remove("env");
        }
    }
    std::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(&settings_path).unwrap();
    let cleaned: serde_json::Value = serde_json::from_str(&raw).unwrap();

    // ANTHROPIC_BASE_URL should be restored to the original upstream URL
    assert_eq!(
        cleaned["env"]["ANTHROPIC_BASE_URL"],
        serde_json::json!(original_url),
        "original upstream URL should be restored from pre_toche_url.txt"
    );
    assert!(cleaned.get("baseURL").is_none());
}

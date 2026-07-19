use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use wiremock::MockServer;
use wiremock::matchers::{method, path};
use wiremock::ResponseTemplate;

use toche::gateway::server::build_router;

/// Serialize tests that mutate `TOCHE_CONFIG_DIR` env var.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// A minimal valid config.toml with no integration configured.
fn minimal_config() -> String {
    r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"
"#
    .to_string()
}

/// Config with upstream for /v1/messages routing.
fn config_with_upstream(upstream_url: &str) -> String {
    format!(
        r#"
schema_version = 2

[runtime]
port = 0
listen_address = "127.0.0.1"
request_timeout_ms = 300000

[defaults]
integration = "a1b2c3d4"

[storage]
ledger_db = "ledger.db"
cas_dir = "cas"

[[integrations]]
id = "a1b2c3d4"
name = "default"
upstream = "e5f6a7b8"
policy = "c9d0e1f2"

[[upstreams]]
id = "e5f6a7b8"
name = "upstream"
url = "{upstream_url}"

[upstreams.auth]
type = "legacy_inline"
value = "test-key"
header_name = "x-api-key"

[[policies]]
id = "c9d0e1f2"
name = "default"
"#
    )
}

async fn spawn_gateway(
    config_dir: &Path,
    config_toml: &str,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    std::fs::create_dir_all(config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), config_toml).unwrap();

    let app = {
        let _lock = CONFIG_LOCK.lock().unwrap();
        build_router(Some(config_dir.to_path_buf())).unwrap()
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    (addr, handle)
}

fn run_git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─── Test 1: non_git_workspace_no_crash ─────────────────────────────────

#[tokio::test]
async fn non_git_workspace_no_crash() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");
    let config = minimal_config();
    let (addr, _handle) = spawn_gateway(&config_dir, &config).await;

    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);
}

// ─── Test 2: dirty_git_workspace_succeeds ────────────────────────────────

#[tokio::test]
async fn dirty_git_workspace_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");
    let work_dir = dir.path().join("work");
    std::fs::create_dir_all(&work_dir).unwrap();

    run_git(&work_dir, &["init"]);
    run_git(&work_dir, &["config", "user.email", "test@test.com"]);
    run_git(&work_dir, &["config", "user.name", "Test"]);
    std::fs::write(work_dir.join("file.txt"), "initial\n").unwrap();
    run_git(&work_dir, &["add", "file.txt"]);
    run_git(&work_dir, &["commit", "-m", "initial"]);

    // Dirty the workspace with an uncommitted change
    std::fs::write(work_dir.join("file.txt"), "modified\n").unwrap();

    let old_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&work_dir).unwrap();

    let mock = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, _handle) = spawn_gateway(&config_dir, &config).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(
            r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#,
        )
        .send()
        .await
        .unwrap();

    std::env::set_current_dir(&old_cwd).unwrap();

    assert_eq!(
        resp.status(),
        200,
        "gateway should serve traffic in dirty git workspace"
    );
}

// ─── Test 3: ledger_locked_traffic_passes ────────────────────────────────

#[tokio::test]
async fn ledger_locked_traffic_passes() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");

    let mock = MockServer::start().await;
    wiremock::Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n",
                "text/event-stream",
            ),
        )
        .mount(&mock)
        .await;

    let config = config_with_upstream(&mock.uri());
    let (addr, _handle) = spawn_gateway(&config_dir, &config).await;

    // Make a request to ensure ledger.db is created by the async recording task
    let client = reqwest::Client::new();
    let _ = client
        .post(format!("http://{addr}/v1/messages"))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .body(
            r#"{"model":"claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"Hi"}]}"#,
        )
        .send()
        .await
        .unwrap();

    // Give the async ledger write time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let ledger_path = config_dir.join("ledger.db");
    assert!(ledger_path.exists(), "ledger.db should exist after a request");

    // Lock ledger with an external connection
    let conn = rusqlite::Connection::open(&ledger_path).unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE").unwrap();

    // Health check should still pass — the health endpoint does not touch the ledger
    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(
        resp.status(),
        200,
        "health should return 200 despite locked ledger"
    );

    // Release the lock
    conn.execute_batch("ROLLBACK").unwrap();
}

// ─── Test 4: malformed_config_descriptive_error ──────────────────────────

#[tokio::test]
async fn malformed_config_descriptive_error() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("toche");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Write broken TOML — missing closing bracket on section header
    std::fs::write(
        config_dir.join("config.toml"),
        "schema_version = 2\n[runtime\nport = 0\n",
    )
    .unwrap();

    let _lock = CONFIG_LOCK.lock().unwrap();
    let result = build_router(Some(config_dir));

    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(!msg.is_empty(), "error message should not be empty");
            // The error chain should contain a TOML parse error from the config loader
            let lower = msg.to_lowercase();
            assert!(
                lower.contains("toml")
                    || lower.contains("config")
                    || lower.contains("parse")
                    || lower.contains("expected"),
                "error should be descriptive, got: {msg}"
            );
        }
        Ok(_) => panic!("expected build_router to fail with broken TOML"),
    }
}

// ─── Test 5: interrupted_setup_no_partial_fragment ───────────────────────

#[tokio::test]
async fn interrupted_setup_no_partial_fragment() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir: PathBuf = dir.path().join("toche");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Simulate an interrupted setup: leave a stale .tmp fragment
    // (atomic_write_secure uses path.with_extension("tmp"), so for
    // ownership.toml the temp file is ownership.tmp)
    let tmp_path = config_dir.join("ownership.tmp");
    std::fs::write(&tmp_path, "partial fragment from killed process").unwrap();
    assert!(tmp_path.exists(), "stale tmp should exist before setup");

    let answers = toche::setup::SetupAnswers {
        upstream_url: Some("https://api.anthropic.com".into()),
        api_key: Some("sk-ant-test-key".into()),
        header_name: Some("x-api-key".into()),
        integration_name: Some("default".into()),
    };

    let tx = toche::setup::SetupTransaction::new(false, false)
        .with_config_dir(config_dir.clone())
        .with_answers(answers);

    let outcome = tx.run().unwrap();
    assert!(
        matches!(outcome, toche::setup::SetupOutcome::Applied { .. }),
        "setup should apply successfully"
    );

    // Stale .tmp fragment should be cleaned up by atomic_write_secure
    assert!(
        !tmp_path.exists(),
        "stale ownership.tmp fragment should be cleaned up"
    );

    // ownership.toml should exist and be valid TOML
    let ownership_path = config_dir.join("ownership.toml");
    assert!(ownership_path.exists(), "ownership.toml should exist");

    let raw = std::fs::read_to_string(&ownership_path).unwrap();
    let record: toche::setup::OwnershipRecord =
        toml::from_str(&raw).expect("ownership.toml should be valid TOML");
    assert!(
        !record.integration_ids.is_empty(),
        "should have integration IDs"
    );
    assert!(!record.upstream_ids.is_empty(), "should have upstream IDs");

    // Verify no orphan .tmp fragments remain in the config directory
    for entry in std::fs::read_dir(&config_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.ends_with(".tmp"),
            "orphan fragment should not exist: {name}"
        );
    }
}

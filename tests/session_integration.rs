//! Integration tests for named MCP session daemons.

#![cfg(unix)]

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::TempDir;

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn python() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".into())
}

fn mcp_stdio_cmd() -> String {
    format!(
        "{} {}",
        python(),
        fixtures_dir().join("mcp_test_server.py").display()
    )
}

struct IsolatedEnv {
    _dir: TempDir,
    cache: PathBuf,
}

impl IsolatedEnv {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        Self { _dir: dir, cache }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::new(cargo_bin!("mcp2cli"));
        cmd.env("MCP2CLI_CACHE_DIR", &self.cache);
        cmd
    }

    fn sock(&self, name: &str) -> PathBuf {
        self.cache.join("sessions").join(format!("{name}.sock"))
    }

    fn stop(&self, name: &str) {
        let _ = self.cmd().args(["--session-stop", name]).output();
    }
}

#[test]
fn session_lifecycle() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let name = "test-lifecycle";
    let server = mcp_stdio_cmd();

    env.cmd()
        .args([
            "--mcp-stdio",
            &server,
            "--session-start",
            name,
            "--session-idle-secs",
            "1800",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(name));

    // Ensure cleanup even on assertion failure.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.cmd()
            .args(["--session-list"])
            .assert()
            .success()
            .stdout(predicate::str::contains(name))
            .stdout(predicate::str::contains("alive"));

        let list_json = env
            .cmd()
            .args(["--session-list", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let entries: Value = serde_json::from_slice(&list_json).unwrap();
        assert!(entries
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == name && e["alive"] == true));

        let sock = env.sock(name);
        assert!(sock.exists());
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "socket should be 0o600, got {mode:#o}");

        env.cmd()
            .args(["--session", name, "echo", "--message", "via session"])
            .assert()
            .success()
            .stdout(predicate::str::contains("via session"));

        env.cmd()
            .args(["--session", name, "--list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("echo"));

        let resources = env
            .cmd()
            .args(["--session", name, "--list-resources"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let data: Value = serde_json::from_slice(&resources).unwrap();
        assert!(data
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["name"] == "Test Document"));

        let prompts = env
            .cmd()
            .args(["--session", name, "--list-prompts"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let data: Value = serde_json::from_slice(&prompts).unwrap();
        assert!(data
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["name"] == "greeting"));
    }));

    env.stop(name);
    result.unwrap();

    let out = env
        .cmd()
        .args(["--session-list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(!text.contains(name) || text.contains("dead") || text.contains("No sessions"));
}

#[test]
fn session_idle_timeout_exits() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let name = "test-idle";
    let server = mcp_stdio_cmd();

    env.cmd()
        .args([
            "--mcp-stdio",
            &server,
            "--session-start",
            name,
            "--session-idle-secs",
            "1",
        ])
        .assert()
        .success();

    // Touch once so last_activity is set, then wait for idle exit.
    let _ = env.cmd().args(["--session", name, "--list"]).output();

    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if !env.sock(name).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        !env.sock(name).exists(),
        "session socket should be removed after idle timeout"
    );
    env.stop(name);
}

#[test]
fn session_duplicate_start_rejected() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let name = "test-dup";
    let server = mcp_stdio_cmd();

    env.cmd()
        .args(["--mcp-stdio", &server, "--session-start", name])
        .assert()
        .success();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        env.cmd()
            .args(["--mcp-stdio", &server, "--session-start", name])
            .assert()
            .failure()
            .stderr(predicate::str::contains("already running"));
    }));
    env.stop(name);
    result.unwrap();
}

#[test]
fn bake_session_emits_flag() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("config");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&cache).unwrap();

    let mut create = Command::new(cargo_bin!("mcp2cli"));
    create
        .env("MCP2CLI_CONFIG_DIR", &config)
        .env("MCP2CLI_CACHE_DIR", &cache)
        .args([
            "bake",
            "create",
            "sessbake",
            "--mcp-stdio",
            &mcp_stdio_cmd(),
            "--session",
            "warm",
        ])
        .assert()
        .success();

    let mut show = Command::new(cargo_bin!("mcp2cli"));
    let out = show
        .env("MCP2CLI_CONFIG_DIR", &config)
        .env("MCP2CLI_CACHE_DIR", &cache)
        .args(["bake", "show", "sessbake"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data["session"], "warm");
}

//! Token-efficiency feature tests against the local MCP stdio fixture.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::tempdir;

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn python() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".into())
}

fn mcp2cli_with_cache() -> (Command, PathBuf) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let mut cmd = Command::new(cargo_bin!("mcp2cli"));
    cmd.env("MCP2CLI_CACHE_DIR", &cache);
    std::mem::forget(dir);
    (cmd, cache)
}

fn stdio_cmd() -> String {
    let server = fixtures_dir().join("mcp_test_server.py");
    format!("{} {}", python(), server.display())
}

#[test]
fn list_detail_names_brief_full_ordering() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();

    let (mut c, _) = mcp2cli_with_cache();
    let names = c
        .args(["--mcp-stdio", &cmd, "--list", "--json", "--detail", "names"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let (mut c, _) = mcp2cli_with_cache();
    let brief = c
        .args(["--mcp-stdio", &cmd, "--list", "--json", "--detail", "brief"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let (mut c, _) = mcp2cli_with_cache();
    let full = c
        .args(["--mcp-stdio", &cmd, "--list", "--json", "--detail", "full"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert!(names.len() < brief.len(), "names < brief");
    assert!(brief.len() < full.len(), "brief < full");

    let names_v: Value = serde_json::from_slice(&names).unwrap();
    assert!(names_v.as_array().unwrap()[0].is_string());

    let brief_v: Value = serde_json::from_slice(&brief).unwrap();
    assert!(brief_v.as_array().unwrap()[0].get("name").is_some());
    assert!(brief_v.as_array().unwrap()[0].get("parameters").is_none());

    let full_v: Value = serde_json::from_slice(&full).unwrap();
    assert!(full_v.as_array().unwrap()[0].get("parameters").is_some());
}

#[test]
fn help_json_and_describe() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();

    let (mut c, _) = mcp2cli_with_cache();
    let out = c
        .args(["--mcp-stdio", &cmd, "--json", "echo", "--help"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["name"], "echo");
    assert!(v["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "message"));

    let (mut c, _) = mcp2cli_with_cache();
    let out = c
        .args(["--mcp-stdio", &cmd, "--describe", "add-numbers", "--json"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["name"], "add-numbers");
}

#[test]
fn json_content_only_vs_envelope() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();

    let (mut c, _) = mcp2cli_with_cache();
    let content = c
        .args(["--mcp-stdio", &cmd, "--json", "echo", "--message", "hello"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&content).unwrap();
    assert_eq!(v, "hello");
    assert!(v.get("content").is_none());

    let (mut c, _) = mcp2cli_with_cache();
    let env = c
        .args([
            "--mcp-stdio",
            &cmd,
            "--envelope",
            "--json",
            "echo",
            "--message",
            "hello",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&env).unwrap();
    assert!(
        v.get("content").is_some()
            || v.as_object()
                .map(|o| o.contains_key("content"))
                .unwrap_or(false)
    );
    assert!(env.len() > content.len());
}

#[test]
fn native_toon_uniform_list() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();

    let (mut c, _) = mcp2cli_with_cache();
    c.args([
        "--mcp-stdio",
        &cmd,
        "--toon",
        "uniform-list",
        "--count",
        "3",
    ])
    .timeout(Duration::from_secs(30))
    .assert()
    .success()
    .stdout(predicate::str::contains("id"))
    .stdout(predicate::str::contains("name"));
}

#[test]
fn spool_on_max_bytes() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();
    let (mut c, cache) = mcp2cli_with_cache();

    let out = c
        .args([
            "--mcp-stdio",
            &cmd,
            "--json",
            "--max-bytes",
            "200",
            "big-payload",
            "--size",
            "5000",
            "--needle",
            "UNIQUE_NEEDLE_XYZ",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["spooled"], true);
    let path = v["path"].as_str().unwrap();
    assert!(std::path::Path::new(path).exists());
    assert!(path.contains("spool"));
    let body = std::fs::read_to_string(path).unwrap();
    assert!(body.contains("UNIQUE_NEEDLE_XYZ"));
    assert!(body.len() > 4000);
    assert!(
        out.len() < body.len() / 2,
        "pointer should be much smaller than body: {} vs {}",
        out.len(),
        body.len()
    );
    assert!(cache.join("spool").is_dir() || path.contains("spool"));
}

#[test]
fn tiny_image_spools_not_inline_base64() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();
    let (mut c, _) = mcp2cli_with_cache();

    let out = c
        .args(["--mcp-stdio", &cmd, "--json", "tiny-image"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(!text.contains("iVBORw0KGgo"));
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["type"], "image");
    assert_eq!(v["spooled"], true);
    assert!(v.get("path").is_some());
}

#[test]
fn agent_flag_implies_json_list() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cmd = stdio_cmd();
    let (mut c, _) = mcp2cli_with_cache();

    let out = c
        .args(["--mcp-stdio", &cmd, "--agent", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert!(v.as_array().unwrap()[0].get("name").is_some());
    assert!(!String::from_utf8_lossy(&out).contains("Available tools"));
}

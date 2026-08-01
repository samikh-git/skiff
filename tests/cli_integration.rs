//! Integration tests for OpenAPI and MCP stdio CLI modes.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command as StdCommand, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tempfile::tempdir;

static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn python() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| "python3".into())
}

struct Petstore {
    child: Child,
    base: String,
}

impl Petstore {
    fn start() -> Self {
        let script = fixtures_dir().join("petstore_server.py");
        let mut child = StdCommand::new(python())
            .arg(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn petstore_server.py");
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read petstore URL");
        let base = line.trim().to_string();
        assert!(base.starts_with("http://"), "unexpected URL: {base}");
        // wait until openapi responds
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(resp) = reqwest::blocking::get(format!("{base}/openapi.json")) {
                if resp.status().is_success() {
                    break;
                }
            }
            if Instant::now() > deadline {
                panic!("petstore did not become ready");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Self { child, base }
    }
}

impl Drop for Petstore {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn skiff() -> Command {
    let mut cmd = Command::new(cargo_bin!("skiff"));
    let dir = tempdir().unwrap();
    // Keep cache isolated; leak dir for process lifetime of this command (ok in tests).
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    cmd.env("SKIFF_CACHE_DIR", &cache);
    // Prevent tempfile from deleting before command runs by keeping ownership in env side channel —
    // use a unique path under /tmp via std instead:
    std::mem::forget(dir);
    cmd
}

#[test]
fn openapi_list_and_get_pet() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = Petstore::start();
    let spec = format!("{}/openapi.json", server.base);
    let base = format!("{}/api/v1", server.base);

    skiff()
        .args(["--spec", &spec, "--base-url", &base, "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list-pets"))
        .stdout(predicate::str::contains("create-pet"));

    let out = skiff()
        .args([
            "--spec",
            &spec,
            "--base-url",
            &base,
            "--pretty",
            "get-pet",
            "--pet-id",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data["name"], "Fido");
}

#[test]
fn openapi_list_pets_limit_and_create() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = Petstore::start();
    let spec = format!("{}/openapi.json", server.base);
    let base = format!("{}/api/v1", server.base);

    let out = skiff()
        .args([
            "--spec",
            &spec,
            "--base-url",
            &base,
            "list-pets",
            "--limit",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data.as_array().unwrap().len(), 1);

    let out = skiff()
        .args([
            "--spec",
            &spec,
            "--base-url",
            &base,
            "create-pet",
            "--name",
            "Buddy",
            "--tag",
            "dog",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data["name"], "Buddy");
    assert!(data.get("id").is_some());
}

#[test]
fn openapi_load_local_json_file() {
    let spec = fixtures_dir().join("petstore.json");
    let data =
        skiff_cli::openapi::load_openapi_spec(spec.to_str().unwrap(), &[], None, Some(3600), false)
            .unwrap();
    assert!(data.get("paths").unwrap().get("/pets").is_some());
}

#[test]
fn mcp_stdio_list_and_echo() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = fixtures_dir().join("mcp_test_server.py");
    let stdio_cmd = format!("{} {}", python(), server.display());

    skiff()
        .args(["--mcp-stdio", &stdio_cmd, "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("add-numbers"));

    skiff()
        .args([
            "--mcp-stdio",
            &stdio_cmd,
            "echo",
            "--message",
            "hello world",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world"));

    skiff()
        .args([
            "--mcp-stdio",
            &stdio_cmd,
            "add-numbers",
            "--a",
            "3",
            "--b",
            "7",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("10"));
}

#[test]
fn mcp_stdio_search_and_env_not_shadowed() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = fixtures_dir().join("mcp_test_server.py");
    let stdio_cmd = format!("{} {}", python(), server.display());

    skiff()
        .args(["--mcp-stdio", &stdio_cmd, "--search", "echo"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("add-numbers").not());

    let out = skiff()
        .args(["--mcp-stdio", &stdio_cmd, "deploy", "--env", "production"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data["env"], "production");
}

//! Integration tests for bake CRUD and `@name` execution.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as StdCommand, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
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
    config: PathBuf,
    cache: PathBuf,
}

impl IsolatedEnv {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("config");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        Self {
            _dir: dir,
            config,
            cache,
        }
    }

    fn cmd(&self) -> Command {
        let mut cmd = Command::new(cargo_bin!("skiff"));
        cmd.env("SKIFF_CONFIG_DIR", &self.config);
        cmd.env("SKIFF_CACHE_DIR", &self.cache);
        cmd
    }
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

#[test]
fn bake_create_list_show_remove() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();

    env.cmd()
        .args(["bake", "create", "test-echo", "--mcp-stdio", &stdio])
        .assert()
        .success()
        .stdout(predicate::str::contains("created"));

    env.cmd()
        .args(["bake", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-echo"));

    let out = env
        .cmd()
        .args(["bake", "show", "test-echo"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data["source_type"], "mcp_stdio");

    env.cmd()
        .args(["bake", "remove", "test-echo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));

    env.cmd()
        .args(["bake", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-echo").not());
}

#[test]
fn bake_create_duplicate_and_force() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();

    env.cmd()
        .args(["bake", "create", "dup", "--mcp-stdio", &stdio])
        .assert()
        .success();

    env.cmd()
        .args(["bake", "create", "dup", "--mcp-stdio", &stdio])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    env.cmd()
        .args(["bake", "create", "dup", "--force", "--mcp-stdio", &stdio])
        .assert()
        .success();
}

#[test]
fn bake_update_and_show_exclude() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();

    env.cmd()
        .args([
            "bake",
            "create",
            "showme",
            "--mcp-stdio",
            &stdio,
            "--exclude",
            "deploy",
        ])
        .assert()
        .success();

    let out = env
        .cmd()
        .args(["bake", "show", "showme"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert!(data["exclude"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str() == Some("deploy")));

    env.cmd()
        .args(["bake", "create", "upd", "--mcp-stdio", &stdio])
        .assert()
        .success();
    env.cmd()
        .args(["bake", "update", "upd", "--cache-ttl", "9999"])
        .assert()
        .success();

    let out = env
        .cmd()
        .args(["bake", "show", "upd"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(data["cache_ttl"], 9999);
}

#[test]
fn bake_at_name_list_and_execute() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();

    env.cmd()
        .args(["bake", "create", "mytools", "--mcp-stdio", &stdio])
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    env.cmd()
        .args(["@mytools", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("add-numbers"));

    env.cmd()
        .args(["@mytools", "echo", "--message", "hello"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn bake_include_exclude_filters() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();

    env.cmd()
        .args([
            "bake",
            "create",
            "filtered",
            "--mcp-stdio",
            &stdio,
            "--include",
            "echo",
        ])
        .assert()
        .success();

    env.cmd()
        .args(["@filtered", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("add-numbers").not());

    env.cmd()
        .args([
            "bake",
            "create",
            "no-deploy",
            "--mcp-stdio",
            &stdio,
            "--exclude",
            "deploy",
        ])
        .assert()
        .success();

    env.cmd()
        .args(["@no-deploy", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("deploy").not());
}

#[test]
fn bake_errors() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();

    env.cmd()
        .args(["@nope", "--list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no baked tool"));

    env.cmd()
        .args(["bake", "create", "Bad-Name", "--mcp-stdio", &stdio])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid name"));

    env.cmd()
        .args(["bake", "create", "nosrc"])
        .assert()
        .failure();
}

#[test]
fn bake_openapi_methods_filter() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = Petstore::start();
    let env = IsolatedEnv::new();
    let spec = format!("{}/openapi.json", server.base);

    env.cmd()
        .args([
            "bake",
            "create",
            "pets",
            "--spec",
            &spec,
            "--methods",
            "GET",
        ])
        .assert()
        .success();

    env.cmd()
        .args(["@pets", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("list-pets"))
        .stdout(predicate::str::contains("get-pet"))
        .stdout(predicate::str::contains("create-pet").not())
        .stdout(predicate::str::contains("delete-pet").not())
        .stdout(predicate::str::contains("update-pet").not());
}

#[test]
fn bake_install_custom_dir() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();
    let custom = env._dir.path().join("custom_bin");

    env.cmd()
        .args(["bake", "create", "dir-test", "--mcp-stdio", &stdio])
        .assert()
        .success();

    env.cmd()
        .args([
            "bake",
            "install",
            "dir-test",
            "--dir",
            custom.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed wrapper"))
        .stdout(predicate::str::contains("may not be in your PATH").not());

    let wrapper = custom.join("dir-test");
    assert!(wrapper.exists());
    let content = std::fs::read_to_string(&wrapper).unwrap();
    assert!(content.contains("@dir-test"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&wrapper).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }
}

#[test]
fn bake_install_default_dir_path_note() {
    // Only verify wrapper content when we can write under a temp "home".
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let env = IsolatedEnv::new();
    let stdio = mcp_stdio_cmd();
    let custom = env._dir.path().join("scripts");

    env.cmd()
        .args(["bake", "create", "warn-test", "--mcp-stdio", &stdio])
        .assert()
        .success();

    env.cmd()
        .args([
            "bake",
            "install",
            "warn-test",
            "--dir",
            custom.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("may not be in your PATH").not());

    assert!(Path::new(&custom).join("warn-test").exists());
}

//! GraphQL integration tests against the Python fixture server.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
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

struct GraphqlServer {
    child: Child,
    url: String,
}

impl GraphqlServer {
    fn start() -> Self {
        let script = fixtures_dir().join("graphql_server.py");
        let mut child = StdCommand::new(python())
            .arg(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", script.display()));
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut url = None;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                if child.try_wait().ok().flatten().is_some() {
                    panic!("{} exited before ready", script.display());
                }
                continue;
            }
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("URL=") {
                url = Some(rest.to_string());
                break;
            }
        }
        let url = url.expect("graphql fixture missing URL=");
        std::thread::sleep(Duration::from_millis(100));
        Self { child, url }
    }
}

impl Drop for GraphqlServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn skiff_isolated() -> (Command, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    let config = dir.path().join("config");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let mut cmd = Command::new(cargo_bin!("skiff"));
    cmd.env("SKIFF_CACHE_DIR", &cache);
    cmd.env("SKIFF_CONFIG_DIR", &config);
    (cmd, dir)
}

#[test]
fn graphql_list_and_query() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = GraphqlServer::start();

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("users"))
        .stdout(predicate::str::contains("create-user"));

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "users"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("Alice"));

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "user", "--id", "1"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("Alice"));
}

#[test]
fn graphql_mutation_and_fields() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = GraphqlServer::start();

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args([
        "--graphql",
        &server.url,
        "create-user",
        "--name",
        "Charlie",
        "--email",
        "charlie@example.com",
    ])
    .timeout(Duration::from_secs(30))
    .assert()
    .success()
    .stdout(predicate::str::contains("Charlie"));

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args([
        "--graphql",
        &server.url,
        "--fields",
        "id name",
        "user",
        "--id",
        "1",
    ])
    .timeout(Duration::from_secs(30))
    .assert()
    .success()
    .stdout(predicate::str::contains("Alice"));
}

#[test]
fn graphql_list_args_head_search_stdin() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = GraphqlServer::start();

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "users-by-ids", "--ids", "1,2"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("Alice"))
        .stdout(predicate::str::contains("Bob"));

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "--head", "1", "users"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "--search", "create", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("create-user"))
        .stdout(predicate::str::contains("users").not());

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "create-user", "--stdin"])
        .write_stdin(r#"{"name":"Dana","email":"dana@example.com"}"#)
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("Dana"));
}

#[test]
fn graphql_mutual_exclusion_and_missing_required() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = GraphqlServer::start();
    let petstore = fixtures_dir().join("petstore.json");

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args([
        "--graphql",
        &server.url,
        "--spec",
        petstore.to_str().unwrap(),
        "--list",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("mutually exclusive"));

    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--graphql", &server.url, "user"])
        .timeout(Duration::from_secs(30))
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing required"));
}

#[test]
fn graphql_bake_create_and_at_name() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = GraphqlServer::start();
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    let config = dir.path().join("config");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&config).unwrap();

    Command::new(cargo_bin!("skiff"))
        .env("SKIFF_CACHE_DIR", &cache)
        .env("SKIFF_CONFIG_DIR", &config)
        .args([
            "bake",
            "create",
            "gqlapi",
            "--graphql",
            &server.url,
            "--exclude",
            "delete-user",
        ])
        .assert()
        .success();

    Command::new(cargo_bin!("skiff"))
        .env("SKIFF_CACHE_DIR", &cache)
        .env("SKIFF_CONFIG_DIR", &config)
        .args(["@gqlapi", "--list"])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("users"))
        .stdout(predicate::str::contains("delete-user").not());
}

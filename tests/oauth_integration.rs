//! Streamable MCP + OAuth client-credentials integration.

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

struct OAuthServer {
    child: Child,
    url: String,
    client_id: String,
    client_secret: String,
}

impl OAuthServer {
    fn start() -> Self {
        let script = fixtures_dir().join("oauth_mcp_server.py");
        let mut child = StdCommand::new(python())
            .arg(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", script.display()));
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut url = None;
        let mut client_id = None;
        let mut client_secret = None;
        let deadline = Instant::now() + Duration::from_secs(20);
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
            } else if let Some(rest) = line.strip_prefix("CLIENT_ID=") {
                client_id = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("CLIENT_SECRET=") {
                client_secret = Some(rest.to_string());
            }
            if url.is_some() && client_id.is_some() && client_secret.is_some() {
                break;
            }
        }
        let url = url.expect("oauth fixture missing URL=");
        let client_id = client_id.expect("oauth fixture missing CLIENT_ID=");
        let client_secret = client_secret.expect("oauth fixture missing CLIENT_SECRET=");
        std::thread::sleep(Duration::from_millis(300));
        Self {
            child,
            url,
            client_id,
            client_secret,
        }
    }
}

impl Drop for OAuthServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn skiff_isolated() -> (Command, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let mut cmd = Command::new(cargo_bin!("skiff"));
    cmd.env("SKIFF_CACHE_DIR", &cache);
    (cmd, dir)
}

#[test]
fn oauth_client_credentials_list_and_echo() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = OAuthServer::start();

    let (mut cmd, _dir) = skiff_isolated();
    cmd.env("SKIFF_TEST_OAUTH_CLIENT_ID", &server.client_id)
        .env("SKIFF_TEST_OAUTH_CLIENT_SECRET", &server.client_secret)
        .args([
            "--mcp",
            &server.url,
            "--transport",
            "streamable",
            "--oauth-flow",
            "client_credentials",
            "--oauth-client-id",
            "env:SKIFF_TEST_OAUTH_CLIENT_ID",
            "--oauth-client-secret",
            "env:SKIFF_TEST_OAUTH_CLIENT_SECRET",
            "--list",
        ])
        .timeout(Duration::from_secs(60))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"));

    let (mut cmd, _dir) = skiff_isolated();
    cmd.env("SKIFF_TEST_OAUTH_CLIENT_ID", &server.client_id)
        .env("SKIFF_TEST_OAUTH_CLIENT_SECRET", &server.client_secret)
        .args([
            "--mcp",
            &server.url,
            "--transport",
            "streamable",
            "--oauth-client-id",
            "env:SKIFF_TEST_OAUTH_CLIENT_ID",
            "--oauth-client-secret",
            "env:SKIFF_TEST_OAUTH_CLIENT_SECRET",
            "echo",
            "--message",
            "oauth ok",
        ])
        .timeout(Duration::from_secs(60))
        .assert()
        .success()
        .stdout(predicate::str::contains("oauth ok"));
}

#[test]
fn oauth_list_without_creds_fails() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = OAuthServer::start();

    // No OAuth: should not get past Bearer gate (or fail discovery/connect).
    let (mut cmd, _dir) = skiff_isolated();
    cmd.args(["--mcp", &server.url, "--transport", "streamable", "--list"])
        .timeout(Duration::from_secs(45))
        .assert()
        .failure();
}

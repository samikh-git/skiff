//! Integration tests for MCP HTTP (streamable) and legacy SSE transports.

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

fn mcp2cli() -> Command {
    let mut cmd = Command::new(cargo_bin!("mcp2cli"));
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    cmd.env("MCP2CLI_CACHE_DIR", &cache);
    std::mem::forget(dir);
    cmd
}

struct HttpServer {
    child: Child,
    url: String,
}

impl HttpServer {
    fn start(script: &str, url_prefix: &str) -> Self {
        let script = fixtures_dir().join(script);
        let mut child = StdCommand::new(python())
            .arg(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", script.display()));
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        let mut url = None;
        let deadline = Instant::now() + Duration::from_secs(15);
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
            if line.starts_with("PORT=") && url.is_none() {
                // keep reading for URL=
            }
        }
        let url = url.unwrap_or_else(|| panic!("{} did not report URL=", script.display()));
        assert!(
            url.starts_with(url_prefix),
            "unexpected URL {url} (wanted prefix {url_prefix})"
        );
        // Brief settle for uvicorn bind
        std::thread::sleep(Duration::from_millis(200));
        Self { child, url }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_streamable_list_and_echo() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = HttpServer::start("mcp_http_server.py", "http://");

    mcp2cli()
        .args(["--mcp", &server.url, "--transport", "streamable", "--list"])
        .timeout(Duration::from_secs(45))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"))
        .stdout(predicate::str::contains("add-numbers"));

    mcp2cli()
        .args([
            "--mcp",
            &server.url,
            "--transport",
            "streamable",
            "echo",
            "--message",
            "http test",
        ])
        .timeout(Duration::from_secs(45))
        .assert()
        .success()
        .stdout(predicate::str::contains("http test"));

    mcp2cli()
        .args([
            "--mcp",
            &server.url,
            "--transport",
            "streamable",
            "add-numbers",
            "--a",
            "10",
            "--b",
            "20",
        ])
        .timeout(Duration::from_secs(45))
        .assert()
        .success()
        .stdout(predicate::str::contains("30"));
}

#[test]
fn mcp_sse_list_and_echo() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let server = HttpServer::start("mcp_sse_server.py", "http://");

    mcp2cli()
        .args(["--mcp", &server.url, "--transport", "sse", "--list"])
        .timeout(Duration::from_secs(45))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"));

    mcp2cli()
        .args([
            "--mcp",
            &server.url,
            "--transport",
            "sse",
            "echo",
            "--message",
            "sse test",
        ])
        .timeout(Duration::from_secs(45))
        .assert()
        .success()
        .stdout(predicate::str::contains("sse test"));
}

#[test]
fn mcp_auto_falls_back_to_sse() {
    let _g = FIXTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // SSE-only URL: streamable should fail, auto should fall back.
    let server = HttpServer::start("mcp_sse_server.py", "http://");

    mcp2cli()
        .args(["--mcp", &server.url, "--transport", "auto", "--list"])
        .timeout(Duration::from_secs(45))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo"));
}
